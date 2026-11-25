//! On-demand native Windows settings application.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(keyrill_windows::settings_ui::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("KeyrillSettings requires Windows.");
    std::process::exit(1);
}
