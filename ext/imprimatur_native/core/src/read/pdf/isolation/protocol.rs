//! Versioned replies and numeric-only arguments for the trusted PDF worker.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use super::ProcessLimits;
use crate::read::Limits;
use crate::read::pdf::{ReadOptions, RenderOptions};
use crate::{Error, Result};

pub(super) const HEADER: &[u8; 6] = b"IMPR\x02\0";
pub(super) const MAX_ERROR_BYTES: usize = 4096;

pub(super) enum Operation {
    Info,
    Pages(ReadOptions),
    Render(u32, RenderOptions),
}

impl Operation {
    pub fn maximum(&self) -> Result<usize> {
        let invalid = || Error::InvalidPdfOption("worker output limit is too large");
        let maximum = match self {
            Self::Info => MAX_ERROR_BYTES,
            // JSON can escape each text byte as six ASCII bytes. Reasons and
            // record metadata are separately bounded for every selected page.
            Self::Pages(options) => options
                .max_output_bytes
                .checked_mul(6)
                .and_then(|text| {
                    options.max_pages.checked_mul(MAX_ERROR_BYTES * 6 + 512)?.checked_add(text)
                })
                .and_then(|bytes| bytes.checked_add(MAX_ERROR_BYTES * 6 + 512))
                .ok_or_else(invalid)?,
            Self::Render(_, options) => options.max_output_bytes,
        };
        maximum.max(MAX_ERROR_BYTES).checked_add(HEADER.len()).ok_or_else(invalid)
    }
}

pub(super) struct Request {
    pub restricted: bool,
    pub operation: Operation,
    pub limits: Limits,
    pub memory: u64,
    pub cpu_seconds: u64,
}

impl Request {
    pub fn new(operation: Operation, limits: &Limits, process: &ProcessLimits) -> Result<Self> {
        operation.maximum()?;
        match &operation {
            Operation::Pages(options) => options.validate()?,
            Operation::Render(0, _) => {
                return Err(Error::InvalidPdfOption("page number must be positive"));
            }
            _ => {}
        }
        if process.memory_bytes == 0 || process.memory_bytes >= i64::MAX as u64 {
            return Err(Error::InvalidPdfOption("memory limit must be positive and finite"));
        }
        let wall = process.wall_time_for(limits);
        let cpu_seconds = wall
            .as_secs()
            .checked_add(u64::from(wall.subsec_nanos() != 0))
            .filter(|&seconds| seconds != 0 && seconds < i64::MAX as u64)
            .ok_or(Error::InvalidPdfOption("worker wall time must be positive and finite"))?;
        u64::try_from(wall.as_nanos())
            .map_err(|_| Error::InvalidPdfOption("worker wall time is too large"))?;
        Ok(Self {
            restricted: false,
            operation,
            limits: limits.clone().timeout(Some(wall)),
            memory: process.memory_bytes,
            cpu_seconds,
        })
    }

    pub fn arguments(&self) -> Vec<OsString> {
        let timeout = self.limits.timeout.unwrap_or_default();
        let mut values = vec![
            u64::from(self.restricted),
            self.limits.max_input as u64,
            self.limits.max_inflated,
            self.limits.max_depth as u64,
            self.memory,
            self.cpu_seconds,
            u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX),
        ];
        match &self.operation {
            Operation::Info => values.push(0),
            Operation::Pages(options) => values.extend([
                1,
                u64::from(options.first_page),
                u64::from(options.last_page),
                options.max_pages as u64,
                options.max_page_bytes as u64,
                options.max_output_bytes as u64,
            ]),
            Operation::Render(number, options) => values.extend([
                2,
                u64::from(*number),
                u64::from(options.max_side),
                options.max_pixels,
                options.max_output_bytes as u64,
                u64::from(options.grayscale),
            ]),
        }
        values.into_iter().map(|number| OsString::from(number.to_string())).collect()
    }

    pub fn parse() -> Result<Self> {
        let invalid = || Error::InvalidPdfOption("invalid PDF worker arguments");
        let values: Vec<u64> = std::env::args()
            .skip(1)
            .map(|value| value.parse().map_err(|_| invalid()))
            .collect::<Result<_>>()?;
        let [restricted, input, inflated, depth, memory, cpu, timeout, rest @ ..] =
            values.as_slice()
        else {
            return Err(invalid());
        };
        if *restricted > 1 {
            return Err(invalid());
        }
        let length = |value| usize::try_from(value).map_err(|_| invalid());
        let operation = match rest {
            [0] => Operation::Info,
            [1, first, last, count, page, output] => Operation::Pages(ReadOptions {
                first_page: u32::try_from(*first).map_err(|_| invalid())?,
                last_page: u32::try_from(*last).map_err(|_| invalid())?,
                max_pages: length(*count)?,
                max_page_bytes: length(*page)?,
                max_output_bytes: length(*output)?,
            }),
            [2, number, side, pixels, output, gray] if *gray <= 1 => Operation::Render(
                u32::try_from(*number).map_err(|_| invalid())?,
                RenderOptions {
                    max_side: u16::try_from(*side).map_err(|_| invalid())?,
                    max_pixels: *pixels,
                    max_output_bytes: length(*output)?,
                    grayscale: *gray == 1,
                },
            ),
            _ => return Err(invalid()),
        };
        let limits = Limits::default()
            .max_input(length(*input)?)
            .max_inflated(*inflated)
            .max_depth(length(*depth)?)
            .timeout(Some(Duration::from_nanos(*timeout)));
        let process =
            ProcessLimits { memory_bytes: *memory, wall_time: Duration::from_nanos(*timeout) };
        let mut request = Self::new(operation, &limits, &process)?;
        request.restricted = *restricted == 1;
        if request.cpu_seconds != *cpu {
            return Err(invalid());
        }
        Ok(request)
    }
}

pub(super) fn failure(error: &Error) -> (u8, String) {
    let kind = match error {
        Error::PasswordRequired => 1,
        Error::UnsupportedDocument(_) => 2,
        Error::LimitExceeded("time") => 6,
        Error::LimitExceeded(_) | Error::Cancelled => 3,
        Error::InvalidPdfOption(_) => 4,
        _ => 5,
    };
    let mut message = error.to_string();
    let mut end = message.len().min(MAX_ERROR_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    (kind, message)
}

pub(super) fn error(kind: u8, message: String) -> Error {
    match kind {
        1 => Error::PasswordRequired,
        2 => Error::UnsupportedDocument(message),
        3 => Error::LimitExceeded("PDF worker limit"),
        4 => Error::InvalidPdfOption("the PDF worker rejected the options"),
        6 => Error::LimitExceeded("time"),
        _ => Error::malformed("PDF worker", message),
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
            let (kind, message) = failure(&error);
            header[5] = kind;
            out.write_all(&header)?;
            out.write_all(message.as_bytes())?;
        }
    }
    out.flush()?;
    Ok(())
}

pub(super) fn decode(mut data: Vec<u8>, maximum: usize) -> Result<Vec<u8>> {
    if data.len() < HEADER.len() || data[..5] != HEADER[..5] {
        return Err(Error::malformed("PDF worker", "missing or incompatible response"));
    }
    let status = data[5];
    data.drain(..HEADER.len());
    if status == 0 {
        return if data.len() > maximum {
            Err(Error::LimitExceeded("PDF worker output"))
        } else {
            Ok(data)
        };
    }
    let message = String::from_utf8_lossy(&data[..data.len().min(MAX_ERROR_BYTES)]).into_owned();
    Err(error(status, message))
}
