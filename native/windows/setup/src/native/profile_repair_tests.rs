use super::*;
use crate::package_fixture::Fixture;

fn fixture() -> (Vec<u8>, Vec<u8>, Value) {
    let original = Journal {
        desired: "a".repeat(64),
        previous: None,
        completed: 1,
        changes: vec![
            Change {
                resource: "service".into(),
                before: json!(false),
                after: json!(true),
            },
            Change {
                resource: "profile/en".into(),
                before: json!({"description":"proof EN","icon":"old.ico","index":0}),
                after: json!({"description":"Keyrill English","icon":"english.ico","index":0}),
            },
            Change {
                resource: "profile/ru".into(),
                before: Value::Null,
                after: json!({"description":"Keyrill Russian","icon":"russian.ico","index":0}),
            },
        ],
    };
    let original = serde_json::to_vec(&original.value()).unwrap();
    let mut resources: serde_json::Map<_, _> = Installation::resources()
        .into_iter()
        .filter(|r| r != "profile/en")
        .map(|r| (r, Value::Null))
        .collect();
    resources.insert("service".into(), json!(true));
    let source = json!({"Description":{"type":1,"data":[0,216,0,0]},
        "IconFile":registry::string("english.ico"),"IconIndex":registry::dword(0)});
    let snapshot = json!({"source":[source,source],"protected":{"resources":resources,"installed":null,
        "native":{"user":[1,2,3],"trees":[{"Enable":registry::string("1")}],
            "layouts":[67699721]}},"session":{"scope":"setup-owner-thread","layout":1}});
    let value = json!({"schema":1,"original":digest(&original),"desired":"a".repeat(64),
        "previous":null,"cursor":1,"count":3,"profile":"profile/en","source":snapshot["source"],
        "protected":snapshot["protected"],"session_observation":snapshot["session"],
        "tool":{"package":"b".repeat(64),"executable":"c".repeat(64),"source":"d".repeat(64)},
        "predecessor":null});
    (
        original,
        serde_json::to_vec_pretty(&value).unwrap(),
        snapshot,
    )
}
fn approval() -> (Approval, Value) {
    let (original, bytes, snapshot) = fixture();
    (
        Approval::parse(&bytes, &digest(&bytes), &original).unwrap(),
        snapshot,
    )
}

struct Provider {
    snapshot: Value,
    saved: Phase,
    calls: usize,
    fail: usize,
    writes: usize,
    effect: bool,
    native_error: bool,
    drift: Option<&'static str>,
    outcome: Value,
}
impl Provider {
    fn new(snapshot: Value) -> Self {
        Self {
            snapshot,
            saved: Phase::Prepared,
            calls: 0,
            fail: usize::MAX,
            writes: 0,
            effect: true,
            native_error: false,
            drift: None,
            outcome: Value::Null,
        }
    }
    fn tick(&mut self) -> Result<()> {
        self.calls += 1;
        if self.calls == self.fail {
            Err("interrupted boundary".into())
        } else {
            Ok(())
        }
    }
}
impl RepairIo for Provider {
    fn snapshot(&mut self, _: Profile) -> Result<Value> {
        self.tick()?;
        Ok(self.snapshot.clone())
    }
    fn checkpoint(&mut self, phase: Phase, outcome: &Value) -> Result<()> {
        self.tick()?;
        self.saved = phase;
        self.outcome = retained_outcome(&self.outcome, outcome);
        Ok(())
    }
    fn write_profile(&mut self, profile: Profile, destination: &Value) -> Result<()> {
        assert_eq!(profile, Profile::English);
        self.writes += 1;
        self.tick()?;
        if self.effect {
            self.snapshot["source"] = raw_destination(destination)?;
        }
        match self.drift {
            Some("enable") => {
                self.snapshot["protected"]["resources"]["enable/en"] = registry::dword(0)
            }
            Some("metadata") => {
                self.snapshot["protected"]["native"]["trees"][0]["Enable"] = Value::Null
            }
            Some("sid") => self.snapshot["protected"]["native"]["user"] = json!([4, 5, 6]),
            Some("source") => self.snapshot["session"]["layout"] = json!(2),
            Some("profile") => {
                self.snapshot["source"][0]["Description"] = registry::string("different")
            }
            Some(_) => panic!("unknown injected drift"),
            None => (),
        }
        if self.native_error {
            Err("AddLanguageProfile HRESULT 0x80004005".into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn every_attempt_boundary_survives_restart_without_replay() {
    for fail in 1..=6 {
        let (approval, snapshot) = approval();
        let mut io = Provider::new(snapshot);
        io.fail = fail;
        let _ = execute(&approval, Phase::Prepared, &mut io);
        let persisted = io.saved;
        let attempted = io.writes;
        io.fail = usize::MAX;
        let resumed = execute(&approval, persisted, &mut io);
        if persisted != Phase::Prepared {
            assert_eq!(io.writes, attempted, "failure {fail}");
        }
        assert!(io.writes <= 1);
        if resumed.is_ok() {
            assert_eq!(io.saved, Phase::Verified);
            let count = io.writes;
            execute(&approval, io.saved, &mut io).unwrap();
            assert_eq!(io.writes, count);
        }
    }
}

#[test]
fn unchanged_source_after_attempt_requires_new_linked_approval() {
    let (approval, snapshot) = approval();
    let mut io = Provider::new(snapshot);
    io.effect = false;
    assert!(execute(&approval, Phase::Prepared, &mut io).is_err());
    assert_eq!(io.saved, Phase::Stopped);
    for _ in 0..3 {
        assert!(execute(&approval, io.saved, &mut io).is_err());
    }
    assert_eq!(io.writes, 1);
}

#[test]
fn native_failure_with_exact_readback_verifies_and_retains_hresult() {
    let (approval, snapshot) = approval();
    let mut io = Provider::new(snapshot);
    io.native_error = true;
    execute(&approval, Phase::Prepared, &mut io).unwrap();
    assert_eq!(io.saved, Phase::Verified);
    assert_eq!(
        io.outcome["native"]["error"],
        "AddLanguageProfile HRESULT 0x80004005"
    );
    let outcome = io.outcome.clone();
    execute(&approval, io.saved, &mut io).unwrap();
    assert_eq!(io.outcome, outcome);
    assert_eq!(io.writes, 1);
}

#[test]
fn restart_retains_attempt_session_but_reports_its_preservation_unavailable() {
    let (approval, snapshot) = approval();
    let original_session = snapshot["session"].clone();
    let mut io = Provider::new(snapshot);
    io.fail = 6;
    assert!(execute(&approval, Phase::Prepared, &mut io).is_err());
    assert_eq!(io.saved, Phase::Attempted);
    assert_eq!(io.outcome["session_before"], original_session);
    io.fail = usize::MAX;
    io.snapshot["session"]["layout"] = json!(77);
    execute(&approval, io.saved, &mut io).unwrap();
    assert_eq!(io.writes, 1);
    assert_eq!(io.outcome["session_before"], original_session);
    assert_eq!(io.outcome["snapshot"]["session"]["layout"], 77);
    assert_eq!(
        io.outcome["session_verification"],
        "unavailable-after-restart"
    );
    let outcome = io.outcome.clone();
    execute(&approval, io.saved, &mut io).unwrap();
    assert_eq!(io.outcome, outcome);
}

#[test]
fn post_call_protected_or_source_drift_stops_without_compensation() {
    for drift in ["enable", "metadata", "sid", "source", "profile"] {
        let (approval, snapshot) = approval();
        let mut io = Provider::new(snapshot);
        io.drift = Some(drift);
        assert!(
            execute(&approval, Phase::Prepared, &mut io).is_err(),
            "{drift}"
        );
        assert_eq!(io.saved, Phase::Stopped);
        let before = io.writes;
        let _ = execute(&approval, io.saved, &mut io);
        assert_eq!(io.writes, before);
    }
}

#[test]
fn protected_drift_before_attempt_refuses_all_writes() {
    for name in ["service", "profile/ru", "enable/en", "com/user/x64/server"] {
        let (approval, mut snapshot) = approval();
        snapshot["protected"]["resources"][name] = json!("changed");
        let mut io = Provider::new(snapshot);
        assert!(execute(&approval, Phase::Prepared, &mut io).is_err());
        assert_eq!(io.saved, Phase::Prepared);
        assert_eq!(io.writes, 0);
    }
}

#[test]
fn inspection_thread_source_is_informational_but_call_source_is_protected() {
    let (approval, mut snapshot) = approval();
    snapshot["session"]["layout"] = json!(42);
    let mut io = Provider::new(snapshot);
    execute(&approval, Phase::Prepared, &mut io).unwrap();
    assert_eq!(io.writes, 1);
    assert_eq!(io.outcome["snapshot"]["session"]["layout"], 42);
}

#[test]
fn approval_rejects_changed_receipt_identity_cursor_raw_views_and_other_conflicts() {
    let (original, bytes, _) = fixture();
    assert!(Approval::parse(&bytes, &"f".repeat(64), &original).is_err());
    let mut changed = original.clone();
    changed.push(b' ');
    assert!(Approval::parse(&bytes, &digest(&bytes), &changed).is_err());
    for field in [
        "original", "desired", "previous", "cursor", "count", "profile", "tool", "source", "extra",
    ] {
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value[field] = json!("invalid");
        let edited = serde_json::to_vec(&value).unwrap();
        assert!(
            Approval::parse(&edited, &digest(&edited), &original).is_err(),
            "{field}"
        );
    }
    for resource in ["service", "profile/ru"] {
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value["protected"]["resources"][resource] = json!("foreign");
        let edited = serde_json::to_vec(&value).unwrap();
        assert!(Approval::parse(&edited, &digest(&edited), &original).is_err());
    }
    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    value["source"][1]["Description"] = registry::string("divergent view");
    let edited = serde_json::to_vec(&value).unwrap();
    assert!(Approval::parse(&edited, &digest(&edited), &original).is_err());
}

fn write_history(installation: &Installation, phase: Phase) -> String {
    let (original, bytes, mut snapshot) = fixture();
    let approval = Approval::parse(&bytes, &digest(&bytes), &original).unwrap();
    let id = approval.digest.clone();
    snapshot["source"] = raw_destination(approval.destination()).unwrap();
    fs::write(
        installation
            .root
            .join(format!("profile-original-{}.json", digest(&original))),
        original,
    )
    .unwrap();
    fs::write(
        installation
            .root
            .join(format!("profile-approval-{id}.json")),
        bytes,
    )
    .unwrap();
    fs::write(
        installation.root.join(HISTORY),
        serde_json::to_vec(&json!({"schema":1,"records":[{
            "approval":id,"phase":phase.name(),"outcome":{"snapshot":snapshot,"native":"not-called"}
        }]}))
        .unwrap(),
    )
    .unwrap();
    id
}
fn installation(fixture: &Fixture) -> Installation {
    Installation {
        root: fixture.0.clone(),
        shortcut: fixture.0.join("settings.lnk"),
        _com: tsf::Com::new().unwrap(),
    }
}

#[test]
fn durable_history_blocks_mutation_and_rejects_modified_archives() {
    let fixture = Fixture::new();
    let mut installation = installation(&fixture);
    write_history(&installation, Phase::Attempted);
    assert!(installation.repair_gate(None).is_err());
    assert!(installation.repair_gate(Some(&"b".repeat(64))).is_err());
    let (original, _, _) = super::tests::fixture();
    fs::write(
        installation
            .root
            .join(format!("profile-original-{}.json", digest(&original))),
        b"changed",
    )
    .unwrap();
    assert!(installation.repair_history().is_err());
}

#[test]
fn retained_history_refuses_defective_package_selection_after_upgrade_and_uninstall() {
    let fixture = Fixture::new();
    let mut installation = installation(&fixture);
    write_history(&installation, Phase::Verified);
    for desired in ["b".repeat(64), "absent".into()] {
        let installed = Journal {
            desired,
            previous: Some("a".repeat(64)),
            changes: Vec::new(),
            completed: 0,
        };
        fs::write(
            installation.root.join("installed.json"),
            serde_json::to_vec(&installed.value()).unwrap(),
        )
        .unwrap();
        assert!(installation.repair_gate(Some(&"a".repeat(64))).is_err());
        installation.repair_gate(Some(&"b".repeat(64))).unwrap();
        assert!(
            installation
                .repair_packages()
                .unwrap()
                .contains(&"a".repeat(64))
        );
    }
}

#[test]
fn successor_requires_explicit_predecessor_and_cannot_reset_an_attempt() {
    let (first, _) = approval();
    let entries = vec![Entry {
        approval: first,
        phase: Phase::Stopped,
        outcome: Value::Null,
    }];
    let (original, bytes, _) = fixture();
    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        validate_successor(
            &entries,
            &Approval::parse(&bytes, &digest(&bytes), &original).unwrap()
        )
        .is_err()
    );
    value["predecessor"] = json!(entries[0].approval.digest);
    let bytes = serde_json::to_vec(&value).unwrap();
    let next = Approval::parse(&bytes, &digest(&bytes), &original).unwrap();
    validate_successor(&entries, &next).unwrap();
    let mut entries = entries;
    entries[0].phase = Phase::Verified;
    assert!(validate_successor(&entries, &next).is_err());
}

#[test]
fn read_only_inspection_does_not_create_an_installation_root() {
    let fixture = Fixture::new();
    let mut installation = installation(&fixture);
    installation.root = fixture.0.join("absent-root");
    assert!(installation.inspect_profile_repair().is_err());
    assert!(!installation.root.exists());
}

#[test]
fn archive_resumes_after_each_immutable_publication_and_never_replaces_evidence() {
    for published in 0..=2 {
        let directory = Fixture::new();
        let (original, approval, _) = fixture();
        let files = [
            (
                format!("profile-original-{}.json", digest(&original)),
                &original,
            ),
            (
                format!("profile-approval-{}.json", digest(&approval)),
                &approval,
            ),
        ];
        for (name, data) in files.iter().take(published) {
            fs::write(directory.0.join(name), data).unwrap();
        }
        archive(&directory.0, &original, &approval).unwrap();
        archive(&directory.0, &original, &approval).unwrap();
        for (name, data) in &files {
            assert_eq!(fs::read(directory.0.join(name)).unwrap(), **data);
        }
        assert!(!directory.0.join(HISTORY).exists());
        fs::write(directory.0.join(&files[0].0), b"conflicting original").unwrap();
        assert!(archive(&directory.0, &original, &approval).is_err());
        assert_eq!(
            fs::read(directory.0.join(&files[0].0)).unwrap(),
            b"conflicting original"
        );
    }
}

#[test]
fn tool_identity_requires_exact_setup_bytes_in_verified_package() {
    let fixture = Fixture::new();
    fixture.candidate();
    let arch = if cfg!(target_arch = "x86") {
        "x86"
    } else {
        "x64"
    };
    let setup = fixture.0.join(arch).join("KeyrillSetup.exe");
    let package = Package::open(&fixture.0).unwrap();
    let identity = tool_identity_at(&setup).unwrap();
    assert_eq!(identity["package"], package.digest());
    assert_eq!(
        identity["executable"],
        package
            .file_digest(&format!("{arch}/KeyrillSetup.exe"))
            .unwrap()
    );
    assert!(tool_identity_at(&fixture.0.join(arch).join("KeyrillSettings.exe")).is_err());
    fs::write(setup, b"changed executable").unwrap();
    assert!(tool_identity_at(&fixture.0.join(arch).join("KeyrillSetup.exe")).is_err());
}

#[test]
fn oversized_checkpoint_preserves_readable_attempt_history() {
    let fixture = Fixture::new();
    let mut installation = installation(&fixture);
    write_history(&installation, Phase::Attempted);
    let before = fs::read(installation.root.join(HISTORY)).unwrap();
    let entries = installation.repair_history().unwrap();
    let mut repair = Repair {
        installation: &mut installation,
        entries,
    };
    assert!(
        repair
            .checkpoint(Phase::Stopped, &json!({"snapshot":"x".repeat(LIMIT)}))
            .is_err()
    );
    assert_eq!(
        fs::read(repair.installation.root.join(HISTORY)).unwrap(),
        before
    );
    assert_eq!(
        repair.installation.repair_history().unwrap()[0].phase,
        Phase::Attempted
    );
}
