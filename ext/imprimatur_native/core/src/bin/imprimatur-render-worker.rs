#[cfg(unix)]
fn main() -> std::process::ExitCode {
    imprimatur::read::pdf::isolation::worker_main()
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("isolated PDF rendering requires a supported Unix platform");
    std::process::ExitCode::FAILURE
}
