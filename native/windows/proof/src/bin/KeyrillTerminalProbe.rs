//! Public-input child for independent Terminal delivery observations.

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    keyrill_windows_proof::terminal_probe::terminal_probe(&arguments)
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("The public Terminal probe requires Windows.");
    std::process::ExitCode::FAILURE
}
