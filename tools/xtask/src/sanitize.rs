use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn runtime_directory() -> io::Result<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles(x86)")
        .ok_or_else(|| io::Error::other("Visual Studio discovery directory missing"))?;
    let vswhere = Path::new(&program_files).join("Microsoft Visual Studio/Installer/vswhere.exe");
    let output = Command::new(vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("Visual Studio discovery failed"));
    }
    let installation = String::from_utf8(output.stdout)
        .map_err(|_| io::Error::other("Visual Studio discovery returned invalid text"))?;
    let versions = Path::new(installation.trim()).join("VC/Tools/MSVC");
    let mut candidates = Vec::new();
    for entry in fs::read_dir(versions)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name();
            if let Some(version) = name.to_str().and_then(version) {
                candidates.push((version, entry.path()));
            }
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    let (_, toolchain) = candidates
        .pop()
        .ok_or_else(|| io::Error::other("MSVC runtime toolchain missing"))?;
    let directory = toolchain.join("bin/Hostx64/x64");
    if !directory.join("clang_rt.asan_dynamic-x86_64.dll").is_file() {
        return Err(io::Error::other(
            "installed x64 AddressSanitizer runtime missing",
        ));
    }
    Ok(directory)
}

fn version(name: &str) -> Option<Vec<u32>> {
    let numbers: Vec<_> = name
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    (numbers.len() >= 3).then_some(numbers)
}

pub(crate) fn prepare(root: &Path) -> io::Result<OsString> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err(io::Error::other(
            "Rust Windows AddressSanitizer requires an x64 Windows host",
        ));
    }
    let runtime = runtime_directory()?;
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let path =
        std::env::join_paths(std::iter::once(runtime).chain(std::env::split_paths(&old_path)))
            .map_err(|_| io::Error::other("AddressSanitizer runtime search path invalid"))?;
    let directory = root.join("target/windows-rust-sanitize/canary");
    fs::create_dir_all(&directory)?;
    let source = directory.join("memory_error.rs");
    fs::write(
        &source,
        r#"fn main() {
    let allocation = vec![41_u8; 64];
    let retired = allocation.as_ptr();
    drop(allocation);
    unsafe { std::hint::black_box(std::ptr::read_volatile(retired)); }
}
"#,
    )?;
    let executable = directory.join("memory_error.exe");
    let compile = Command::new("rustc")
        .current_dir(root)
        .args([
            "--edition=2024",
            "-Z",
            "sanitizer=address",
            "-C",
            "debuginfo=1",
        ])
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .env("RUSTC_BOOTSTRAP", "1")
        .env("PATH", &path)
        .stdin(Stdio::null())
        .output()?;
    fs::write(directory.join("compile.stderr.log"), &compile.stderr)?;
    if !compile.status.success() {
        return Err(io::Error::other(
            "Rust AddressSanitizer canary compilation failed",
        ));
    }
    let output = Command::new(executable)
        .env("PATH", &path)
        .stdin(Stdio::null())
        .output()?;
    fs::write(directory.join("canary.stderr.log"), &output.stderr)?;
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    if output.status.success() || !diagnostic.contains("AddressSanitizer: heap-use-after-free") {
        return Err(io::Error::other(
            "Rust AddressSanitizer did not detect the canary memory error",
        ));
    }
    println!("Experimental Rust AddressSanitizer detected the canary heap-use-after-free.");
    println!(
        "This uses child-local RUSTC_BOOTSTRAP with the pinned compiler. Prebuilt standard-library and Windows code remain uninstrumented."
    );
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolchain_versions_sort_numerically_and_reject_nonversions() {
        assert!(version("14.50.12345").unwrap() > version("14.9.12345").unwrap());
        for value in ["current", "14", "14.50", "14.50.x", "14..50"] {
            assert!(version(value).is_none());
        }
    }

    #[test]
    #[ignore = "compiles and executes the installed experimental Rust sanitizer canary"]
    fn installed_rust_sanitizer_detects_a_real_memory_error() {
        let root = std::env::var_os("KEYRILL_SANITIZER_ROOT").expect("explicit workspace required");
        prepare(Path::new(&root)).unwrap();
    }
}
