use crate::support::{
    client::Client,
    com_slots,
    dll::*,
    oracle,
    providers::{Diagnostic, Kind, Provider},
};
use keyrill_windows_proof::{
    diagnostics::{Poller, ServiceSnapshot},
    shutdown::{Probe, Verdict},
};
use std::rc::Rc;
use windows::Win32::{Foundation::*, UI::TextServices::*};
use windows_core::{IUnknown, Interface};

#[test]
fn diagnostic_publication_replaces_failed_and_changed_provider_evidence() {
    let c = Client::new();
    let poller = Poller::default();
    poller.poll(&c.thread);
    assert_eq!(
        (
            poller.value().availability,
            poller.value().continuity,
            poller.value().source
        ),
        (4, 1, 1)
    );
    let diagnostic = Rc::new(Diagnostic::default());
    diagnostic.snapshot.borrow_mut()[..2].copy_from_slice(&[1, 1]);
    let provider = unsafe {
        IUnknown::from_raw(Provider::create(
            Rc::clone(&c.host),
            Kind::Diagnostic(Rc::clone(&diagnostic)),
            ITfFunctionProvider::IID,
        ))
    };
    let old = c.manager.diagnostic.replace(Some(provider));
    drop(old);
    poller.poll(&c.thread);
    assert_eq!(
        (
            poller.value().availability,
            poller.value().continuity,
            poller.value().source
        ),
        (4, 1, 2)
    );
    for (result, active) in [(E_ACCESSDENIED, 1), (S_FALSE, 1), (E_FAIL, 1), (S_OK, 2)] {
        diagnostic.result.set(result);
        diagnostic.snapshot.borrow_mut()[1] = active;
        poller.poll(&c.thread);
        assert_eq!(
            (poller.value().availability, poller.value().continuity),
            (1, 0)
        );
        assert_eq!(poller.value().snapshot, ServiceSnapshot::default());
    }
    diagnostic.result.set(S_OK);
    diagnostic.snapshot.borrow_mut()[1] = 1;
    diagnostic.legacy_only.set(true);
    poller.poll(&c.thread);
    assert_eq!(poller.value().availability, 5);
    assert_eq!(poller.value().snapshot, ServiceSnapshot::default());
    assert!(poller.value().valid());
    assert_eq!(diagnostic.legacy_reads.get(), 0);
    diagnostic.legacy_only.set(false);
    diagnostic.snapshot.borrow_mut()[1] = 0;
    poller.poll(&c.thread);
    assert_eq!(
        (poller.value().availability, poller.value().snapshot.base[1]),
        (2, 0)
    );
    diagnostic.snapshot.borrow_mut()[1] = 1;
    poller.poll(&c.thread);
    assert_eq!(poller.value().continuity, 1);
    poller.poll(&c.thread);
    assert_eq!(poller.value().continuity, 2);
    diagnostic.snapshot.borrow_mut()[0] += 1;
    poller.poll(&c.thread);
    assert_eq!(poller.value().continuity, 1);
    c.manager
        .results
        .borrow_mut()
        .insert("GetFunctionProvider", E_FAIL);
    poller.poll(&c.thread);
    assert_eq!(
        (poller.value().availability, poller.value().result),
        (1, E_FAIL.0 as u32)
    );
    assert_eq!(poller.value().snapshot, ServiceSnapshot::default());
    c.manager.results.borrow_mut().clear();
    c.manager.nulls.borrow_mut().push("GetFunctionProvider");
    poller.poll(&c.thread);
    assert_eq!((poller.value().availability, poller.value().result), (1, 0));
    assert_eq!(poller.value().snapshot, ServiceSnapshot::default());
    c.manager.nulls.borrow_mut().clear();
    poller.set_publication_number_for_test(u32::MAX - 1);
    poller.poll(&c.thread);
    assert_eq!(
        (
            poller.value().publication,
            poller.value().saturated,
            poller.value().continuity
        ),
        (u32::MAX, 1, 3)
    );
    oracle::no_effects(&c);
}

#[test]
fn diagnostic_registered_service_busy_and_activation_intervals_are_explicit() {
    let c = Client::new();
    assert_eq!(c.manager.diagnostic_advises.get(), 1);
    let poller = Rc::new(Poller::default());
    poller.poll(&c.thread);
    let activation = poller.value().snapshot.base[0];
    let keys = c.service.cast::<ITfKeyEventSink>().unwrap();
    unsafe {
        assert_eq!(
            (keys.vtable().OnSetFocus)(keys.as_raw(), false.into()),
            S_OK
        );
    }
    poller.poll(&c.thread);
    assert_eq!((poller.value().continuity, poller.value().source), (2, 1));
    let reader = c.service.cast::<SnapshotReaderV2>().unwrap();
    let inside = Rc::clone(&poller);
    let thread = c.thread.clone();
    let retained = reader.clone();
    c.host.on("GetSelection", 0, move || {
        let mut output = SnapshotV2::default();
        output.base.active = 1;
        unsafe {
            assert_eq!(
                retained.read(564, 2, &mut output),
                keyrill_windows_proof::diagnostics::E_PENDING
            );
        }
        assert_eq!(output, SnapshotV2::default());
        inside.poll(&thread);
        assert_eq!(inside.value().availability, 3);
        assert_eq!(inside.value().snapshot, ServiceSnapshot::default());
    });
    assert!(c.key(b'g' as u16, false));
    oracle::document(&c, "g", 8);
    poller.poll(&c.thread);
    assert_eq!((poller.value().continuity, poller.value().source), (1, 2));
    c.manager
        .results
        .borrow_mut()
        .insert("GetFunctionProvider", TF_E_NOPROVIDER);
    poller.poll(&c.thread);
    assert_eq!(poller.value().availability, 0);
    assert_eq!(poller.value().snapshot, ServiceSnapshot::default());
    c.manager.results.borrow_mut().clear();
    assert_eq!(c.deactivate(), S_OK);
    let mut inactive = SnapshotV2::default();
    unsafe {
        assert_eq!(reader.read(564, 2, &mut inactive), S_OK);
    }
    assert_eq!(inactive.base.active, 0);
    assert_eq!(c.reactivate(), S_OK);
    poller.poll(&c.thread);
    assert_eq!(
        (
            poller.value().availability,
            poller.value().continuity,
            poller.value().snapshot.base[0]
        ),
        (4, 1, activation + 1)
    );
}

#[test]
fn shutdown_service_requires_exact_terminal_evidence_before_deactivation() {
    for case in 0..4 {
        let c = Client::new();
        c.type_text("ghbdtn");
        let probe = Probe::default();
        let effects = (
            c.a.reads.get(),
            c.a.writes.get(),
            c.a.starts.get(),
            c.a.ends.get(),
        );
        probe.observe(&c.thread);
        probe.capture_close(&c.thread);
        assert_eq!(
            (
                c.a.reads.get(),
                c.a.writes.get(),
                c.a.starts.get(),
                c.a.ends.get()
            ),
            effects
        );
        let queries = c
            .host
            .trace
            .borrow()
            .iter()
            .filter(|n| **n == "GetFunctionProvider")
            .count();
        if case < 2 {
            if case == 1 {
                let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
                unsafe {
                    assert_eq!(
                        (sink.vtable().OnSetFocus)(sink.as_raw(), false.into()),
                        S_OK
                    );
                }
            }
            let pointer = c.a.active.borrow().as_ref().unwrap().pointer;
            let composition = unsafe { IUnknown::from_raw(com_slots::retain(pointer)) };
            c.a.active.borrow_mut().take();
            let sink = c.service.cast::<ITfCompositionSink>().unwrap();
            unsafe {
                assert_eq!(
                    (sink.vtable().OnCompositionTerminated)(
                        sink.as_raw(),
                        123,
                        composition.as_raw()
                    ),
                    S_OK
                );
            }
        }
        if case == 2 {
            assert!(!c.event(Event {
                role: 6,
                mode: 1,
                ..Event::default()
            }));
        }
        assert_eq!(probe.finish(c.deactivate()) == Verdict::Verified, case != 3);
        assert_eq!(
            c.host
                .trace
                .borrow()
                .iter()
                .filter(|n| **n == "GetFunctionProvider")
                .count(),
            queries
        );
        assert_eq!(
            (c.a.writes.get(), c.a.starts.get(), c.a.ends.get()),
            (effects.1, effects.2, effects.3 + u32::from(case == 2))
        );
        oracle::document(&c, "ghbdtn", 13);
    }
    let c = Client::new();
    let probe = Probe::default();
    probe.observe(&c.thread);
    probe.capture_close(&c.thread);
    assert_eq!(probe.finish(c.deactivate()), Verdict::NoOwnedMark);
    oracle::no_effects(&c);
}

#[test]
fn shutdown_service_busy_capture_or_endpoint_is_permanently_rejected() {
    for endpoint in [false, true] {
        let c = Client::new();
        c.type_text("g");
        let probe = Rc::new(Probe::default());
        probe.observe(&c.thread);
        if endpoint {
            probe.capture_close(&c.thread);
        }
        let inside = Rc::clone(&probe);
        let thread = c.thread.clone();
        c.host.on("GetSelection", 0, move || {
            if endpoint {
                assert_eq!(inside.finish(S_OK), Verdict::Busy);
            } else {
                inside.capture_close(&thread);
            }
        });
        assert!(c.key(b'h' as u16, false));
        assert_eq!(probe.finish(c.deactivate()), Verdict::Busy);
        oracle::document(&c, "gh", 9);
    }
}

fn diagnostic_provider(c: &Client, activation: u32) -> (Rc<Diagnostic>, IUnknown) {
    let state = Rc::new(Diagnostic::default());
    state.snapshot.borrow_mut()[..2].copy_from_slice(&[activation, 1]);
    let provider = unsafe {
        IUnknown::from_raw(Provider::create(
            Rc::clone(&c.host),
            Kind::Diagnostic(Rc::clone(&state)),
            ITfFunctionProvider::IID,
        ))
    };
    (state, provider)
}
#[test]
fn diagnostic_poller_nested_native_callbacks_keep_the_newer_publication() {
    for boundary in [
        "GetFunctionProvider",
        "identity",
        "interface",
        "function",
        "copy",
        "addref",
        "release",
    ] {
        let c = Client::new();
        let poller = Rc::new(Poller::default());
        poller.poll(&c.thread);
        let (old_state, old) = diagnostic_provider(&c, 3);
        let (_, new) = diagnostic_provider(&c, 7);
        let retired = c.manager.diagnostic.replace(Some(old.clone()));
        drop(retired);
        let manager = Rc::clone(&c.manager);
        let thread = c.thread.clone();
        let nested = Rc::clone(&poller);
        let expected = Rc::new(std::cell::RefCell::new(None));
        let record = Rc::clone(&expected);
        c.host.on(boundary, 0, move || {
            let previous = manager.diagnostic.replace(Some(new.clone()));
            drop(previous);
            nested.poll(&thread);
            let value = nested.value();
            assert_eq!(value.snapshot.base[0], 7);
            *record.borrow_mut() = Some(value);
        });
        poller.poll(&c.thread);
        assert_eq!(
            &poller.value(),
            expected.borrow().as_ref().unwrap(),
            "boundary {boundary}"
        );
        drop(old);
        assert_eq!(
            Rc::strong_count(&old_state),
            1,
            "old native provider retired at {boundary}"
        );
        oracle::no_effects(&c);
    }
}
#[test]
fn diagnostic_poller_publishes_before_final_release_and_preserves_release_reentry() {
    let c = Client::new();
    let poller = Rc::new(Poller::default());
    let (old_state, old) = diagnostic_provider(&c, 3);
    let retired = c.manager.diagnostic.replace(Some(old));
    drop(retired);
    poller.poll(&c.thread);
    let (_, next) = diagnostic_provider(&c, 7);
    let (_, newest) = diagnostic_provider(&c, 11);
    let retired = c.manager.diagnostic.replace(Some(next));
    drop(retired);
    let manager = Rc::clone(&c.manager);
    let thread = c.thread.clone();
    let nested = Rc::clone(&poller);
    let expected = Rc::new(std::cell::RefCell::new(None));
    let record = Rc::clone(&expected);
    c.host.on("Release.final", 0, move || {
        assert_eq!(nested.value().snapshot.base[0], 7);
        let previous = manager.diagnostic.replace(Some(newest.clone()));
        drop(previous);
        nested.poll(&thread);
        let value = nested.value();
        assert_eq!(value.snapshot.base[0], 11);
        *record.borrow_mut() = Some(value);
    });
    poller.poll(&c.thread);
    assert_eq!(&poller.value(), expected.borrow().as_ref().unwrap());
    assert_eq!(Rc::strong_count(&old_state), 1);
    oracle::no_effects(&c);
}

#[test]
fn host_mapping_publication_preserves_complete_edit_and_command_snapshots() {
    use keyrill_windows_proof::{
        mapping::{ArmedPage, Availability},
        native::capture_identity,
        protocol::decode_payload,
    };
    use windows::Win32::{System::Threading::*, UI::Input::KeyboardAndMouse::*};
    use windows_core::GUID;
    struct Keyboard([u8; 256]);
    impl Keyboard {
        fn command() -> Self {
            let mut prior = [0; 256];
            assert!(crate::support::keyboard_state(&mut prior));
            let mut state = [0; 256];
            for key in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_LMENU] {
                state[key.0 as usize] = 0x80;
            }
            unsafe {
                SetKeyboardState(&state).unwrap();
            }
            Self(prior)
        }
    }
    impl Drop for Keyboard {
        fn drop(&mut self) {
            unsafe {
                SetKeyboardState(&self.0).unwrap();
            }
        }
    }
    fn preserved(c: &Client, reverse: bool) -> bool {
        let id = GUID::from_u128(if reverse {
            0x16531b70_516e_4dfb_9f18_cb760de590b4
        } else {
            0xd7abce42_c8a9_4a43_990a_4a8d6c011ebb
        });
        let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
        let mut eaten = false.into();
        unsafe {
            assert_eq!(
                (sink.vtable().OnPreservedKey)(
                    sink.as_raw(),
                    c.context_a.as_raw(),
                    &id,
                    &mut eaten
                ),
                S_OK
            );
        }
        c.host.check();
        eaten.as_bool()
    }
    fn commands(c: &Client) -> SnapshotV2 {
        let _keyboard = Keyboard::command();
        let composition = c.a.active.borrow().as_ref().unwrap().pointer;
        let writes = c.a.writes.get();
        assert!(preserved(c, false));
        oracle::document(c, "ghbdtn ", 14);
        assert!(preserved(c, true));
        oracle::document(c, "ghbdtn ", 14);
        assert_eq!(c.a.active.borrow().as_ref().unwrap().pointer, composition);
        assert_eq!(
            (c.a.writes.get(), c.a.starts.get(), c.a.ends.get()),
            (writes + 2, 1, 0)
        );
        snapshot_v2(&c.service)
    }
    fn base_words(snapshot: Snapshot) -> [u32; 55] {
        let mut words = [0; 55];
        words[..9].copy_from_slice(&[
            snapshot.activation,
            snapshot.active,
            snapshot.stopped,
            snapshot.mode,
            snapshot.saturated,
            snapshot.request_present,
            snapshot.request_result,
            snapshot.session_present,
            snapshot.session_result,
        ]);
        words[9..21].copy_from_slice(&snapshot.counts);
        words[21..47].copy_from_slice(&snapshot.refusals);
        words[47..51].copy_from_slice(&snapshot.first);
        words[51..].copy_from_slice(&snapshot.latest);
        words
    }
    let (baseline, command_baseline) = {
        let c = Client::new();
        c.type_text("ghbdtn ");
        oracle::document(&c, "привет ", 14);
        (snapshot_v2(&c.service), commands(&c))
    };
    let identity = unsafe {
        capture_identity(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            GetCurrentThreadId(),
        )
        .unwrap()
    };
    let armed = ArmedPage::create(identity).unwrap();
    {
        let c = Client::new();
        c.type_text("ghbdtn ");
        oracle::document(&c, "привет ", 14);
        let current = snapshot_v2(&c.service);
        assert_eq!(current, baseline);
        let (availability, payload) = armed.read();
        assert_eq!(availability, Availability::Available);
        let decoded = decode_payload(&payload).unwrap();
        assert_eq!(decoded.service, base_words(current.base));
        assert_eq!(decoded.lifecycle, current.lifecycle);
        assert_eq!(decoded.lifecycle[4], 1);
        assert_ne!(decoded.lifecycle[5], 0);
        let current = commands(&c);
        assert_eq!(current, command_baseline);
        let (availability, payload) = armed.read();
        assert_eq!(availability, Availability::Available);
        let decoded = decode_payload(&payload).unwrap();
        assert_eq!(decoded.service, base_words(current.base));
        assert_eq!(decoded.lifecycle, current.lifecycle);
        assert_eq!(decoded.observations[2], 6);
        let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
        unsafe {
            assert_eq!(
                (sink.vtable().OnSetFocus)(sink.as_raw(), false.into()),
                S_OK
            );
        }
        let (availability, payload) = armed.read();
        assert_eq!(availability, Availability::Available);
        assert_eq!(decode_payload(&payload).unwrap().metadata[0], 0);
        let _keyboard = Keyboard::command();
        preserved(&c, false);
        let (availability, payload) = armed.read();
        assert_eq!(availability, Availability::Available);
        assert!(decode_payload(&payload).unwrap().observations[2] >= 2);
    }
    assert_eq!(armed.read().0, Availability::Missing);
}
