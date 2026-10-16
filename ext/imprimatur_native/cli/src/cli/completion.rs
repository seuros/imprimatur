use std::process::ExitCode;

use usage_rs::complete::Shell;
use usage_rs::{Args, Run};

/// Print a shell completion script
#[derive(Args, Debug)]
pub struct Completion {
    /// Shell to generate for
    #[usage(arg, choices("bash", "zsh", "fish", "elvish", "nu", "powershell"))]
    pub shell: String,
}

impl Run for Completion {
    type Output = ExitCode;

    fn run(self) -> ExitCode {
        // The choices above are the names Shell::from_name knows.
        let Some(shell) = Shell::from_name(&self.shell) else {
            eprintln!("imprimatur: unsupported shell: {}", self.shell);
            return ExitCode::from(2);
        };
        print!("{}", super::Imprimatur::completion_script(shell));
        ExitCode::SUCCESS
    }
}
