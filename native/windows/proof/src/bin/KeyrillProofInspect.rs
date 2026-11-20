//! Inspect explicitly named public proof-editor cases through UI Automation.

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    keyrill_windows_proof::proof_inspect::inspect_proof(&arguments)
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("The proof inspector requires Windows.");
    std::process::ExitCode::FAILURE
}
