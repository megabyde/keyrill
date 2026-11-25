include!("fixture.rs");
use super::{MAX_PACKAGE_FILES, MAX_PE_BYTES, MAX_RESOURCE_BYTES, read_observed};
use std::io::Cursor;
impl Fixture {
    fn replace(&self, manifest: &mut Value, name: &str, bytes: &[u8]) {
        fs::write(self.0.join(name), bytes).unwrap();
        manifest["files"][name] = json!(Digest::of_bytes(bytes).to_string());
        self.save(manifest);
    }
}

#[test]
fn synthetic_pe_package_is_deterministic_and_copy_verifiable_without_execution() {
    let one = Fixture::new();
    let two = Fixture::new();
    one.candidate();
    two.candidate();
    let one = Package::open(&one.0).unwrap();
    let two = Package::open(&two.0).unwrap();
    assert_eq!(one.digest(), two.digest());
    assert_eq!(one.files(), two.files());
}
#[test]
fn refuses_missing_resources_stale_files_and_missing_client_architecture() {
    for name in [
        "resources/english.ico",
        "x86/KeyrillTsf.dll",
        "licenses/synthetic-1/LICENSE",
    ] {
        let fixture = Fixture::new();
        fixture.candidate();
        fs::remove_file(fixture.0.join(name)).unwrap();
        assert!(Package::open(&fixture.0).is_err());
    }
    for name in [
        "stale.dll",
        "words-en.txt",
        "KeyrillProofEditor.exe",
        "KeyrillTsfTest.dll",
    ] {
        let fixture = Fixture::new();
        fixture.candidate();
        fs::write(fixture.0.join(name), b"unexpected").unwrap();
        assert!(Package::open(&fixture.0).is_err());
    }
}
#[test]
fn refuses_wrong_target_test_exports_resource_tampering_and_changed_source() {
    let fixture = Fixture::new();
    let mut manifest = fixture.candidate();
    fixture.replace(
        &mut manifest,
        "x86/KeyrillTsf.dll",
        &synthetic_pe(Target::X64, false),
    );
    assert!(Package::open(&fixture.0).is_err());
    fixture.replace(
        &mut manifest,
        "x86/KeyrillTsf.dll",
        &synthetic_pe(Target::X86, true),
    );
    assert!(Package::open(&fixture.0).is_err());
    fixture.replace(
        &mut manifest,
        "x86/KeyrillTsf.dll",
        &synthetic_pe(Target::X86, false),
    );
    assert!(Package::open(&fixture.0).is_ok());
    fixture.replace(&mut manifest, "resources/english.ico", b"stale icon");
    assert!(Package::open(&fixture.0).is_err());
    fixture.replace(&mut manifest, "resources/english.ico", &icon(false));
    manifest["source"] = json!("f".repeat(64));
    fixture.save(&manifest);
    assert!(Package::open(&fixture.0).is_err());
}
#[test]
fn refuses_hash_mismatch_and_narrow_or_wide_developer_paths() {
    let fixture = Fixture::new();
    fixture.candidate();
    fs::write(fixture.0.join("LICENSE"), b"changed").unwrap();
    assert!(Package::open(&fixture.0).is_err());
    let mut manifest = fixture.candidate();
    for path in [
        b"C:\\Users\\fixture\\source.rs".to_vec(),
        "C:\\Users\\fixture\\source.rs"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
    ] {
        let mut image = synthetic_pe(Target::X64, false);
        image.extend(path);
        fixture.replace(&mut manifest, "x64/KeyrillSetup.exe", &image);
        assert!(Package::open(&fixture.0).is_err());
    }
    assert!(reject_developer_paths(b"c:\\users\\\0/home/\0/users/\0").is_ok());
}

#[test]
fn refuses_sparse_oversized_files_before_content_validation() {
    let mut errors = Vec::new();
    for (name, length) in [
        ("package.json", 1024 * 1024 + 1),
        ("LICENSE", 4 * 1024 * 1024 + 1),
        ("model-provenance.json", 1024 * 1024 + 1),
        ("x64/KeyrillTsf.dll", 64 * 1024 * 1024 + 1),
    ] {
        let fixture = Fixture::new();
        fixture.candidate();
        fs::File::options()
            .write(true)
            .open(fixture.0.join(name))
            .unwrap()
            .set_len(length)
            .unwrap();
        let error = Package::open(&fixture.0).err().unwrap().to_string();
        errors.push((name, error));
    }
    assert!(
        errors
            .iter()
            .all(|(_, error)| error == "package file exceeds byte limit"),
        "{errors:?}"
    );
}

#[test]
fn refuses_excess_file_count() {
    let fixture = Fixture::new();
    let mut manifest = fixture.candidate();
    for index in 0..256 {
        let name = format!("licenses/synthetic-{index}/LICENSE");
        fs::create_dir_all(fixture.0.join(&name).parent().unwrap()).unwrap();
        fs::write(fixture.0.join(&name), b"Synthetic MIT license").unwrap();
        manifest["files"][&name] = json!(Digest::of_bytes(b"Synthetic MIT license").to_string());
    }
    fixture.save(&manifest);
    assert!(Package::open(&fixture.0).is_err());
}

#[test]
fn refuses_unapproved_directory_depth() {
    let fixture = Fixture::new();
    fixture.candidate();
    fs::create_dir_all(fixture.0.join("licenses/a/b/c/d/e")).unwrap();
    assert!(Package::open(&fixture.0).is_err());
}

#[test]
fn refuses_aggregate_bytes_before_reading_artifact_bodies() {
    let fixture = Fixture::new();
    fixture.candidate();
    for name in [
        "x64/KeyrillTsf.dll",
        "x64/KeyrillSettings.exe",
        "x64/KeyrillRegister.exe",
        "x64/KeyrillSetup.exe",
        "x86/KeyrillTsf.dll",
    ] {
        fs::File::options()
            .write(true)
            .open(fixture.0.join(name))
            .unwrap()
            .set_len(MAX_PE_BYTES)
            .unwrap();
    }
    assert_eq!(
        Package::open(&fixture.0).err().unwrap().to_string(),
        "package exceeds total byte limit"
    );
}

#[test]
fn accepts_file_count_limit_and_retained_receipts_across_repeated_reads() {
    let fixture = Fixture::new();
    let mut manifest = fixture.candidate();
    let remaining = MAX_PACKAGE_FILES - 1 - manifest["files"].as_object().unwrap().len();
    for index in 0..remaining {
        let name = format!("licenses/limit-{index}/LICENSE");
        fs::create_dir_all(fixture.0.join(&name).parent().unwrap()).unwrap();
        fs::write(fixture.0.join(&name), b"Synthetic MIT license").unwrap();
        manifest["files"][&name] = json!(Digest::of_bytes(b"Synthetic MIT license").to_string());
    }
    fixture.save(&manifest);
    let package = Package::open(&fixture.0).unwrap();
    assert_eq!(package.files().len() + 1, MAX_PACKAGE_FILES);
    let mut file = fs::File::open(fixture.0.join("LICENSE")).unwrap();
    package.verify_file("LICENSE", &mut file).unwrap();
    package.verify_file("LICENSE", &mut file).unwrap();
    assert!(package.verify_file("undeclared", &mut file).is_err());
    drop(file);
    fs::write(fixture.0.join("LICENSE"), b"altered").unwrap();
    let mut file = fs::File::open(fixture.0.join("LICENSE")).unwrap();
    assert!(package.verify_file("LICENSE", &mut file).is_err());
    drop(file);
    fs::File::options()
        .write(true)
        .open(fixture.0.join("LICENSE"))
        .unwrap()
        .set_len(MAX_RESOURCE_BYTES + 1)
        .unwrap();
    let mut file = fs::File::open(fixture.0.join("LICENSE")).unwrap();
    assert_eq!(
        package
            .verify_file("LICENSE", &mut file)
            .err()
            .unwrap()
            .to_string(),
        "package file exceeds byte limit"
    );
}

#[test]
fn observed_reader_requires_exact_length_and_stops_after_one_growth_byte() {
    for (input, observed, valid, position) in [
        (b"".as_slice(), 0, true, 0),
        (b"a".as_slice(), 1, true, 1),
        (b"a".as_slice(), 2, false, 1),
        (b"abcde".as_slice(), 2, false, 3),
        (b"abcde".as_slice(), 0, false, 1),
    ] {
        let mut reader = Cursor::new(input);
        let mut bytes = Vec::new();
        assert_eq!(
            read_observed(&mut reader, observed, |chunk| bytes
                .extend_from_slice(chunk))
            .is_ok(),
            valid
        );
        assert_eq!(reader.position(), position);
        if valid {
            assert_eq!(bytes, input);
        }
    }
    let mut reader = Cursor::new(vec![7; 32 * 1024 + 10]);
    assert!(read_observed(&mut reader, 32 * 1024, |_| {}).is_err());
    assert_eq!(reader.position(), 32 * 1024 + 1);
}

#[test]
fn developer_path_matching_remains_case_insensitive_without_copying_the_image() {
    for text in [
        "C:\\UsErS\\synthetic\\source.rs",
        "C:\\PrOjEcTs\\KeYrIlL\\src\\source.rs",
        "/HoMe/synthetic/source.rs",
        "/UsErS/synthetic/source.rs",
        "C:\\.CaRgO\\registry\\source.rs",
        "C:\\.RuStUp\\toolchains\\source.rs",
    ] {
        assert!(reject_developer_paths(text.as_bytes()).is_err());
        let wide: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(reject_developer_paths(&wide).is_err());
    }
}

#[cfg(windows)]
#[test]
fn retained_validation_directories_and_files_deny_replacement() {
    let fixture = Fixture::new();
    fixture.candidate();
    let (root, mut leases) = super::retain_root(&fixture.0).unwrap();
    let package = Package::open(&fixture.0).unwrap();
    let mut expected: std::collections::BTreeSet<_> = package.files().keys().cloned().collect();
    expected.insert("package.json".into());
    let mut files = std::collections::BTreeMap::new();
    let directory = super::Directory {
        path: root.clone(),
        file: leases.pop().unwrap(),
    };
    super::walk(
        &root,
        directory,
        &expected,
        &mut files,
        &mut leases,
        &mut 0,
        &mut 0,
    )
    .unwrap();
    for name in ["resources", "licenses/synthetic-1", "x64"] {
        assert!(fs::rename(root.join(name), root.join("replacement")).is_err());
    }
    assert!(fs::write(root.join("LICENSE"), b"altered").is_err());
    assert!(fs::remove_file(root.join("LICENSE")).is_err());
}
