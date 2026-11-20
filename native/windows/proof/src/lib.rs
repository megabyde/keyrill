//! Independent Rust Windows proof editor, inspectors, and test providers.
#![deny(unsafe_op_in_unsafe_fn)]

mod build_identity {
    include!(concat!(env!("OUT_DIR"), "/build_identity.rs"));
}

/// Return the expected production DLL identity for these independent tools.
#[must_use]
pub fn observer_build() -> [u32; 8] {
    build_identity::BUILD_DIGEST
}

pub mod field;
pub mod options;
pub mod protocol;
pub mod public_fixture;

#[cfg(windows)]
pub mod diagnostics;
#[cfg(windows)]
pub mod editor;
#[cfg(windows)]
pub mod host_inspect;
#[cfg(windows)]
pub mod mapping;
#[cfg(windows)]
pub mod native;
#[cfg(windows)]
pub mod proof_inspect;
#[cfg(windows)]
pub mod routing;
#[cfg(windows)]
pub mod shutdown;
#[cfg(windows)]
pub mod source;
#[cfg(windows)]
pub mod terminal_probe;
#[cfg(windows)]
pub mod uia;

#[cfg(all(windows, test))]
mod uia_test_process;
