use crate::{cargo, pe, sanitize, test_run::TestRun};
use cargo::Result;
use keyrill_windows_build::{BuildConfig, Digest, Identity, Instrumentation, Target, identity};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PUBLIC_EXPORTS: [&str; 2] = ["DllCanUnloadNow", "DllGetClassObject"];
const PRODUCTION_PACKAGES: &[&str] = &["keyrill-windows", "keyrill-windows-setup"];
const PRODUCTION_EXECUTABLES: &[&str] = &["KeyrillRegister", "KeyrillSettings", "KeyrillSetup"];

pub(crate) fn common_checks(root: &Path, target: Target) -> Result<()> {
    require_target(target)?;
    for arguments in [
        vec!["fmt", "--all", "--", "--check"],
        vec!["fmt", "--manifest-path", "fuzz/Cargo.toml", "--", "--check"],
        vec![
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        vec![
            "clippy",
            "--locked",
            "-p",
            "keyrill-windows",
            "--all-targets",
            "--features",
            "test-api",
            "--",
            "-D",
            "warnings",
        ],
        vec![
            "clippy",
            "--locked",
            "-p",
            "keyrill-windows-proof",
            "--all-targets",
            "--features",
            "dll-tests",
            "--",
            "-D",
            "warnings",
        ],
        vec!["test", "--locked", "--workspace"],
        vec!["doc", "--locked", "--workspace", "--no-deps"],
        vec!["build", "--locked", "--workspace", "--release"],
    ] {
        let mut command = Command::new("cargo");
        if arguments.first() != Some(&"fmt") {
            command.args([arguments[0], "--target", target.triple()]);
        } else {
            command.arg(arguments[0]);
        }
        command
            .current_dir(root)
            .args(&arguments[1..])
            .env("RUSTDOCFLAGS", "-D warnings");
        cargo::run(&mut command)?;
    }
    Ok(())
}

pub(crate) struct Build {
    pub(crate) target: Target,
    binary_hashes: BTreeMap<PathBuf, Digest>,
    pub(crate) directory: PathBuf,
    pub(crate) identity: Identity,
    pub(crate) test_executable: PathBuf,
    runtime_path: Option<OsString>,
    compiler_environment: Vec<(&'static str, OsString)>,
}

impl Build {
    fn record_executable(
        &mut self,
        artifacts: &[cargo::Artifact],
        name: &str,
        test: bool,
    ) -> Result<PathBuf> {
        let path = cargo::executable(artifacts, name, test)?;
        let digest = artifact_digest(&path, self.target)?;
        record_artifact(&mut self.binary_hashes, &path, digest)?;
        Ok(path)
    }

    fn command(&self, executable: &Path) -> Result<Command> {
        if !self.binary_hashes.contains_key(executable) {
            return Err("Windows executable has no recorded artifact identity".into());
        }
        for (path, expected) in &self.binary_hashes {
            if *expected != artifact_digest(path, self.target)? {
                return Err(
                    "Windows artifact changed after its build identity was recorded".into(),
                );
            }
        }
        let mut command = Command::new(executable);
        if let Some(path) = &self.runtime_path {
            command.env("PATH", path);
        }
        command
            .env(
                "KEYRILL_TEST_DLL",
                self.directory.join("KeyrillTsfTest.dll"),
            )
            .env(
                "KEYRILL_PRODUCTION_DLL",
                self.directory.join("KeyrillTsf.dll"),
            )
            .env(
                "KEYRILL_WORKER_SMOKE",
                self.directory.join("KeyrillWorkerSmoke.exe"),
            )
            .env(
                "KEYRILL_TEST_BUILD_DIGEST",
                self.identity
                    .test_api
                    .words()
                    .map(|word| format!("{word:08x}"))
                    .join(","),
            );
        command.env("KEYRILL_TARGET_ARCH", self.target.name());
        command.env(
            "KEYRILL_SETTINGS_PREVIEW",
            self.directory.join("KeyrillSettings.exe"),
        );
        Ok(command)
    }
}

fn artifact_digest(path: &Path, target: Target) -> Result<Digest> {
    let image = fs::read(path)?;
    pe::validate(&image, target)?;
    Ok(Digest::of_bytes(&image))
}

fn record_artifact(
    binary_hashes: &mut BTreeMap<PathBuf, Digest>,
    path: &Path,
    digest: Digest,
) -> Result<()> {
    match binary_hashes.entry(path.to_owned()) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(digest);
            Ok(())
        }
        std::collections::btree_map::Entry::Occupied(entry) if *entry.get() == digest => Ok(()),
        _ => Err("Windows artifact changed after its build identity was recorded".into()),
    }
}

fn copy(
    source: &Path,
    directory: &Path,
    name: &str,
    target: Target,
    binary_hashes: &mut BTreeMap<PathBuf, Digest>,
) -> Result<PathBuf> {
    let actual = artifact_digest(source, target)?;
    let expected = binary_hashes.get(source).copied().unwrap_or(actual);
    if actual != expected {
        return Err("Windows artifact changed after its build identity was recorded".into());
    }
    let destination = directory.join(name);
    fs::copy(source, &destination)?;
    if artifact_digest(&destination, target)? != expected {
        return Err("Windows artifact copy differs from its recorded source".into());
    }
    record_artifact(binary_hashes, &destination, expected)?;
    Ok(destination)
}

fn validate_exports(path: &Path, target: Target, private: bool) -> Result<()> {
    let actual = pe::exports(&fs::read(path)?, target)?;
    let required: BTreeSet<_> = PUBLIC_EXPORTS.into_iter().map(str::to_owned).collect();
    if !required.is_subset(&actual) {
        return Err("DLL is missing public COM exports".into());
    }
    let private_count = actual
        .iter()
        .filter(|name| name.starts_with("KeyrillTest"))
        .count();
    if (private && private_count == 0) || (!private && private_count != 0) {
        return Err("DLL test feature export boundary violated".into());
    }
    Ok(())
}

fn require_target(target: Target) -> Result<()> {
    let output = Command::new("rustc")
        .args(["--print", "target-libdir", "--target", target.triple()])
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err("unable to locate the selected Rust target".into());
    }
    let directory = PathBuf::from(std::str::from_utf8(&output.stdout)?.trim());
    if !directory.is_dir() {
        return Err(format!("Rust standard library for {} is unavailable; target installation requires maintainer approval", target.triple()).into());
    }
    Ok(())
}

pub(crate) fn build(
    root: &Path,
    target: Target,
    instrumentation: Instrumentation,
) -> Result<Build> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("Windows artifacts require an x64 Windows host".into());
    }
    if instrumentation == Instrumentation::Address && target != Target::X64 {
        return Err("Windows AddressSanitizer supports x64 only".into());
    }
    require_target(target)?;
    let config = BuildConfig {
        target: target.triple(),
        profile: "release",
        instrumentation,
    };
    let before = identity(root, config)?;
    let (directory_name, mut environment, runtime_path) = match instrumentation {
        Instrumentation::None => (
            "target/windows-rust-check",
            vec![
                ("KEYRILL_INSTRUMENTATION", "none".into()),
                ("RUSTFLAGS", "".into()),
            ],
            None,
        ),
        Instrumentation::Address => (
            "target/windows-rust-sanitize",
            vec![
                ("KEYRILL_INSTRUMENTATION", "address".into()),
                ("RUSTC_BOOTSTRAP", "1".into()),
                ("RUSTFLAGS", "-Zsanitizer=address -Cdebuginfo=1".into()),
            ],
            Some(sanitize::prepare(root)?),
        ),
    };
    let directory = root.join(match target {
        Target::X64 => directory_name.to_owned(),
        Target::X86 => format!("{directory_name}-x86"),
    });
    // Final outputs remain separate so the two DLL variants never share a destination.
    environment.push((
        "CARGO_BUILD_BUILD_DIR",
        directory.join("cargo-build").into_os_string(),
    ));
    fs::create_dir_all(directory.join("tests"))?;
    let mut binary_hashes = BTreeMap::new();
    let target_directory = directory.join("cargo-production");
    let mut arguments = vec![
        "build",
        "--release",
        "--target",
        target.triple(),
        "--target-dir",
        target_directory
            .to_str()
            .ok_or("target path is not Unicode")?,
    ];
    for package in PRODUCTION_PACKAGES {
        arguments.extend(["-p", package]);
    }
    let production = cargo::artifacts(root, &arguments, &environment)?;
    let production_dll = copy(
        &cargo::dll(&production)?,
        &directory,
        "KeyrillTsf.dll",
        target,
        &mut binary_hashes,
    )?;
    for name in PRODUCTION_EXECUTABLES {
        copy(
            &cargo::executable(&production, name, false)?,
            &directory,
            &format!("{name}.exe"),
            target,
            &mut binary_hashes,
        )?;
    }
    validate_exports(&production_dll, target, false)?;

    let target_directory = directory.join("cargo-test-api");
    let test_api = cargo::artifacts(
        root,
        &[
            "build",
            "--release",
            "--target",
            target.triple(),
            "-p",
            "keyrill-windows",
            "--features",
            "test-api",
            "--target-dir",
            target_directory
                .to_str()
                .ok_or("target path is not Unicode")?,
        ],
        &environment,
    )?;
    let test_dll = copy(
        &cargo::dll(&test_api)?,
        &directory,
        "KeyrillTsfTest.dll",
        target,
        &mut binary_hashes,
    )?;
    validate_exports(&test_dll, target, true)?;

    let target_directory = directory.join("cargo-proof");
    let proof = cargo::artifacts(
        root,
        &[
            "build",
            "--release",
            "--target",
            target.triple(),
            "-p",
            "keyrill-windows-proof",
            "--bins",
            "--target-dir",
            target_directory
                .to_str()
                .ok_or("target path is not Unicode")?,
        ],
        &environment,
    )?;
    for name in [
        "KeyrillProofEditor",
        "KeyrillProofInspect",
        "KeyrillHostInspect",
        "KeyrillDllSmoke",
        "KeyrillWorkerSmoke",
        "KeyrillDiagnosticDecoder",
        "KeyrillTerminalProbe",
        "KeyrillPublicFixtures",
    ] {
        copy(
            &cargo::executable(&proof, name, false)?,
            &directory,
            &format!("{name}.exe"),
            target,
            &mut binary_hashes,
        )?;
    }
    let tests = cargo::artifacts(
        root,
        &[
            "test",
            "--no-run",
            "--release",
            "--target",
            target.triple(),
            "-p",
            "keyrill-windows-proof",
            "--test",
            "controlled",
            "--features",
            "dll-tests",
            "--target-dir",
            target_directory
                .to_str()
                .ok_or("target path is not Unicode")?,
        ],
        &environment,
    )?;
    let test_executable = copy(
        &cargo::executable(&tests, "controlled", true)?,
        &directory.join("tests"),
        "KeyrillProofEditor.exe",
        target,
        &mut binary_hashes,
    )?;
    let after = identity(root, config)?;
    if before != after {
        return Err("source inputs changed while Windows artifacts were built".into());
    }
    let mut hashes = serde_json::Map::new();
    for (path, digest) in &binary_hashes {
        if artifact_digest(path, target)? != *digest {
            return Err("Windows artifact changed after its build identity was recorded".into());
        }
        let name = path
            .strip_prefix(&directory)?
            .to_string_lossy()
            .replace('\\', "/");
        hashes.insert(name, json!(digest.to_string()));
    }
    let manifest = json!({"schema": 2, "target": target.triple(), "architecture": target.name(), "pointerBits": target.pointer_bits(), "peMachine": target.pe_machine(), "profile": "release", "instrumentation": match instrumentation { Instrumentation::None => "none", Instrumentation::Address => "address" }, "source": before.source.to_string(), "production": before.production.to_string(), "testApi": before.test_api.to_string(), "files": hashes});
    fs::write(
        directory.join("artifacts.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(Build {
        target,
        binary_hashes,
        directory,
        identity: before,
        test_executable,
        runtime_path,
        compiler_environment: environment,
    })
}

pub(crate) fn controlled(build: &Build) -> Result<BTreeSet<String>> {
    run_tests(build, &build.test_executable, "controlled", None)
}

pub(crate) fn controlled_case(build: &Build, case: &str) -> Result<BTreeSet<String>> {
    execute_tests(
        build,
        &build.test_executable,
        "controlled",
        None,
        Some(case),
    )
}

fn run_tests(
    build: &Build,
    executable: &Path,
    prefix: &str,
    decoder_identity: Option<Digest>,
) -> Result<BTreeSet<String>> {
    execute_tests(build, executable, prefix, decoder_identity, None)
}

fn execute_tests(
    build: &Build,
    executable: &Path,
    prefix: &str,
    decoder_identity: Option<Digest>,
    exact: Option<&str>,
) -> Result<BTreeSet<String>> {
    let mut command = build.command(executable)?;
    let listing = command
        .args(["--list", "--format", "terse"])
        .stdin(Stdio::null())
        .output()?;
    if !listing.status.success() {
        return Err("controlled test enumeration failed".into());
    }
    let tests = TestRun::enumerate(prefix, &listing.stdout, exact)?;
    let count = tests.count();
    println!("Running {count} independently enumerated tests from {prefix}.");
    let mut command = build.command(executable)?;
    command.args([
        "--test-threads=1",
        "--include-ignored",
        "--format=pretty",
        "--color=never",
    ]);
    if let Some(name) = tests.exact() {
        command.args(["--exact", name]);
    }
    if let Some(digest) = decoder_identity {
        command
            .env(
                "KEYRILL_DIAGNOSTIC_DECODER",
                build.directory.join("KeyrillDiagnosticDecoder.exe"),
            )
            .env("KEYRILL_DECODER_BUILD_DIGEST", digest.to_string());
    }
    let output = command.stdin(Stdio::null()).output()?;
    io::stdout().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    if !output.status.success() {
        return Err("selected test execution failed".into());
    }
    tests.confirmed(&output.stdout)
}

pub(crate) fn native_tests(root: &Path, build: &mut Build) -> Result<BTreeSet<String>> {
    let mut executed = BTreeSet::new();
    for (variant, target_directory, features, digest) in [
        (
            "production",
            "cargo-production",
            None,
            build.identity.production,
        ),
        (
            "test-api",
            "cargo-test-api",
            Some("test-api"),
            build.identity.test_api,
        ),
    ] {
        let target = build.directory.join(target_directory);
        let mut arguments = vec![
            "test",
            "--no-run",
            "--release",
            "--target",
            build.target.triple(),
            "-p",
            "keyrill-windows",
            "--lib",
            "--target-dir",
            target.to_str().ok_or("target path is not Unicode")?,
        ];
        if let Some(features) = features {
            arguments.extend(["--features", features]);
        }
        let artifacts = cargo::artifacts(root, &arguments, &build.compiler_environment)?;
        let executable = build.record_executable(&artifacts, "keyrill_windows", true)?;
        executed.extend(run_tests(
            build,
            &executable,
            &format!("runtime::{variant}"),
            Some(digest),
        )?);
    }
    let target = build.directory.join("cargo-setup");
    let artifacts = cargo::artifacts(
        root,
        &[
            "test",
            "--no-run",
            "--release",
            "--target",
            build.target.triple(),
            "-p",
            "keyrill-windows-setup",
            "--lib",
            "--target-dir",
            target.to_str().ok_or("target path is not Unicode")?,
        ],
        &build.compiler_environment,
    )?;
    let executable = build.record_executable(&artifacts, "keyrill_windows_setup", true)?;
    executed.extend(run_tests(build, &executable, "setup", None)?);
    let target = build.directory.join("cargo-proof");
    let artifacts = cargo::artifacts(
        root,
        &[
            "test",
            "--no-run",
            "--release",
            "--target",
            build.target.triple(),
            "-p",
            "keyrill-windows-proof",
            "--lib",
            "--features",
            "dll-tests",
            "--target-dir",
            target.to_str().ok_or("target path is not Unicode")?,
        ],
        &build.compiler_environment,
    )?;
    let directory = build.directory.join("tests/fixtures");
    fs::create_dir_all(&directory)?;
    let executable = copy(
        &cargo::executable(&artifacts, "keyrill_windows_proof", true)?,
        &directory,
        "KeyrillProofEditor.exe",
        build.target,
        &mut build.binary_hashes,
    )?;
    executed.extend(run_tests(build, &executable, "proof", None)?);

    let target = build.directory.join("cargo-production");
    let artifacts = cargo::artifacts(
        root,
        &[
            "test",
            "--no-run",
            "--release",
            "--target",
            build.target.triple(),
            "-p",
            "keyrill-ffi",
            "--test",
            "windows_abi",
            "--target-dir",
            target.to_str().ok_or("target path is not Unicode")?,
        ],
        &build.compiler_environment,
    )?;
    let executable = build.record_executable(&artifacts, "windows_abi", true)?;
    executed.extend(run_tests(build, &executable, "abi", None)?);
    Ok(executed)
}

pub(crate) fn smoke_tests(build: &mut Build) -> Result<BTreeSet<String>> {
    let setup = build.directory.join("KeyrillSetup.exe");
    cargo::run(build.command(&setup)?.arg("status"))?;
    let executable = build.directory.join("KeyrillDllSmoke.exe");
    cargo::run(
        build
            .command(&executable)?
            .arg("factory")
            .arg(build.directory.join("KeyrillTsf.dll")),
    )?;
    cargo::run(build.command(&executable)?.arg("activate"))?;
    let negative = build.directory.join("missing-adjacent");
    fs::create_dir_all(&negative)?;
    if negative.join("KeyrillTsf.dll").try_exists()? {
        return Err("negative activation directory contains a DLL".into());
    }
    let executable = copy(
        &executable,
        &negative,
        "KeyrillDllSmoke.exe",
        build.target,
        &mut build.binary_hashes,
    )?;
    cargo::run(build.command(&executable)?.arg("missing"))?;
    Ok(BTreeSet::from([
        "smoke::setup-status".to_owned(),
        "smoke::factory".to_owned(),
        "smoke::activation".to_owned(),
        "smoke::missing-adjacent".to_owned(),
    ]))
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    #[test]
    fn production_registrar_build_includes_its_adjacent_setup_dependency() {
        assert!(PRODUCTION_PACKAGES.contains(&"keyrill-windows-setup"));
        assert_eq!(
            PRODUCTION_EXECUTABLES,
            &["KeyrillRegister", "KeyrillSettings", "KeyrillSetup"]
        );
    }

    #[test]
    fn adjacent_setup_identity_is_checked_for_production_and_negative_launches() {
        let fixture = Fixture::new();
        let mut build = fixture.build();
        let current = std::env::current_exe().unwrap();
        let setup = copy(
            &current,
            &fixture.0,
            "KeyrillSetup.exe",
            Target::X64,
            &mut build.binary_hashes,
        )
        .unwrap();
        let registrar = copy(
            &current,
            &fixture.0,
            "KeyrillRegister.exe",
            Target::X64,
            &mut build.binary_hashes,
        )
        .unwrap();
        let negative = fixture.0.join("missing-adjacent");
        fs::create_dir(&negative).unwrap();
        let detached = copy(
            &registrar,
            &negative,
            "KeyrillRegister.exe",
            Target::X64,
            &mut build.binary_hashes,
        )
        .unwrap();
        build.command(&registrar).unwrap();
        build.command(&detached).unwrap();
        let mut image = fs::read(&setup).unwrap();
        image.push(0);
        fs::write(&setup, image).unwrap();
        assert!(build.command(&registrar).is_err());
        assert!(build.command(&detached).is_err());
    }

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join("keyrill-executable-binding-tests");
            fs::create_dir_all(&base).unwrap();
            loop {
                let directory = base.join(format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&directory) {
                    Ok(()) => return Self(directory),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("fixture directory failed: {error}"),
                }
            }
        }

        fn build(&self) -> Build {
            let digest = Digest::of_bytes(b"binding fixture");
            Build {
                target: Target::X64,
                binary_hashes: BTreeMap::new(),
                directory: self.0.clone(),
                identity: Identity {
                    source: digest,
                    production: digest,
                    test_api: digest,
                },
                test_executable: self.0.join("controlled.exe"),
                runtime_path: None,
                compiler_environment: Vec::new(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let base = std::env::temp_dir().join("keyrill-executable-binding-tests");
            assert_eq!(self.0.parent(), Some(base.as_path()));
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn untracked_real_executable_is_refused_before_enumeration() {
        let fixture = Fixture::new();
        let build = fixture.build();
        let executable = std::env::current_exe().unwrap();
        pe::validate(&fs::read(&executable).unwrap(), Target::X64).unwrap();
        assert_eq!(
            build.command(&executable).unwrap_err().to_string(),
            "Windows executable has no recorded artifact identity"
        );
        assert_eq!(
            execute_tests(&build, &executable, "untracked", None, None)
                .unwrap_err()
                .to_string(),
            "Windows executable has no recorded artifact identity"
        );
    }

    fn cargo_executable(path: &Path) -> Vec<cargo::Artifact> {
        vec![cargo::Artifact {
            name: "bound_test".to_owned(),
            executable: Some(path.to_owned()),
            files: vec![path.to_owned()],
            test: true,
        }]
    }

    #[test]
    fn native_executable_identity_is_immutable_after_cargo_acquisition() {
        let fixture = Fixture::new();
        let mut build = fixture.build();
        let path = fixture.0.join("native.exe");
        fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
        let artifacts = cargo_executable(&path);
        let executable = build
            .record_executable(&artifacts, "bound_test", true)
            .unwrap();
        let expected = build.binary_hashes[&executable];
        build.command(&executable).unwrap();
        let mut image = fs::read(&executable).unwrap();
        image.push(0);
        fs::write(&executable, image).unwrap();
        pe::validate(&fs::read(&executable).unwrap(), Target::X64).unwrap();
        assert_eq!(
            execute_tests(&build, &executable, "swapped", None, None)
                .unwrap_err()
                .to_string(),
            "Windows artifact changed after its build identity was recorded"
        );
        assert!(
            build
                .record_executable(&artifacts, "bound_test", true)
                .is_err()
        );
        assert_eq!(build.binary_hashes[&executable], expected);
    }

    #[test]
    #[ignore = "requires an explicit compiled x64 production DLL"]
    fn compiled_same_architecture_swaps_are_refused_before_launch() {
        let fixture = Fixture::new();
        let mut build = fixture.build();
        let dll = std::env::var_os("KEYRILL_PRODUCTION_DLL").expect("compiled DLL required");
        let replacement = fs::read(dll).unwrap();
        pe::validate(&replacement, Target::X64).unwrap();
        let source = fixture.0.join("native.exe");
        fs::copy(std::env::current_exe().unwrap(), &source).unwrap();
        let artifacts = cargo_executable(&source);
        let executable = build
            .record_executable(&artifacts, "bound_test", true)
            .unwrap();
        let expected = build.binary_hashes[&executable];
        let negative = fixture.0.join("missing-adjacent");
        fs::create_dir(&negative).unwrap();
        let copied = copy(
            &executable,
            &negative,
            "negative.exe",
            build.target,
            &mut build.binary_hashes,
        )
        .unwrap();
        assert_eq!(build.binary_hashes[&copied], expected);
        build.command(&copied).unwrap();
        fs::write(&copied, &replacement).unwrap();
        assert_eq!(
            build.command(&copied).unwrap_err().to_string(),
            "Windows artifact changed after its build identity was recorded"
        );
        assert_eq!(build.binary_hashes[&copied], expected);
        fs::copy(std::env::current_exe().unwrap(), &copied).unwrap();
        fs::write(&executable, replacement).unwrap();
        assert_eq!(
            execute_tests(&build, &executable, "swapped", None, None)
                .unwrap_err()
                .to_string(),
            "Windows artifact changed after its build identity was recorded"
        );
        assert!(build.command(&copied).is_err());
        assert!(
            build
                .record_executable(&artifacts, "bound_test", true)
                .is_err()
        );
        assert!(
            copy(
                &executable,
                &negative,
                "new-negative.exe",
                build.target,
                &mut build.binary_hashes,
            )
            .is_err()
        );
        assert!(!negative.join("new-negative.exe").exists());
        assert_eq!(build.binary_hashes[&executable], expected);
        assert_eq!(artifact_digest(&copied, Target::X64).unwrap(), expected);
    }
}
