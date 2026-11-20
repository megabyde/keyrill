//! Windows TSF adapter with Rust-owned native authority.
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(windows, allow(non_snake_case))]

#[cfg(windows)]
mod application_identity;
#[cfg(windows)]
mod com;
#[cfg(windows)]
mod configure;
#[cfg(windows)]
mod diagnostics;
#[cfg(windows)]
mod engine;
#[cfg(windows)]
mod host_metadata;
#[cfg(windows)]
mod observation;
#[cfg(windows)]
mod build_identity {
    include!(concat!(env!("OUT_DIR"), "/build_identity.rs"));
}
#[cfg(windows)]
mod admission_watch;
mod edit;
#[cfg(windows)]
mod field;
#[cfg(windows)]
mod field_contract;
#[cfg(windows)]
mod foreign;
#[cfg(windows)]
mod keys;
#[cfg(windows)]
pub mod profiles;
#[cfg(windows)]
pub mod registrar;
#[cfg(windows)]
mod runtime_window;
#[cfg(windows)]
mod service;
#[cfg(windows)]
pub mod settings;
#[cfg(windows)]
pub mod settings_ui;
#[cfg(all(windows, feature = "test-api"))]
mod test_api;
#[cfg(windows)]
mod vocabulary;
#[cfg(windows)]
mod vocabulary_contract;

#[cfg(windows)]
pub use com::{DllCanUnloadNow, DllGetClassObject};
