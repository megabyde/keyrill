//! Explicit maintainer installer entry point. No elevation or source selection.
fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    let result = keyrill_windows_setup::native::run();
    #[cfg(not(windows))]
    let result: keyrill_windows_setup::Result<()> = Err("KeyrillSetup requires Windows".into());
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
