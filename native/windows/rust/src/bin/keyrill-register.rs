//! Maintainer registration command for the Windows TSF profiles.

#[cfg(windows)]
fn main() {
    std::process::exit(keyrill_windows::registrar::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("KeyrillRegister requires Windows.");
    std::process::exit(1);
}
