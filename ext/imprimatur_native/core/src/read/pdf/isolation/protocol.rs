//! A versioned, bounded reply and numeric-only worker arguments.

use std::ffi::OsString;
use std::io::Write;

use super::ProcessLimits;
use crate::read::Limits;
use crate::read::pdf::RenderOptions;
use crate::{Error, Result};

pub(super) const HEADER: &[u8; 6] = b"IMPR\x01\0";
pub(super) const MAX_ERROR_BYTES: usize = 4096;

pub(super) struct Request {
    pub number: u32,
    pub options: RenderOptions,
    pub limits: Limits,
    pub memory: u64,
    pub cpu_seconds: u64,
}

impl Request {
    pub fn new(
        number: u32,
        options: &RenderOptions,
        limits: &Limits,
        process: &ProcessLimits,
    ) -> Result<Self> {
        options
            .max_output_bytes
            .checked_add(HEADER.len())
            .ok_or(Error::InvalidPdfOption("output limit is too large"))?;
        if process.memory_bytes == 0 || process.memory_bytes >= i64::MAX as u64 {
            return Err(Error::InvalidPdfOption("memory limit must be positive and finite"));
        }
        let cpu_seconds = process
            .wall_time
            .as_secs()
            .checked_add(u64::from(process.wall_time.subsec_nanos() != 0))
            .filter(|&seconds| seconds != 0 && seconds < i64::MAX as u64)
            .ok_or(Error::InvalidPdfOption("worker wall time must be positive and finite"))?;
        Ok(Self {
            number,
            options: options.clone(),
            limits: limits.clone(),
            memory: process.memory_bytes,
            cpu_seconds,
        })
    }

    pub fn arguments(&self) -> Vec<OsString> {
        [
            u64::from(self.number),
            u64::from(self.options.max_side),
            self.options.max_pixels,
            self.options.max_output_bytes as u64,
            u64::from(self.options.grayscale),
            self.limits.max_input as u64,
            self.limits.max_inflated,
            self.limits.max_depth as u64,
            self.memory,
            self.cpu_seconds,
        ]
        .map(|number| OsString::from(number.to_string()))
        .to_vec()
    }

    pub fn parse() -> Result<Self> {
        let invalid = || Error::InvalidPdfOption("invalid render worker arguments");
        let values: Vec<u64> = std::env::args()
            .skip(1)
            .map(|value| value.parse().map_err(|_| invalid()))
            .collect::<Result<_>>()?;
        let [number, side, pixels, output, gray, input, inflated, depth, memory, cpu] =
            values.as_slice()
        else {
            return Err(invalid());
        };
        if *gray > 1
            || *memory == 0
            || *memory >= i64::MAX as u64
            || *cpu == 0
            || *cpu >= i64::MAX as u64
        {
            return Err(invalid());
        }
        Ok(Self {
            number: u32::try_from(*number).map_err(|_| invalid())?,
            options: RenderOptions {
                max_side: u16::try_from(*side).map_err(|_| invalid())?,
                max_pixels: *pixels,
                max_output_bytes: usize::try_from(*output).map_err(|_| invalid())?,
                grayscale: *gray == 1,
            },
            limits: Limits::default()
                .max_input(usize::try_from(*input).map_err(|_| invalid())?)
                .max_inflated(*inflated)
                .max_depth(usize::try_from(*depth).map_err(|_| invalid())?)
                .timeout(None),
            memory: *memory,
            cpu_seconds: *cpu,
        })
    }
}

pub(super) fn write_reply(mut out: impl Write, result: Result<Vec<u8>>) -> Result<()> {
    let mut header = *HEADER;
    match result {
        Ok(data) => {
            out.write_all(&header)?;
            out.write_all(&data)?;
        }
        Err(error) => {
            header[5] = match &error {
                Error::PasswordRequired => 1,
                Error::UnsupportedDocument(_) => 2,
                Error::LimitExceeded(_) | Error::Cancelled => 3,
                Error::InvalidPdfOption(_) => 4,
                _ => 5,
            };
            let message = error.to_string();
            let mut end = message.len().min(MAX_ERROR_BYTES);
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            out.write_all(&header)?;
            out.write_all(&message.as_bytes()[..end])?;
        }
    }
    out.flush()?;
    Ok(())
}

pub(super) fn decode(mut data: Vec<u8>, maximum: usize) -> Result<Vec<u8>> {
    if data.len() < HEADER.len() || data[..5] != HEADER[..5] {
        return Err(Error::malformed("render worker", "missing or incompatible response"));
    }
    let status = data[5];
    data.drain(..HEADER.len());
    if status == 0 {
        return if data.len() > maximum {
            Err(Error::LimitExceeded("rendered bytes"))
        } else {
            Ok(data)
        };
    }
    let message = String::from_utf8_lossy(&data[..data.len().min(MAX_ERROR_BYTES)]).into_owned();
    Err(match status {
        1 => Error::PasswordRequired,
        2 => Error::UnsupportedDocument(message),
        3 => Error::LimitExceeded("render worker limit"),
        4 => Error::InvalidPdfOption("the render worker rejected the rendering options"),
        _ => Error::malformed("render worker", message),
    })
}
