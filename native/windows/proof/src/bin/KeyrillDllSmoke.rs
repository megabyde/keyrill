//! Child-process check of the explicitly selected production DLL.

#[cfg(windows)]
#[path = "support/smoke.rs"]
mod smoke;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let result = match arguments.as_slice() {
        [command, path] if command == "factory" => smoke::factory(std::path::Path::new(path)),
        [command] if command == "activate" => smoke::activation(false),
        [command] if command == "missing" => smoke::activation(true),
        _ => Err("expected factory DLL_PATH, activate, or missing".into()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
