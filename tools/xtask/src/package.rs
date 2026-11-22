use crate::cargo::{self, Result};
use keyrill_windows_build::artifact::{BINARIES, Package, icon, reject_developer_paths};
use keyrill_windows_build::{
    BuildConfig, Digest, Instrumentation, Target, identity, is_source_input, pe, source_inputs,
};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn output(root: &Path, program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err("package provenance command failed".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
fn config(target: Target) -> BuildConfig<'static> {
    BuildConfig {
        target: target.triple(),
        profile: "release",
        instrumentation: Instrumentation::None,
    }
}
fn build_directory(root: &Path, target: Target) -> PathBuf {
    root.join(format!("target/windows-package-{}", target.name()))
}

pub(crate) fn build(root: &Path, target: Target) -> Result<()> {
    let libdir = output(
        root,
        "rustc",
        &["--print", "target-libdir", "--target", target.triple()],
    )?;
    if !Path::new(&libdir).is_dir() {
        return Err("selected Rust target standard library missing; target installation requires maintainer approval".into());
    }
    let before = identity(root, config(target))?;
    let compiler = output(root, "rustc", &["--version", "--verbose"])?;
    let directory = build_directory(root, target);
    fs::create_dir_all(&directory)?;
    let cargo_directory = root.join(format!("target/windows-package-cargo-{}", target.name()));
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".cargo")))
        .ok_or("Cargo home unavailable for release path remapping")?;
    let sysroot = output(root, "rustc", &["--print", "sysroot"])?;
    let flags = [
        format!("--remap-path-prefix={}=/keyrill", root.display()),
        format!("--remap-path-prefix={}=/cargo", cargo_home.display()),
        format!("--remap-path-prefix={sysroot}=/rust"),
        "-Clink-arg=/Brepro".into(),
        "-Clink-arg=/PDBALTPATH:%_PDB%".into(),
    ]
    .join("\x1f");
    let artifacts = cargo::artifacts(
        root,
        &[
            "build",
            "--release",
            "--target",
            target.triple(),
            "-p",
            "keyrill-windows",
            "-p",
            "keyrill-windows-setup",
            "--target-dir",
            cargo_directory
                .to_str()
                .ok_or("non-Unicode build directory")?,
        ],
        &[
            ("KEYRILL_INSTRUMENTATION", "none"),
            ("CARGO_ENCODED_RUSTFLAGS", &flags),
        ],
    )?;
    let mut hashes = serde_json::Map::new();
    for name in BINARIES {
        let source = if name.ends_with(".dll") {
            cargo::dll(&artifacts)?
        } else {
            cargo::executable(&artifacts, name.trim_end_matches(".exe"), false)?
        };
        let bytes = fs::read(&source)?;
        pe::validate(&bytes, target)?;
        reject_developer_paths(&bytes)?;
        if name.ends_with(".dll") {
            pe::production(&bytes, target)?;
        }
        fs::write(directory.join(name), &bytes)?;
        hashes.insert(name.into(), json!(Digest::of_bytes(&bytes).to_string()));
    }
    if identity(root, config(target))? != before {
        return Err("source changed during package build".into());
    }
    let manifest = json!({"schema":1,"target":target.triple(),"source":before.source.to_string(),"production":before.production.to_string(),"compiler":compiler,"instrumentation":"none","profile":"release","features":[],"pathRemapping":true,"reproducibleLink":true,"files":hashes});
    fs::write(
        directory.join("artifacts.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!("Production release {} built and hashed", target.name());
    Ok(())
}

fn put(
    directory: &Path,
    name: &str,
    bytes: &[u8],
    files: &mut serde_json::Map<String, Value>,
) -> Result<()> {
    let path = directory.join(name);
    fs::create_dir_all(path.parent().ok_or("release parent missing")?)?;
    fs::write(path, bytes)?;
    files.insert(name.into(), json!(Digest::of_bytes(bytes).to_string()));
    Ok(())
}
fn licenses(
    root: &Path,
    directory: &Path,
    files: &mut serde_json::Map<String, Value>,
) -> Result<()> {
    let metadata: Value = serde_json::from_str(&output(
        root,
        "cargo",
        &[
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--filter-platform",
            "x86_64-pc-windows-msvc",
        ],
    )?)?;
    let mut dependencies = Vec::new();
    for package in metadata["packages"]
        .as_array()
        .ok_or("Cargo dependency metadata absent")?
    {
        if package["source"].is_null() {
            continue;
        }
        let name = package["name"].as_str().ok_or("dependency name absent")?;
        let version = package["version"]
            .as_str()
            .ok_or("dependency version absent")?;
        let license = package["license"]
            .as_str()
            .ok_or("dependency license declaration absent")?;
        let base = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or("dependency manifest absent")?,
        )
        .parent()
        .ok_or("dependency directory absent")?;
        let mut texts = Vec::new();
        let mut entries = fs::read_dir(base)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file = entry.file_name().to_string_lossy().into_owned();
            if !keyrill_windows_build::artifact::LICENSE_NAMES.contains(&file.as_str()) {
                continue;
            }
            let path = entry.path();
            if path.is_file() {
                let relative = format!("licenses/{name}-{version}/{file}");
                put(directory, &relative, &fs::read(path)?, files)?;
                texts.push(relative);
            }
        }
        if texts.is_empty() {
            return Err(
                format!("dependency license text unavailable for {name}; package refused").into(),
            );
        }
        dependencies.push(json!({"name":name,"version":version,"license":license,"source":package["source"],"files":texts}));
    }
    dependencies.sort_by_key(|value| format!("{}-{}", value["name"], value["version"]));
    put(
        directory,
        "licenses/dependencies.json",
        &serde_json::to_vec_pretty(&dependencies)?,
        files,
    )
}

fn require_committed_source(root: &Path) -> Result<()> {
    let status = Command::new("git")
        .current_dir(root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .stdin(Stdio::null())
        .output()?;
    if !status.status.success() {
        return Err("package provenance command failed".into());
    }
    let text = std::str::from_utf8(&status.stdout)?;
    let mut records = text.split_terminator('\0');
    while let Some(record) = records.next() {
        let path = record.get(3..).ok_or("invalid source status record")?;
        if is_source_input(Path::new(path)) {
            return Err("candidate requires committed release source inputs".into());
        }
        if record.as_bytes()[..2]
            .iter()
            .any(|status| matches!(status, b'R' | b'C'))
        {
            let original = records.next().ok_or("missing source rename origin")?;
            if is_source_input(Path::new(original)) {
                return Err("candidate requires committed release source inputs".into());
            }
        }
    }
    let committed = Command::new("git")
        .current_dir(root)
        .args(["ls-tree", "-r", "--name-only", "-z", "HEAD"])
        .stdin(Stdio::null())
        .output()?;
    if !committed.status.success() {
        return Err("package provenance command failed".into());
    }
    let committed = std::str::from_utf8(&committed.stdout)?;
    let committed: std::collections::BTreeSet<_> = committed.split_terminator('\0').collect();
    for path in source_inputs(root)? {
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("non-Unicode source input")?
            .replace('\\', "/");
        if !committed.contains(relative.as_str()) {
            return Err("candidate source input is absent from HEAD".into());
        }
    }
    Ok(())
}

pub(crate) fn stage(root: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        return Err(
            "candidate destination must be new; use package-verify for an existing candidate"
                .into(),
        );
    }
    let before = identity(root, config(Target::X64))?;
    require_committed_source(root)?;
    let commit = output(root, "git", &["rev-parse", "HEAD"])?;
    let compiler = output(root, "rustc", &["--version", "--verbose"])?;
    let mut inputs = Vec::new();
    let mut architectures = serde_json::Map::new();
    for target in [Target::X64, Target::X86] {
        let directory = build_directory(root, target);
        let manifest:Value=serde_json::from_slice(&fs::read(directory.join("artifacts.json")).map_err(|_|"both actual production architecture builds are required; run package-build --arch all")?)?;
        let expected = identity(root, config(target))?;
        if manifest["schema"] != 1
            || manifest["target"] != target.triple()
            || manifest["source"] != before.source.to_string()
            || manifest["production"] != expected.production.to_string()
            || manifest["compiler"] != compiler
            || manifest["instrumentation"] != "none"
            || manifest["profile"] != "release"
            || manifest["features"] != json!([])
            || manifest["pathRemapping"] != true
            || manifest["reproducibleLink"] != true
        {
            return Err("stale or incompatible production artifact provenance".into());
        }
        architectures.insert(
            target.name().into(),
            json!({"target":target.triple(),"production":expected.production.to_string()}),
        );
        for name in BINARIES {
            let bytes = fs::read(directory.join(name))?;
            if manifest["files"][name] != Digest::of_bytes(&bytes).to_string() {
                return Err("production artifact hash changed".into());
            }
            inputs.push((format!("{}/{name}", target.name()), bytes));
        }
    }
    fs::create_dir_all(destination)?;
    let mut files = serde_json::Map::new();
    for (name, bytes) in inputs {
        put(destination, &name, &bytes, &mut files)?;
    }
    for (source, name) in [
        ("LICENSE", "LICENSE"),
        ("assets/model-notice.txt", "model-notice.txt"),
        (
            "assets/wordfreq-code-license.txt",
            "wordfreq-code-license.txt",
        ),
        ("assets/model-provenance.json", "model-provenance.json"),
    ] {
        put(destination, name, &fs::read(root.join(source))?, &mut files)?;
    }
    for (name, russian) in [
        ("resources/english.ico", false),
        ("resources/russian.ico", true),
    ] {
        put(destination, name, &icon(russian), &mut files)?;
    }
    licenses(root, destination, &mut files)?;
    let manifest = json!({"schema":1,"unsigned":true,"version":env!("CARGO_PKG_VERSION"),"commit":commit,"source":before.source.to_string(),"compiler":compiler,"model":Digest::of_bytes(&fs::read(root.join("assets/model.bin"))?).to_string(),"dependencies":Digest::of_bytes(&fs::read(root.join("Cargo.lock"))?).to_string(),"architectures":architectures,"files":files});
    if identity(root, config(Target::X64))? != before
        || output(root, "git", &["rev-parse", "HEAD"])? != commit
    {
        return Err("source changed during candidate staging; candidate refused".into());
    }
    fs::write(
        destination.join("package.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    let package = Package::open(destination)?;
    println!("Verified unsigned candidate {}", package.digest());
    Ok(())
}

pub(crate) fn verify(directory: &Path) -> Result<()> {
    let package = Package::open(directory)?;
    println!("Verified unsigned candidate {}", package.digest());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Repository(PathBuf);
    impl Repository {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "keyrill-package-source-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            output(&path, "git", &["init", "--quiet"]).unwrap();
            for file in [
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
                "native/windows/setup/src/lib.rs",
                "docs/notes.md",
            ] {
                let path = path.join(file);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, b"committed source").unwrap();
            }
            output(&path, "git", &["add", "."]).unwrap();
            output(
                &path,
                "git",
                &[
                    "-c",
                    "user.name=Package Test",
                    "-c",
                    "user.email=package-test@example.invalid",
                    "commit",
                    "--quiet",
                    "-m",
                    "test fixture",
                ],
            )
            .unwrap();
            Self(path)
        }
    }
    impl Drop for Repository {
        fn drop(&mut self) {
            assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn unrelated_audit_and_document_changes_allow_candidate_provenance() {
        let repository = Repository::new();
        fs::create_dir(repository.0.join(".audit")).unwrap();
        fs::write(repository.0.join(".audit/run.json"), b"local evidence").unwrap();
        fs::write(repository.0.join("notes.md"), b"unrelated notes").unwrap();
        fs::write(repository.0.join("docs/notes.md"), b"updated notes").unwrap();
        fs::create_dir(repository.0.join("native/windows/.audit")).unwrap();
        fs::write(
            repository.0.join("native/windows/.audit/run.json"),
            b"local evidence",
        )
        .unwrap();
        require_committed_source(&repository.0).unwrap();
    }

    #[test]
    fn source_status_uses_exact_unquoted_names_and_both_sides_of_renames() {
        let repository = Repository::new();
        let file = "native/windows/setup/src/space и apostrophe'.rs";
        fs::write(repository.0.join(file), b"source").unwrap();
        assert!(require_committed_source(&repository.0).is_err());
        fs::remove_file(repository.0.join(file)).unwrap();
        output(
            &repository.0,
            "git",
            &["mv", "native/windows/setup/src/lib.rs", "notes.md"],
        )
        .unwrap();
        assert!(require_committed_source(&repository.0).is_err());
    }

    #[test]
    fn ignored_release_source_is_refused_while_ignored_unrelated_evidence_is_allowed() {
        let repository = Repository::new();
        fs::write(
            repository.0.join(".git/info/exclude"),
            b"hidden.rs\n.audit/\n",
        )
        .unwrap();
        fs::create_dir(repository.0.join(".audit")).unwrap();
        fs::write(repository.0.join(".audit/run.json"), b"ignored evidence").unwrap();
        require_committed_source(&repository.0).unwrap();
        let hidden = repository.0.join("native/windows/setup/src/hidden.rs");
        fs::write(&hidden, b"ignored compilable input").unwrap();
        assert!(
            output(
                &repository.0,
                "git",
                &["status", "--porcelain", "--untracked-files=all"]
            )
            .unwrap()
            .is_empty()
        );
        assert!(source_inputs(&repository.0).unwrap().contains(&hidden));
        assert_eq!(
            require_committed_source(&repository.0)
                .unwrap_err()
                .to_string(),
            "candidate source input is absent from HEAD"
        );
    }

    #[test]
    fn release_inputs_must_be_committed_even_when_deleted_or_untracked() {
        for file in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "native/windows/setup/src/lib.rs",
        ] {
            let repository = Repository::new();
            fs::write(repository.0.join(file), b"changed").unwrap();
            assert!(
                require_committed_source(&repository.0).is_err(),
                "changed {file}"
            );
            fs::remove_file(repository.0.join(file)).unwrap();
            assert!(
                require_committed_source(&repository.0).is_err(),
                "deleted {file}"
            );
        }
        let repository = Repository::new();
        fs::write(
            repository.0.join("native/windows/setup/src/untracked.rs"),
            b"source",
        )
        .unwrap();
        assert!(require_committed_source(&repository.0).is_err());
    }
}
