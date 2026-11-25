//! Give independent observers the production identity of the same source snapshot.

use keyrill_windows_build::{
    BuildConfig, Instrumentation, Target, identity, source_inputs, write_rust_identity,
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
    let digest = identity(root, config)
        .expect("observer build identity")
        .production;
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    write_rust_identity(&output.join("build_identity.rs"), digest)
        .expect("write observer identity");
    if target.ends_with("-windows-msvc") {
        let target: Target = target.parse().expect("supported Windows client target");
        let template =
            std::fs::read_to_string(root.join("native/windows/proof_activation.manifest"))
                .expect("activation manifest template");
        let activation = output.join("proof_activation.manifest");
        std::fs::write(
            &activation,
            template.replace("@KEYRILL_ARCHITECTURE@", target.manifest_architecture()),
        )
        .expect("target activation manifest");
        for binary in ["KeyrillProofEditor", "KeyrillDllSmoke"] {
            println!("cargo:rustc-link-arg-bin={binary}=/MANIFEST:EMBED");
            println!(
                "cargo:rustc-link-arg-bin={binary}=/MANIFESTINPUT:{}",
                activation.display()
            );
        }
    }
}
