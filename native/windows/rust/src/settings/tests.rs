use super::*;
use native::testing;
use std::cell::Cell;
use std::sync::{Arc, Barrier};
use windows::Win32::Foundation::*;
use windows::Win32::System::Registry::HKEY;
use windows_core::PCWSTR;

struct Isolated(Store);

impl Isolated {
    fn new() -> Self {
        Self(Store {
            native: testing::isolated(),
        })
    }

    fn save(&self, expected: Revision, value: Preferences) -> Snapshot {
        match self.0.save(expected, value) {
            SaveOutcome::Saved(saved) => saved,
            other => panic!("unexpected settings outcome {other:?}"),
        }
    }
}

impl Drop for Isolated {
    fn drop(&mut self) {
        testing::cleanup(&self.0.native);
    }
}

fn complete() -> Preferences {
    Preferences {
        automatic: true,
        switch_source: false,
        dictionaries: [
            DictionaryPath::Explicit(LocalPath::new("c:\\Words\\English.txt").unwrap()),
            DictionaryPath::Default,
        ],
        applications: BTreeMap::from([
            (
                ApplicationIdentity::executable("c:\\Apps\\Editor.exe").unwrap(),
                AppPolicy::Manual,
            ),
            (
                ApplicationIdentity::package_family("Example.Editor_123456789abcd").unwrap(),
                AppPolicy::Off,
            ),
        ]),
    }
}

#[test]
fn codec_roundtrip_is_complete_canonical_and_private() {
    let snapshot = Snapshot {
        revision: Revision(Some([1; 16])),
        value: Arc::new(complete()),
    };
    let bytes = codec::encode(&snapshot).unwrap();
    assert_eq!(codec::decode(&bytes), Ok(snapshot.clone()));
    assert_eq!(
        bytes,
        codec::encode(&codec::decode(&bytes).unwrap()).unwrap()
    );
    let displayed = format!("{snapshot:?}");
    assert!(!displayed.contains("English"));
    assert!(!displayed.contains("Editor"));
    assert_eq!(
        ApplicationIdentity::executable("c:\\Apps\\Editor.exe"),
        ApplicationIdentity::executable("C:\\APPS\\EDITOR.EXE")
    );
}

#[test]
fn codec_rejects_truncation_future_version_overflow_and_noncanonical_fields() {
    let snapshot = Snapshot {
        revision: Revision(Some([1; 16])),
        value: Arc::new(complete()),
    };
    let bytes = codec::encode(&snapshot).unwrap();
    for length in 0..bytes.len() {
        assert_eq!(codec::decode(&bytes[..length]), Err(SettingsError::Invalid));
    }
    for (index, value) in [
        (4, 2),
        (6, 1),
        (8, 255),
        (28, 2),
        (30, 9),
        (31, 255),
        (32, 255),
    ] {
        let mut invalid = bytes.clone();
        invalid[index] = value;
        assert_eq!(codec::decode(&invalid), Err(SettingsError::Invalid));
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    let length = trailing.len() as u32;
    trailing[8..12].copy_from_slice(&length.to_le_bytes());
    assert_eq!(codec::decode(&trailing), Err(SettingsError::Invalid));
    assert_eq!(
        codec::decode(&vec![0; MAX_BYTES + 1]),
        Err(SettingsError::Invalid)
    );
    let mut zero = bytes;
    zero[12..28].fill(0);
    assert_eq!(codec::decode(&zero), Err(SettingsError::Invalid));
}

#[test]
fn codec_rejects_invalid_utf16_aliases_counts_duplicate_and_unsorted_identities() {
    let snapshot = Snapshot {
        revision: Revision(Some([1; 16])),
        value: Arc::new(complete()),
    };
    let encoded = codec::encode(&snapshot).unwrap();
    for first_unit in [b'c' as u16, 0, 0xd800] {
        let mut candidate = encoded.clone();
        candidate[33..35].copy_from_slice(&first_unit.to_le_bytes());
        assert_eq!(codec::decode(&candidate), Err(SettingsError::Invalid));
    }
    let count_offset = 31 + 2 + "C:\\Words\\English.txt".encode_utf16().count() * 2 + 1;
    let entries_offset = count_offset + 2;
    let first_length = 1 + 2 + "C:\\APPS\\EDITOR.EXE".encode_utf16().count() * 2 + 1;
    let first = &encoded[entries_offset..entries_offset + first_length];
    let second = &encoded[entries_offset + first_length..];
    for (left, right) in [(first, first), (second, first)] {
        let mut candidate = encoded[..entries_offset].to_vec();
        candidate.extend(left);
        candidate.extend(right);
        let length = candidate.len() as u32;
        candidate[8..12].copy_from_slice(&length.to_le_bytes());
        assert_eq!(codec::decode(&candidate), Err(SettingsError::Invalid));
    }
    let empty = Snapshot {
        revision: Revision(Some([1; 16])),
        value: Arc::default(),
    };
    let mut count = codec::encode(&empty).unwrap();
    count[32..34].copy_from_slice(&257u16.to_le_bytes());
    assert_eq!(codec::decode(&count), Err(SettingsError::Invalid));
}

#[test]
fn paths_and_application_counts_are_bounded_at_the_boundary() {
    for path in [
        "relative.txt",
        "C:\\..\\words",
        "C:\\NUL",
        "C:\\x.",
        "C:\\x ",
        "C:\\x:stream",
        "\\\\server\\share",
        "\\\\?\\C:\\x",
    ] {
        assert_eq!(LocalPath::new(path), Err(SettingsError::Invalid));
    }
    assert_eq!(
        LocalPath::new(&format!("C:\\{}", "x".repeat(256))),
        Err(SettingsError::Invalid)
    );
    assert_eq!(
        LocalPath::new(&format!("C:\\{}x", "a\\".repeat(64))),
        Err(SettingsError::Invalid)
    );
    assert_eq!(
        ApplicationIdentity::package_family("invalid"),
        Err(SettingsError::Invalid)
    );
    let mut value = Preferences::default();
    for index in 0..=MAX_APPLICATIONS {
        value.applications.insert(
            ApplicationIdentity::executable(&format!("C:\\Apps\\App{index}.exe")).unwrap(),
            AppPolicy::Manual,
        );
    }
    let snapshot = Snapshot {
        revision: Revision(Some([1; 16])),
        value: Arc::new(value),
    };
    assert_eq!(codec::encode(&snapshot), Err(SettingsError::Invalid));
}

#[test]
fn actual_registry_defaults_complete_save_conflict_and_nonce_aba() {
    let store = Isolated::new();
    let missing = store.0.load().unwrap();
    assert_eq!(missing, Snapshot::default());
    assert_eq!(store.0.native.read().unwrap(), None);
    testing::unrelated_value(&store.0.native, true);
    let first = store.save(missing.revision, complete());
    assert_eq!(store.0.load().unwrap(), first);
    assert_ne!(first.revision, missing.revision);
    assert_eq!(
        store.0.save(missing.revision, Preferences::default()),
        SaveOutcome::Conflict(first.clone())
    );
    let same_value = store.save(first.revision, complete());
    assert_ne!(first.revision, same_value.revision);
    testing::delete_value(&store.0.native);
    assert_eq!(
        store.0.save(same_value.revision, complete()),
        SaveOutcome::Conflict(Snapshot::default())
    );
    let recreated = store.save(missing.revision, complete());
    assert_ne!(recreated.revision, first.revision);
    assert_eq!(
        store.0.save(first.revision, Preferences::default()),
        SaveOutcome::Conflict(recreated)
    );
    testing::unrelated_value(&store.0.native, false);
}

#[test]
fn actual_registry_invalid_values_retain_last_good_at_reader_boundary() {
    let store = Isolated::new();
    let saved = store.save(Revision::default(), complete());
    let mut reader = Reader::default();
    assert_eq!(reader.refresh(&store.0).unwrap(), &saved);
    for invalid in [vec![], vec![1; 10], vec![0; MAX_BYTES + 1], {
        let mut future = codec::encode(&saved).unwrap();
        future[4] = 2;
        future
    }] {
        store.0.native.write(&invalid).unwrap();
        assert_eq!(reader.refresh(&store.0), Err(SettingsError::Invalid));
        assert_eq!(reader.snapshot(), &saved);
        assert_eq!(
            store.0.save(saved.revision, Preferences::default()),
            SaveOutcome::Rejected(SettingsError::Invalid)
        );
    }
    testing::wrong_type(&store.0.native);
    assert_eq!(reader.refresh(&store.0), Err(SettingsError::Invalid));
    assert_eq!(reader.snapshot(), &saved);
    testing::delete_value(&store.0.native);
    assert_eq!(reader.refresh(&store.0).unwrap(), &Snapshot::default());
}

unsafe extern "system" fn mismatched_readback(
    key: HKEY,
    name: PCWSTR,
    reserved: *const u32,
    kind: *mut u32,
    bytes: *mut u8,
    length: *mut u32,
) -> WIN32_ERROR {
    let count = QUERIES.with(|count| {
        count.set(count.get() + 1);
        count.get()
    });
    // SAFETY: forwarded unchanged from the store native boundary.
    let result = unsafe { testing::real_query(key, name, reserved, kind, bytes, length) };
    if count == 2 && result == ERROR_SUCCESS {
        // SAFETY: a successful test-value query returned the nonempty settings buffer.
        unsafe { *bytes ^= 1 };
    }
    result
}

unsafe extern "system" fn denied_set(
    _: HKEY,
    _: PCWSTR,
    _: u32,
    _: u32,
    _: *const u8,
    _: u32,
) -> WIN32_ERROR {
    ERROR_ACCESS_DENIED
}

unsafe extern "system" fn denied_query(
    _: HKEY,
    _: PCWSTR,
    _: *const u32,
    _: *mut u32,
    _: *mut u8,
    _: *mut u32,
) -> WIN32_ERROR {
    ERROR_ACCESS_DENIED
}

#[test]
fn unavailable_native_provider_retains_complete_last_good_reader_state() {
    let mut store = Isolated::new();
    let saved = store.save(Revision::default(), complete());
    let mut reader = Reader::default();
    reader.refresh(&store.0).unwrap();
    testing::fail_query(&mut store.0.native, denied_query);
    assert_eq!(reader.refresh(&store.0), Err(SettingsError::Unavailable));
    assert_eq!(reader.snapshot(), &saved);
    assert_eq!(
        store.0.save(saved.revision, Preferences::default()),
        SaveOutcome::Rejected(SettingsError::Unavailable)
    );
}

#[test]
fn native_provider_mismatched_readback_and_failed_write_never_report_saved() {
    let mut store = Isolated::new();
    let first = store.save(Revision::default(), Preferences::default());
    testing::fail_query(&mut store.0.native, mismatched_readback);
    QUERIES.with(|count| count.set(0));
    assert_eq!(
        store.0.save(first.revision, complete()),
        SaveOutcome::Rejected(SettingsError::Readback)
    );
    let actual = store.0.load().unwrap();
    assert_eq!(actual.value.as_ref(), &complete());
    testing::fail_set(&mut store.0.native, denied_set);
    assert_eq!(
        store.0.save(actual.revision, Preferences::default()),
        SaveOutcome::Rejected(SettingsError::Unavailable)
    );
    assert_eq!(store.0.load().unwrap(), actual);
}

#[test]
fn process_writer_child() {
    let Ok(namespace) = std::env::var("KEYRILL_SETTINGS_TEST_NAMESPACE") else {
        return;
    };
    let expected = std::env::var("KEYRILL_SETTINGS_TEST_REVISION").unwrap();
    let nonce = u128::from_str_radix(&expected, 16).unwrap().to_le_bytes();
    let store = Store {
        native: testing::open_isolated(&namespace),
    };
    match store.save(Revision(Some(nonce)), complete()) {
        SaveOutcome::Saved(_) => println!("settings-child-saved"),
        SaveOutcome::Conflict(_) => println!("settings-child-conflict"),
        SaveOutcome::Rejected(error) => panic!("child settings save rejected {error}"),
    }
}

#[test]
fn separate_process_writers_share_one_native_cas_lock() {
    let store = Isolated::new();
    let first = store.save(Revision::default(), Preferences::default());
    let expected = format!("{:032x}", u128::from_le_bytes(first.revision.0.unwrap()));
    let namespace = testing::namespace(&store.0.native);
    let mut children = Vec::new();
    for _ in 0..2 {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "settings::tests::process_writer_child",
                    "--nocapture",
                ])
                .env("KEYRILL_SETTINGS_TEST_NAMESPACE", &namespace)
                .env("KEYRILL_SETTINGS_TEST_REVISION", &expected)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let mut saved = 0;
    let mut conflicts = 0;
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        saved += usize::from(stdout.contains("settings-child-saved"));
        conflicts += usize::from(stdout.contains("settings-child-conflict"));
    }
    assert_eq!((saved, conflicts), (1, 1));
    assert_eq!(store.0.load().unwrap().value.as_ref(), &complete());
}

#[test]
fn concurrent_readers_observe_only_complete_old_or_new_values() {
    let store = Isolated::new();
    let first = store.save(Revision::default(), Preferences::default());
    let native = store.0.native.clone();
    let writer = std::thread::spawn(move || {
        let store = Store { native };
        let mut revision = first.revision;
        for index in 0..40 {
            let value = if index % 2 == 0 {
                complete()
            } else {
                Preferences::default()
            };
            let SaveOutcome::Saved(saved) = store.save(revision, value) else {
                panic!("sole writer failed");
            };
            revision = saved.revision;
        }
    });
    for _ in 0..80 {
        let value = store.0.load().unwrap().value;
        assert!(value.as_ref() == &Preferences::default() || value.as_ref() == &complete());
    }
    writer.join().unwrap();
}

thread_local! { static QUERIES: Cell<u32> = const { Cell::new(0) }; }

unsafe extern "system" fn failed_readback(
    key: HKEY,
    name: PCWSTR,
    reserved: *const u32,
    kind: *mut u32,
    bytes: *mut u8,
    length: *mut u32,
) -> WIN32_ERROR {
    let count = QUERIES.with(|count| {
        count.set(count.get() + 1);
        count.get()
    });
    if count == 2 {
        return ERROR_ACCESS_DENIED;
    }
    // SAFETY: forwarded unchanged from the store native boundary.
    unsafe { testing::real_query(key, name, reserved, kind, bytes, length) }
}

#[test]
fn actual_write_requires_exact_fresh_readback_and_stale_retry_conflicts() {
    let mut store = Isolated::new();
    let first = store.save(Revision::default(), Preferences::default());
    testing::fail_query(&mut store.0.native, failed_readback);
    QUERIES.with(|count| count.set(0));
    assert_eq!(
        store.0.save(first.revision, complete()),
        SaveOutcome::Rejected(SettingsError::Readback)
    );
    let actual = store.0.load().unwrap();
    assert_ne!(actual.revision, first.revision);
    assert_eq!(actual.value.as_ref(), &complete());
    assert_eq!(
        store.0.save(first.revision, Preferences::default()),
        SaveOutcome::Conflict(actual)
    );
}

#[test]
fn concurrent_writers_compare_inside_private_native_lock() {
    let store = Isolated::new();
    let first = store.save(Revision::default(), Preferences::default());
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let native = store.0.native.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let store = Store { native };
            barrier.wait();
            store.save(first.revision, complete())
        }));
    }
    barrier.wait();
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, SaveOutcome::Saved(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, SaveOutcome::Conflict(_)))
            .count(),
        1
    );
}

#[test]
fn actual_registry_abandoned_lock_rereads_and_rejects_stale_save() {
    let store = Isolated::new();
    let first = store.save(Revision::default(), Preferences::default());
    let newer = store.save(first.revision, complete());
    let _retained = testing::abandon(&store.0.native);
    assert_eq!(
        store.0.save(first.revision, Preferences::default()),
        SaveOutcome::Conflict(newer)
    );
}

#[test]
fn actual_registry_existing_insecure_writer_object_is_rejected_without_write() {
    let store = Isolated::new();
    let _object = testing::insecure_mutex(&store.0.native);
    assert_eq!(
        store.0.save(Revision::default(), complete()),
        SaveOutcome::Rejected(SettingsError::Security)
    );
    assert_eq!(store.0.native.read().unwrap(), None);
    // Create the test key so the cleanup assertion also checks its uniquely owned namespace.
    store.0.native.write(&[]).unwrap();
}

#[test]
fn actual_writer_object_has_explicit_medium_integrity_label() {
    let store = Isolated::new();
    store.save(Revision::default(), Preferences::default());
    let label = testing::writer_integrity_label(&store.0.native);
    assert_eq!(
        label.split_once('(').map(|(_, ace)| ace),
        Some("ML;;NW;;;ME)")
    );
}

#[test]
fn actual_registry_existing_low_integrity_writer_object_is_rejected_without_write() {
    let store = Isolated::new();
    let _object = testing::low_integrity_mutex(&store.0.native);
    assert_eq!(
        store.0.save(Revision::default(), complete()),
        SaveOutcome::Rejected(SettingsError::Security)
    );
    assert_eq!(store.0.native.read().unwrap(), None);
    store.0.native.write(&[]).unwrap();
}
