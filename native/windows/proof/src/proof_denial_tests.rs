use super::*;
use crate::options::FixtureMode;

fn session_denial() -> Publication {
    let mut publication = Publication {
        publication: 10,
        source: 2,
        availability: 4,
        continuity: 2,
        ..Default::default()
    };
    let s = &mut publication.snapshot.base;
    s[0..9].copy_from_slice(&[3, 1, 0, 1, 0, 1, 0, 1, 0x80040208]);
    s[14] = 8;
    s[29] = 1;
    s[47..55].copy_from_slice(&[1, 8, 1, 0x80040208, 1, 8, 1, 0x80040208]);
    assert!(publication.valid());
    publication
}

fn next(first: &Publication) -> Publication {
    let mut second = first.clone();
    second.publication += 1;
    second
}

#[test]
fn native_denial_startup_modes_and_exact_setup_confirmation() {
    for (arguments, expected) in [
        (vec![], Some(FixtureMode::Normal)),
        (vec!["--deny-tsf-locks"], Some(FixtureMode::DenyTsfLocks)),
        (
            vec!["--prove-owned-shutdown"],
            Some(FixtureMode::ProveOwnedShutdown),
        ),
        (
            vec!["--prove-owned-en-to-ru"],
            Some(FixtureMode::ProveOwnedEnToRu),
        ),
        (vec!["--unknown"], None),
        (vec!["--deny-tsf-locks", "--deny-tsf-locks"], None),
        (vec!["--deny-tsf-locks", "extra"], None),
        (vec!["--deny-tsf-locks", "--prove-owned-shutdown"], None),
        (
            vec!["--prove-owned-shutdown", "--prove-owned-shutdown"],
            None,
        ),
        (vec!["--prove-owned-en-to-ru", "ru"], None),
        (
            vec!["--prove-owned-en-to-ru", "--prove-owned-en-to-ru"],
            None,
        ),
        (
            vec!["--prove-owned-en-to-ru", "--prove-owned-shutdown"],
            None,
        ),
        (vec!["--prove-owned-en-to-ru", "--deny-tsf-locks"], None),
    ] {
        let arguments: Vec<String> = arguments.into_iter().map(str::to_owned).collect();
        assert_eq!(FixtureMode::parse(&arguments), expected, "{arguments:?}");
    }
    assert!(denial_setup_confirmed(Some(
        "Keyrill native denial v1; setup=Confirmed"
    )));
    for status in [
        None,
        Some(""),
        Some("Keyrill native denial v2; setup=Confirmed"),
    ] {
        assert!(!denial_setup_confirmed(status), "{status:?}");
    }
}

#[test]
fn native_denial_accepts_failed_request_and_failed_session_evidence() {
    let first = session_denial();
    assert!(denial_matches(&first, &next(&first)));
    let mut request = first;
    let s = &mut request.snapshot.base;
    s[6..9].copy_from_slice(&[0x80070005, 0, 0]);
    s[29] = 0;
    s[28] = 1;
    s[47..55].copy_from_slice(&[1, 7, 1, 0x80070005, 1, 7, 1, 0x80070005]);
    assert!(request.valid());
    assert!(denial_matches(&request, &next(&request)));
}

#[test]
fn native_denial_rejects_all_six_effects_in_either_sample() {
    let first = session_denial();
    let second = next(&first);
    for (name, index) in [
        ("SessionEntered", 15),
        ("SessionCompleted", 16),
        ("Attempted", 17),
        ("Verified", 18),
        ("Unknown", 20),
        ("Consumed", 12),
    ] {
        let mut dirty = second.clone();
        dirty.snapshot.base[index] = 1;
        assert!(dirty.valid());
        assert!(!denial_matches(&first, &dirty), "later {name}");
        let mut dirty = first.clone();
        dirty.snapshot.base[index] = 1;
        assert!(dirty.valid());
        assert!(!denial_matches(&dirty, &second), "earlier {name}");
    }
}

#[test]
fn native_denial_rejects_identity_freshness_mode_and_native_result_drift() {
    let first = session_denial();
    let second = next(&first);
    assert!(!denial_matches(&first, &first));
    let mut older = first.clone();
    older.publication = 9;
    assert!(!denial_matches(&first, &older));
    for mode in [0, 2] {
        let mut changed = second.clone();
        changed.snapshot.base[3] = mode;
        assert!(changed.valid());
        assert!(!denial_matches(&first, &changed), "mode {mode}");
    }
    for (name, index, value) in [
        ("no requests", 14, 0),
        ("successful session", 8, 0),
        ("non-S_OK request success", 6, 1),
        ("unmatched native refusal", 54, 0x80004005),
        ("stopped", 2, 1),
        ("activation", 0, 4),
    ] {
        let mut changed = second.clone();
        changed.snapshot.base[index] = value;
        assert!(changed.valid(), "{name}");
        assert!(!denial_matches(&first, &changed), "{name}");
    }
    let mut changed = second.clone();
    changed.snapshot.base[7..9].fill(0);
    assert!(changed.valid());
    assert!(!denial_matches(&first, &changed));
    let mut changed = second.clone();
    changed.snapshot.base[53..55].copy_from_slice(&[2, 0]);
    assert!(changed.valid());
    assert!(!denial_matches(&first, &changed));
    let mut changed = second.clone();
    changed.source += 1;
    assert!(!denial_matches(&first, &changed));
    let mut changed = second;
    changed.continuity = 1;
    assert!(!denial_matches(&first, &changed));
}

#[test]
fn native_denial_rejects_absent_busy_and_saturated_publications() {
    let first = session_denial();
    for availability in [0, 3] {
        let changed = Publication {
            publication: 11,
            source: 2,
            availability,
            ..Default::default()
        };
        assert!(changed.valid());
        assert!(
            !denial_matches(&first, &changed),
            "availability {availability}"
        );
    }
    let mut changed = next(&first);
    changed.saturated = 1;
    changed.snapshot.base[4] = 1;
    changed.continuity = 3;
    assert!(changed.valid());
    assert!(!denial_matches(&first, &changed));
}
