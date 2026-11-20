//! Emit the shared Rust artifact identity and COM export directives.

use keyrill_windows_build::{
    BuildConfig, Instrumentation, Target, Variant, identity, source_inputs, write_rust_identity,
};
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest"));
    let root = manifest.ancestors().nth(3).expect("workspace root");
    for directory in ["include", "crates", "native/windows", "tools/xtask"] {
        println!("cargo:rerun-if-changed={}", root.join(directory).display());
    }
    for path in source_inputs(root).expect("build identity source enumeration") {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rerun-if-env-changed=KEYRILL_INSTRUMENTATION");
    let instrumentation = match std::env::var("KEYRILL_INSTRUMENTATION").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("none") => Instrumentation::None,
        Ok("address") => Instrumentation::Address,
        _ => panic!("unknown build instrumentation"),
    };
    let target = std::env::var("TARGET").expect("Cargo target");
    let profile = std::env::var("PROFILE").expect("Cargo profile");
    let config = BuildConfig {
        target: &target,
        profile: &profile,
        instrumentation,
    };
    let variant = if std::env::var_os("CARGO_FEATURE_TEST_API").is_some() {
        Variant::TestApi
    } else {
        Variant::Production
    };
    let digest = identity(root, config)
        .expect("build identity")
        .variant(variant);
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    write_rust_identity(&output.join("build_identity.rs"), digest).expect("write build identity");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let target: Target = target.parse().expect("supported Windows client target");
        assert!(
            instrumentation != Instrumentation::Address || target == Target::X64,
            "Windows AddressSanitizer supports x64 only"
        );
        println!("cargo:rustc-cdylib-link-arg=/EXPORT:DllGetClassObject,PRIVATE");
        println!("cargo:rustc-cdylib-link-arg=/EXPORT:DllCanUnloadNow,PRIVATE");
    }
}
