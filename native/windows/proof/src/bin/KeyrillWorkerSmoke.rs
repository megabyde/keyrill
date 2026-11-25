//! Isolated worker and real module-lifetime schedules.
#[cfg(windows)]
#[path = "../../tests/support/mod.rs"]
mod support;
#[cfg(windows)]
#[path = "../../tests/worker_support.rs"]
mod worker_support;
#[cfg(windows)]
fn main() {
    worker_support::worker_child_main();
}
#[cfg(not(windows))]
fn main() {
    std::process::exit(1);
}
