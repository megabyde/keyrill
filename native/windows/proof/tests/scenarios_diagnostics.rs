use crate::support::{client::Client, dll::*, document::Span, oracle};
use std::rc::Rc;
use windows::Win32::{Foundation::*, System::Variant::*, UI::TextServices::*};
use windows_core::{GUID, Interface};
#[test]
fn diagnostic_refusals_report_exact_stage_cause_and_result() {
    for case in 0..18 {
        let c = if case == 17 {
            Client::notepad()
        } else {
            Client::new()
        };
        let (stage, cause, result) = match case {
            0 => {
                c.a.results.borrow_mut().insert("GetAppProperty", E_FAIL);
                (17, 1, E_FAIL)
            }
            1 => {
                c.a.results.borrow_mut().insert("GetAppProperty", S_FALSE);
                (17, 1, S_FALSE)
            }
            2 => {
                c.a.nulls.borrow_mut().push("GetAppProperty");
                (17, 8, S_OK)
            }
            3 => {
                c.a.results
                    .borrow_mut()
                    .insert("scope.GetValue", E_ACCESSDENIED);
                (18, 1, E_ACCESSDENIED)
            }
            4 => {
                c.a.scope_type.set(VT_EMPTY);
                (18, 9, S_OK)
            }
            5 => {
                c.a.scope_value_present.set(false);
                (18, 8, S_OK)
            }
            6 => {
                c.a.results.borrow_mut().insert("scope.QI", E_NOINTERFACE);
                (19, 1, E_NOINTERFACE)
            }
            7 => {
                c.a.results.borrow_mut().insert("GetInputScopes", S_FALSE);
                (20, 1, S_FALSE)
            }
            8 => {
                c.a.scope_count.set(2);
                (20, 2, S_OK)
            }
            9 => {
                c.a.status.set(TF_SD_READONLY);
                (13, 2, S_OK)
            }
            10 => {
                c.a.disabled.set(true);
                (14, 2, S_OK)
            }
            11 => {
                c.a.empty_disabled.set(true);
                (15, 2, S_OK)
            }
            12 => {
                c.manager.disabled.set(true);
                (16, 2, S_OK)
            }
            13 => {
                c.a.foreign.set(true);
                (21, 2, S_OK)
            }
            14 => {
                c.a.selection.set(Span {
                    start: 7,
                    length: 1,
                });
                (9, 2, S_OK)
            }
            15 => {
                c.a.nulls.borrow_mut().push("GetActiveView");
                (10, 8, S_OK)
            }
            16 => {
                c.a.text.borrow_mut()[6] = b'x' as u16;
                (22, 2, S_OK)
            }
            17 => {
                c.host
                    .field_facts
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .field
                    .style |= 0x20;
                (12, 2, S_OK)
            }
            _ => unreachable!(),
        };
        let before = c.a.text.borrow().clone();
        let selection = c.a.selection.get();
        assert!(!c.key(b'g' as u16, false));
        let s = snapshot(&c.service);
        assert_eq!(s.first, [1, stage, cause, result.0 as u32], "case{case}");
        assert_eq!(s.first, s.latest);
        assert_eq!(s.refusals[stage as usize], 1);
        let session = u32::from(!matches!(case, 9 | 15 | 17));
        assert_eq!(s.counts[4..9], [1, session, session, session, 0]);
        assert_eq!(*c.a.text.borrow(), before);
        assert_eq!(c.a.selection.get(), selection);
        oracle::no_effects(&c);
        assert_eq!(
            *c.b.text.borrow(),
            "prefix suffix".encode_utf16().collect::<Vec<_>>()
        );
    }
}
#[test]
fn diagnostic_request_and_session_denial_are_distinct() {
    for case in 0..3 {
        let c = Client::new();
        c.a.invoke.set(false);
        if case == 0 {
            c.a.results
                .borrow_mut()
                .insert("RequestEditSession", E_ACCESSDENIED);
        }
        if case == 1 {
            c.a.results.borrow_mut().insert("session", TF_E_SYNCHRONOUS);
        }
        assert!(!c.key(b'g' as u16, false));
        let s = snapshot(&c.service);
        assert_eq!((s.request_present, s.session_present), (1, 1));
        assert_eq!(
            s.request_result,
            if case == 0 {
                E_ACCESSDENIED.0 as u32
            } else {
                0
            }
        );
        assert_eq!(
            s.session_result,
            if case == 1 {
                TF_E_SYNCHRONOUS.0 as u32
            } else {
                0
            }
        );
        assert_eq!(s.first[1], if case == 0 { 7 } else { 8 });
        assert_eq!(s.first[2], if case == 2 { 2 } else { 1 });
        assert_eq!((s.counts[5], s.counts[6], s.counts[7]), (1, 0, 0));
        oracle::no_effects(&c);
    }
}
#[test]
fn diagnostic_attempted_verified_and_unknown_are_distinct() {
    let c = Client::new();
    assert!(c.key(b'g' as u16, false));
    let s = snapshot(&c.service);
    assert_eq!(s.first[0], 0);
    assert_eq!((s.counts[8], s.counts[9]), (1, 1));
    c.a.results.borrow_mut().insert("SetText", E_FAIL);
    assert!(c.key(b'h' as u16, false));
    let s = snapshot(&c.service);
    assert_eq!(s.stopped, 1);
    assert_eq!((s.first[1], s.first[2], s.counts[11]), (24, 1, 1));
    let keys = c.service.cast::<ITfKeyEventSink>().unwrap();
    let mut eaten = true.into();
    unsafe {
        assert_eq!(
            (keys.vtable().OnTestKeyDown)(
                keys.as_raw(),
                c.context_a.as_raw(),
                WPARAM(0),
                LPARAM(0),
                &mut eaten
            ),
            S_OK
        );
    }
    assert!(!eaten.as_bool());
    let s = snapshot(&c.service);
    assert_eq!((s.latest[1], s.latest[2]), (0, 4));
}
#[test]
fn diagnostic_native_hresult_successors_preserve_effect_evidence() {
    for boundary in ["GetAppProperty", "SetText.before", "SetText.after"] {
        let c = Client::new();
        let d = Rc::clone(&c.a);
        c.host.on(boundary, 0, move || {
            d.results.borrow_mut().insert(
                if boundary == "SetText.after" {
                    "SetText"
                } else {
                    boundary
                },
                E_FAIL,
            );
        });
        let attempted = boundary != "GetAppProperty";
        let wrote = boundary == "SetText.after";
        assert_eq!(c.key(b'g' as u16, false), attempted);
        let s = snapshot(&c.service);
        assert_eq!(s.first, s.latest);
        assert_eq!(
            (s.first[1], s.first[2]),
            (if attempted { 24 } else { 17 }, 1)
        );
        assert_eq!(s.first[3], E_FAIL.0 as u32);
        assert_eq!(s.counts[8], u32::from(attempted));
        assert_eq!(s.counts[11], u32::from(attempted));
        assert_eq!(s.counts[9], 0);
        assert_eq!(
            *c.a.text.borrow(),
            if wrote {
                "prefix gsuffix"
            } else {
                "prefix suffix"
            }
            .encode_utf16()
            .collect::<Vec<_>>()
        );
        assert_eq!(
            c.a.selection.get(),
            Span {
                start: 7,
                length: 0
            }
        );
        assert_eq!(c.a.writes.get(), u32::from(wrote));
        assert!(!c.key(b'h' as u16, false));
        assert_eq!(c.a.writes.get(), u32::from(wrote));
    }
}
#[test]
fn diagnostic_copy_refuses_busy_versions_sizes_and_foreign_threads() {
    let c = Client::new();
    let reader = c.service.cast::<SnapshotReaderV2>().unwrap();
    for (size, version) in [(564, 3), (563, 2)] {
        let mut out = SnapshotV2::default();
        out.base.active = 1;
        unsafe {
            assert_eq!(reader.read(size, version, &mut out), E_INVALIDARG);
        }
        assert_eq!(out, SnapshotV2::default());
    }
    let raw = reader.as_raw() as usize;
    std::thread::spawn(move || unsafe {
        let raw = raw as *mut _;
        let borrowed = SnapshotReaderV2::from_raw_borrowed(&raw).unwrap();
        let mut out = SnapshotV2::default();
        out.base.active = 1;
        assert_eq!(borrowed.read(564, 2, &mut out), RPC_E_WRONG_THREAD);
        assert_eq!(out, SnapshotV2::default());
    })
    .join()
    .unwrap();
    let inside = reader.clone();
    c.host.on("GetSelection", 0, move || {
        let mut out = SnapshotV2::default();
        out.base.active = 1;
        unsafe {
            assert_eq!(
                inside.read(564, 2, &mut out),
                windows_core::HRESULT(0x8000000Au32 as i32)
            );
        }
        assert_eq!(out, SnapshotV2::default());
    });
    assert!(c.key(b'g' as u16, false));
    oracle::document(&c, "g", 8);
}
#[test]
fn diagnostic_lifetime_counters_survive_activation_but_not_other_instances() {
    let c = Client::new();
    let reader = c.service.cast::<SnapshotReaderV2>().unwrap();
    let keys = c.service.cast::<ITfKeyEventSink>().unwrap();
    let initial = snapshot(&c.service);
    let mut eaten = false.into();
    unsafe {
        assert_eq!(
            (keys.vtable().OnTestKeyDown)(
                keys.as_raw(),
                c.context_a.as_raw(),
                WPARAM(0),
                LPARAM(0),
                &mut eaten
            ),
            S_OK
        );
    }
    assert!(eaten.as_bool());
    assert_eq!(c.deactivate(), S_OK);
    let mut inactive = SnapshotV2::default();
    unsafe {
        assert_eq!(reader.read(564, 2, &mut inactive), S_OK);
    }
    assert_eq!(inactive.base.active, 0);
    assert_eq!(inactive.base.counts[0], 1);
    assert_eq!(c.reactivate(), S_OK);
    let active = snapshot(&c.service);
    assert_eq!(active.activation, initial.activation + 1);
    assert_eq!(active.counts[0], 1);
    let other = Client::new();
    assert_eq!(snapshot(&other.service).counts[0], 0);
    assert_eq!(other.deactivate(), S_OK);
    let other_keys = other.service.cast::<ITfKeyEventSink>().unwrap();
    unsafe {
        assert_eq!(
            (other_keys.vtable().OnTestKeyDown)(
                other_keys.as_raw(),
                std::ptr::null_mut(),
                WPARAM(0),
                LPARAM(0),
                &mut eaten
            ),
            S_OK
        );
    }
    assert!(!eaten.as_bool());
    assert_eq!(snapshot(&other.service).first[2], 3);
    let profile = c
        .service
        .cast::<ITfInputProcessorProfileActivationSink>()
        .unwrap();
    unsafe {
        assert_eq!(
            (profile.vtable().OnActivated)(
                profile.as_raw(),
                0,
                0,
                &GUID::zeroed(),
                &GUID::zeroed(),
                &GUID::zeroed(),
                Default::default(),
                0
            ),
            S_OK
        );
        assert_eq!(
            (keys.vtable().OnTestKeyDown)(
                keys.as_raw(),
                c.context_a.as_raw(),
                WPARAM(0),
                LPARAM(0),
                &mut eaten
            ),
            S_OK
        );
    }
    assert!(!eaten.as_bool());
    assert_eq!(snapshot(&c.service).latest[2], 5);
}

#[test]
fn optional_diagnostic_registration_drains_each_captured_ownership_once() {
    for case in 0..9 {
        let c = Client::new();
        assert_eq!(c.deactivate(), S_OK);
        let advises = c.manager.diagnostic_advises.get();
        let unadvises = c.manager.diagnostic_unadvises.get();
        if case == 0 {
            c.manager
                .results
                .borrow_mut()
                .insert("diagnostic.QI", E_NOINTERFACE);
        }
        if [1, 2, 6, 7].contains(&case) {
            c.manager.results.borrow_mut().insert(
                "AdviseSingleSink",
                if case == 2 || case == 7 {
                    S_FALSE
                } else {
                    E_FAIL
                },
            );
        }
        if [4, 5, 6, 7].contains(&case) {
            c.manager.results.borrow_mut().insert(
                "UnadviseSingleSink",
                if case == 5 || case == 7 {
                    S_FALSE
                } else {
                    E_FAIL
                },
            );
        }
        if case == 3 || case == 8 {
            let service = c.service.clone();
            c.host.on(
                if case == 3 {
                    "AdviseSingleSink"
                } else {
                    "UnadviseSingleSink"
                },
                0,
                move || unsafe {
                    assert_eq!((service.vtable().base__.Deactivate)(service.as_raw()), S_OK);
                },
            );
        }
        let activation = c.reactivate();
        if case == 3 {
            assert!(activation.is_err());
        } else {
            assert_eq!(activation, S_OK);
        }
        assert_eq!(c.deactivate(), S_OK);
        assert_eq!(c.deactivate(), S_OK);
        assert!(c.manager.diagnostic.borrow().is_none(), "case {case}");
        assert_eq!(
            c.manager.diagnostic_advises.get(),
            advises + u32::from(case != 0)
        );
        assert_eq!(
            c.manager.diagnostic_unadvises.get(),
            unadvises + u32::from(case != 0)
        );
        oracle::no_effects(&c);
    }
}
