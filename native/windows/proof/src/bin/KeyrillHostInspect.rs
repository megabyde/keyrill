//! Inspect one explicitly selected process through metadata and public test cases.

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    keyrill_windows_proof::host_inspect::inspect_host(
        &arguments,
        keyrill_windows_proof::observer_build(),
    )
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("The host inspector requires Windows.");
    std::process::ExitCode::FAILURE
}
