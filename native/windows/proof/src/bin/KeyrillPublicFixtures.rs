//! Prepare owned local public acceptance documents without launching applications.
use std::process::ExitCode;
fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let [directory] = arguments.as_slice() else {
        eprintln!("Usage: KeyrillPublicFixtures DIRECTORY");
        return ExitCode::from(2);
    };
    match keyrill_windows_proof::public_fixture::generate(std::path::Path::new(directory)) {
        Ok(()) => {
            println!("public-fixtures prepared=3 schema=1");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("public-fixtures unavailable kind={:?}", error.kind());
            ExitCode::FAILURE
        }
    }
}
