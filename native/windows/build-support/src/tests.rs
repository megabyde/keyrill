use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join("keyrill-windows-build-tests");
        fs::create_dir_all(&base).unwrap();
        loop {
            let next = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let directory = base.join(format!("{}-{next}", std::process::id()));
            match fs::create_dir(&directory) {
                Ok(()) => return Self(directory),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("test directory creation failed: {error}"),
            }
        }
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn identity(&self) -> Identity {
        identity(&self.0, config()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let expected_parent = std::env::temp_dir().join("keyrill-windows-build-tests");
        assert_eq!(self.0.parent(), Some(expected_parent.as_path()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn config() -> BuildConfig<'static> {
    BuildConfig {
        target: "x86_64-pc-windows-msvc",
        profile: "release",
        instrumentation: Instrumentation::None,
    }
}

#[test]
fn source_path_classification_matches_the_enumerated_release_closure() {
    let fixture = Fixture::new();
    let inputs = [
        "Cargo.lock",
        "rust-toolchain.toml",
        "native/windows/setup/build.rs",
        "native/windows/setup/setup.manifest",
        "native/windows/setup/src/space и name.rs",
    ];
    let ignored = [
        "docs/notes.json",
        ".audit/run.json",
        "native/windows/.audit/run.json",
        "native/windows/target/generated.rs",
        "native/windows/cache.log",
        "native/windows/notes.md",
    ];
    for path in inputs.iter().chain(ignored.iter()) {
        fixture.write(path, b"fixture");
    }
    let actual = source_inputs(&fixture.0).unwrap();
    for path in inputs.iter().chain(ignored.iter()) {
        assert_eq!(
            is_source_input(Path::new(path)),
            actual.contains(&fixture.0.join(path)),
            "{path}"
        );
    }
    assert_eq!(actual.len(), inputs.len());
}

#[test]
fn sha256_and_diagnostic_word_order_match_the_standard_vector() {
    let hash = Digest::of_bytes(b"abc");
    assert_eq!(
        hash.to_string(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(hash.words()[0], 0xba7816bf);
    assert_eq!(hash.words()[7], 0xf20015ad);
}

#[test]
fn source_records_use_relative_paths_and_sorted_normalized_text() {
    let fixture = Fixture::new();
    fixture.write("native/windows/z.rs", b"line\r\n");
    fixture.write("native/windows/a.rs", b"abc");
    let expected_records = format!(
        "native/windows/a.rs:{}\nnative/windows/z.rs:{}\n",
        Digest::of_bytes(b"abc"),
        Digest::of_bytes(b"line\n")
    );
    assert_eq!(
        fixture.identity().source,
        Digest::of_bytes(expected_records.as_bytes())
    );
    let before = fixture.identity();
    fixture.write("native/windows/z.rs", b"line\n");
    assert_eq!(fixture.identity(), before);
    fixture.write("native/windows/z.rs", b"line\r");
    assert_ne!(fixture.identity(), before);
}

#[test]
fn binary_bytes_are_preserved() {
    let fixture = Fixture::new();
    fixture.write("assets/model.bin", b"model\r\n");
    let before = fixture.identity();
    fixture.write("assets/model.bin", b"model\n");
    assert_ne!(fixture.identity().source, before.source);
}

#[test]
fn source_additions_and_removals_change_identity() {
    let fixture = Fixture::new();
    fixture.write("native/windows/first.rs", b"first");
    let before = fixture.identity();
    fixture.write("native/windows/new/second.rs", b"second");
    assert_ne!(fixture.identity(), before);
    fs::remove_file(fixture.0.join("native/windows/new/second.rs")).unwrap();
    assert_eq!(fixture.identity(), before);
}

#[test]
fn frozen_model_inputs_and_provenance_outputs_are_part_of_the_identity() {
    let fixture = Fixture::new();
    for file in [
        "model/data/model.inc.gz",
        "model/data/model-manifest.json",
        "model/data/model-notice.txt",
        "model/data/wordfreq-code-license.txt",
        "assets/model-provenance.json",
        "assets/model-notice.txt",
        "assets/wordfreq-code-license.txt",
    ] {
        let before = fixture.identity();
        fixture.write(file, b"first\r\n");
        let added = fixture.identity();
        assert_ne!(added.source, before.source, "{file}");
        fixture.write(file, b"second\r\n");
        assert_ne!(fixture.identity().source, added.source, "{file}");
    }
    fixture.write("model/data/model.inc.gz", b"binary\r\n");
    let before = fixture.identity();
    fixture.write("model/data/model.inc.gz", b"binary\n");
    assert_ne!(fixture.identity().source, before.source);
}

#[test]
fn tooling_is_hashed_and_generated_directories_are_excluded() {
    let fixture = Fixture::new();
    fixture.write("tools/xtask/src/main.rs", b"tool");
    let before = fixture.identity();
    fixture.write("native/windows/target/generated.rs", b"generated");
    assert_eq!(fixture.identity(), before);
    fixture.write("tools/xtask/src/main.rs", b"changed tool");
    assert_ne!(fixture.identity(), before);
}

#[test]
fn feature_target_profile_and_instrumentation_have_distinct_domains() {
    let fixture = Fixture::new();
    fixture.write("Cargo.toml", b"workspace");
    let ordinary = fixture.identity();
    assert_ne!(ordinary.production, ordinary.test_api);
    for changed in [
        BuildConfig {
            target: Target::X86.triple(),
            ..config()
        },
        BuildConfig {
            target: "x86_64-unknown-linux-gnu",
            ..config()
        },
        BuildConfig {
            profile: "debug",
            ..config()
        },
        BuildConfig {
            instrumentation: Instrumentation::Address,
            ..config()
        },
    ] {
        let changed = identity(&fixture.0, changed).unwrap();
        assert_eq!(changed.source, ordinary.source);
        assert_ne!(changed.production, ordinary.production);
    }
}

#[test]
fn windows_targets_have_exact_native_and_manifest_metadata() {
    for (target, name, triple, machine, magic, bits, manifest) in [
        (
            Target::X64,
            "x64",
            "x86_64-pc-windows-msvc",
            0x8664,
            0x20b,
            64,
            "amd64",
        ),
        (
            Target::X86,
            "x86",
            "i686-pc-windows-msvc",
            0x014c,
            0x10b,
            32,
            "x86",
        ),
    ] {
        assert_eq!(name.parse::<Target>(), Ok(target));
        assert_eq!(triple.parse::<Target>(), Ok(target));
        assert_eq!(target.name(), name);
        assert_eq!(target.triple(), triple);
        assert_eq!(target.pe_machine(), machine);
        assert_eq!(target.pe_magic(), magic);
        assert_eq!(target.pointer_bits(), bits);
        assert_eq!(target.manifest_architecture(), manifest);
    }
    for unsupported in ["arm64", "aarch64-pc-windows-msvc", "all", "unknown"] {
        assert!(unsupported.parse::<Target>().is_err());
    }
}

#[test]
fn generated_rust_constant_contains_the_selected_words() {
    let fixture = Fixture::new();
    let digest = Digest::of_bytes(b"abc");
    let output = fixture.0.join("identity.rs");
    write_rust_identity(&output, digest).unwrap();
    let text = fs::read_to_string(output).unwrap();
    assert!(text.starts_with("pub(crate) const BUILD_DIGEST: [u32; 8] = [0xba7816bf"));
    assert!(text.ends_with("0xf20015ad];\n"));
}
