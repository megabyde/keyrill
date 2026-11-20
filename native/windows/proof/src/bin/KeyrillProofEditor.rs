//! Registration-free RichEdit fixture with explicit lifecycle proof modes.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let Some(mode) = keyrill_windows_proof::options::FixtureMode::parse(&arguments) else {
        return std::process::ExitCode::FAILURE;
    };
    match keyrill_windows_proof::editor::run_editor(mode) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Proof editor failed with {}.", error.code());
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("The proof editor requires Windows.");
    std::process::ExitCode::FAILURE
}
