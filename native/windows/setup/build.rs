//! Keep setup executables in the caller's explicitly chosen privilege context.
fn main() {
    println!("cargo:rerun-if-changed=setup.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let directory = std::env::var("CARGO_MANIFEST_DIR").expect("Cargo manifest directory");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{directory}/setup.manifest");
    }
}
