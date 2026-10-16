use std::process::ExitCode;
use std::time::Duration;

use imprimatur::read::Limits;
use usage_rs::Args;

/// The bounds a read runs under, shared by every command that reads.
#[derive(Args, Debug)]
pub struct LimitFlags {
    /// Seconds one document may take
    #[usage(long, default = "60", value_name = "SECONDS", help_heading = "Limits")]
    pub timeout: f64,

    /// Largest document accepted, in bytes (512 MiB by default)
    #[usage(long, value_name = "BYTES", help_heading = "Limits")]
    pub max_bytes: Option<usize>,

    /// Decompressed data allowed per document, in bytes (1 GiB by default)
    #[usage(long, value_name = "BYTES", help_heading = "Limits")]
    pub max_inflated: Option<u64>,
}

impl LimitFlags {
    /// The limits, or the exit status of a wrong command line, reported.
    pub fn checked(&self) -> Result<Limits, ExitCode> {
        self.limits().map_err(|message| {
            eprintln!("imprimatur: {message}");
            ExitCode::from(2)
        })
    }

    /// The limits, or what is wrong with the flags.
    pub fn limits(&self) -> Result<Limits, String> {
        let timeout = Duration::try_from_secs_f64(self.timeout)
            .ok()
            .filter(|timeout| !timeout.is_zero())
            .ok_or_else(|| {
                format!("--timeout must be a positive number of seconds, not {}", self.timeout)
            })?;
        let mut limits = Limits::default().timeout(Some(timeout));
        if let Some(bytes) = self.max_bytes {
            limits = limits.max_input(bytes);
        }
        if let Some(bytes) = self.max_inflated {
            limits = limits.max_inflated(bytes);
        }
        Ok(limits)
    }
}
