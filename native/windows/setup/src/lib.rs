//! Verified Windows release contents and recoverable installation ownership.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod journal;
#[cfg(windows)]
pub mod native;
#[cfg(test)]
#[path = "../../build-support/src/artifact/fixture.rs"]
mod package_fixture;

/// Content-free operation failure.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
