use crate::support::{
    client::Client,
    dll::{Api, Snapshot, SnapshotReader, SnapshotReaderV2, SnapshotV2},
    oracle,
    providers::{Diagnostic, Kind, Provider},
};
use keyrill_windows_proof::{
    diagnostics::ServiceSnapshot,
    shutdown::{Probe, Verdict, evaluate},
};
use std::{cell::Cell, rc::Rc};
use windows::Win32::{Foundation::*, UI::TextServices::*};
use windows_core::{HRESULT, IUnknown, Interface};

struct History {
    before: ServiceSnapshot,
    events: Vec<[u32; 5]>,
}
impl History {
    fn new() -> Self {
        let mut before = ServiceSnapshot::default();
        before.base[..4].copy_from_slice(&[3, 1, 0, 1]);
        before.base[18] = 1;
        before.base[28] = 1;
        before.base[47..51].copy_from_slice(&[1, 7, 1, E_ACCESSDENIED.0 as u32]);
        before.base[51..55].copy_from_slice(&[1, 7, 1, E_ACCESSDENIED.0 as u32]);
        let mut result = Self {
            before,
            events: vec![[1, 1, 1, 0, 0], [2, 4, 1, 0, 0], [3, 1, 2, 0, 0]],
        };
        result.capture();
        result
    }
    fn record(&mut self, kind: u32, subject: u32, cause: u32) {
        self.events
            .push([self.events.len() as u32 + 1, kind, subject, cause, 0]);
    }
    fn snapshot(&self, active: bool) -> ServiceSnapshot {
        let mut value = self.before.clone();
        value.base[1] = u32::from(active);
        value.lifecycle.fill(0);
        let retained = &self.events[self.events.len().saturating_sub(16)..];
        value.lifecycle[..6].copy_from_slice(&[
            self.events.len() as u32,
            2,
            0,
            retained.len() as u32,
            u32::from(active),
            if active { 2 } else { 0 },
        ]);
        for (slot, event) in value.lifecycle[6..]
            .as_chunks_mut::<5>()
            .0
            .iter_mut()
            .zip(retained)
        {
            slot.copy_from_slice(event);
        }
        value
    }
    fn capture(&mut self) {
        self.before = self.snapshot(true);
    }
    fn deactivate(&mut self) {
        self.record(8, 0, 8);
    }
}

#[test]
fn shutdown_predicate_exact_rejection_matrix_and_exit_codes() {
    assert_eq!(Verdict::Verified as i32, 0);
    let verdicts = [
        Verdict::NotCaptured,
        Verdict::Missing,
        Verdict::Busy,
        Verdict::InvalidEvidence,
        Verdict::ProviderChanged,
        Verdict::ActivationChanged,
        Verdict::NoOwnedMark,
        Verdict::HistoryLost,
        Verdict::UnknownEffect,
        Verdict::NoTerminalEvent,
        Verdict::Detached,
        Verdict::DeactivationFailed,
    ];
    let mut codes = std::collections::BTreeSet::new();
    for verdict in verdicts {
        assert!((3..32).contains(&(verdict as i32)));
        assert!(codes.insert(verdict as i32));
    }
    let mut history = History::new();
    history.record(4, 2, 0);
    history.deactivate();
    let after = history.snapshot(false);
    assert_eq!(evaluate(&history.before, &after), Verdict::Verified);
    for scenario in 0..25 {
        let mut a = history.before.clone();
        let mut b = after.clone();
        match scenario {
            0 => a.base[1] = 0,
            1 => a.base[2] = 1,
            2 => a.lifecycle[4..6].fill(0),
            3 => a.lifecycle[5] = 0,
            4 => b.base[1] = 1,
            5 => b.lifecycle[4..6].copy_from_slice(&[1, 2]),
            6 => b.base[0] += 1,
            7 => b.base[20] = 1,
            8 => a.base[20] = 1,
            9 => b.base[18] = 0,
            10 => b.base[21..55].fill(0),
            11 => b.lifecycle[1] += 1,
            12 => b.lifecycle[22] = 1,
            13 => b.lifecycle[22] = 9,
            14 => {
                b.lifecycle[22] = 7;
                b.lifecycle[25] = 2;
            }
            15 => b.lifecycle[23] = 1,
            16 => b.lifecycle[22] = 2,
            17 => b.lifecycle[22] = 3,
            18 => b.lifecycle[22] = 5,
            19 => {
                b.lifecycle[22] = 6;
                b.lifecycle[23] = 0;
            }
            20 => b.lifecycle[29] = 5,
            21 => b.lifecycle[7] = 2,
            22 => b.lifecycle[2] = 1,
            23 => a.base[4] = 1,
            24 => b.lifecycle[26] -= 1,
            _ => unreachable!(),
        }
        assert_ne!(evaluate(&a, &b), Verdict::Verified, "scenario {scenario}");
    }
    for serial in [false, true] {
        let mut saturated = history.before.clone();
        if serial {
            saturated.lifecycle[1] = u32::MAX;
            saturated.lifecycle[5] = u32::MAX;
            saturated.lifecycle[2] = 1;
        } else {
            saturated.base[0] = u32::MAX;
            saturated.base[4] = 1;
        }
        assert_ne!(evaluate(&saturated, &after), Verdict::Verified);
    }
}

#[test]
fn shutdown_predicate_terminal_order_stale_capture_and_capacity() {
    for kind in [4, 3] {
        let mut history = History::new();
        history.deactivate();
        history.record(2, 2, 0);
        history.record(kind, 2, 0);
        assert_ne!(
            evaluate(&history.before, &history.snapshot(false)),
            Verdict::Verified
        );
    }
    let mut finish = History::new();
    finish.record(2, 2, 0);
    finish.record(3, 2, 0);
    finish.deactivate();
    let mut stopped = finish.snapshot(false);
    stopped.base[2] = 1;
    assert_eq!(evaluate(&finish.before, &stopped), Verdict::Verified);
    finish.record(9, 2, 0);
    assert_eq!(
        evaluate(&finish.before, &finish.snapshot(false)),
        Verdict::Detached
    );
    for kind in [5, 6] {
        let mut history = History::new();
        history.record(kind, if kind == 6 { 0 } else { 2 }, 0);
        history.record(4, 2, 0);
        history.deactivate();
        assert_eq!(
            evaluate(&history.before, &history.snapshot(false)),
            Verdict::Verified
        );
    }
    for kind in [2, 4] {
        let mut history = History::new();
        history.record(kind, 2, 0);
        history.capture();
        if kind == 2 {
            history.record(3, 2, 0);
        }
        history.deactivate();
        assert_ne!(
            evaluate(&history.before, &history.snapshot(false)),
            Verdict::Verified
        );
        let mut backwards = History::new().snapshot(false);
        backwards.base = history.snapshot(false).base;
        assert_ne!(evaluate(&history.before, &backwards), Verdict::Verified);
    }
    for count in [16, 17] {
        let mut history = History::new();
        for _ in 0..16 {
            history.record(5, 2, 0);
        }
        history.capture();
        history.record(4, 2, 0);
        for _ in 2..count {
            history.record(6, 0, 0);
        }
        history.deactivate();
        assert_eq!(
            evaluate(&history.before, &history.snapshot(false)) == Verdict::Verified,
            count == 16
        );
    }
}

fn publish(provider: &Diagnostic, snapshot: &ServiceSnapshot) {
    let mut words = provider.snapshot.borrow_mut();
    words[..55].copy_from_slice(&snapshot.base);
    words[55..].copy_from_slice(&snapshot.lifecycle);
}
fn diagnostic(c: &Client, snapshot: &ServiceSnapshot) -> (Rc<Diagnostic>, IUnknown) {
    let state = Rc::new(Diagnostic::default());
    publish(&state, snapshot);
    // SAFETY: the provider allocates one owned COM reference with a stable SDK slot.
    let interface = unsafe {
        IUnknown::from_raw(Provider::create(
            Rc::clone(&c.host),
            Kind::Diagnostic(Rc::clone(&state)),
            ITfFunctionProvider::IID,
        ))
    };
    (state, interface)
}
fn install(c: &Client, provider: &IUnknown) {
    let replacement = provider.clone();
    let retired = c.manager.diagnostic.replace(Some(replacement));
    drop(retired);
}

#[test]
fn shutdown_probe_capture_finish_and_provider_failures() {
    let mut history = History::new();
    history.record(4, 2, 0);
    history.deactivate();
    for scenario in 0..21 {
        let c = Client::new();
        let (state, provider) = diagnostic(&c, &history.before);
        let (_other_state, other) = diagnostic(&c, &history.before);
        install(&c, &provider);
        let probe = Probe::default();
        match scenario {
            1 => probe.capture_close(&c.thread),
            2 => state.legacy_only.set(true),
            3 => {
                state.foreign_function.replace(Some(other.clone()));
            }
            4 => {
                c.manager
                    .results
                    .borrow_mut()
                    .insert("GetFunctionProvider", TF_E_NOPROVIDER);
            }
            _ => {}
        }
        probe.observe(&c.thread);
        c.manager.results.borrow_mut().remove("GetFunctionProvider");
        if scenario == 4 {
            probe.observe(&c.thread);
        }
        match scenario {
            5 => install(&c, &other),
            6 => state.snapshot.borrow_mut()[0] += 1,
            7 => state
                .result
                .set(keyrill_windows_proof::diagnostics::E_PENDING),
            8 => state.result.set(RPC_E_WRONG_THREAD),
            9 => state.result.set(E_FAIL),
            10 => state.snapshot.borrow_mut()[59..61].fill(0),
            11 => {
                c.manager
                    .results
                    .borrow_mut()
                    .insert("GetFunctionProvider", TF_E_NOPROVIDER);
            }
            12 => {
                install(&c, &other);
                probe.observe(&c.thread);
                install(&c, &provider);
            }
            13 => {
                state.snapshot.borrow_mut()[0] += 1;
                probe.observe(&c.thread);
                state.snapshot.borrow_mut()[0] -= 1;
            }
            14 => {
                state
                    .result
                    .set(keyrill_windows_proof::diagnostics::E_PENDING);
                probe.observe(&c.thread);
                state.result.set(S_OK);
            }
            _ => {}
        }
        if scenario != 15 {
            probe.capture_close(&c.thread);
        }
        if scenario == 16 {
            probe.capture_close(&c.thread);
        }
        let queries = c
            .host
            .trace
            .borrow()
            .iter()
            .filter(|n| **n == "GetFunctionProvider")
            .count();
        c.manager
            .results
            .borrow_mut()
            .insert("GetFunctionProvider", E_FAIL);
        publish(&state, &history.snapshot(false));
        state.result.set(match scenario {
            17 => keyrill_windows_proof::diagnostics::E_PENDING,
            18 => RPC_E_WRONG_THREAD,
            _ => S_OK,
        });
        if scenario == 19 {
            state.snapshot.borrow_mut()[1] = 1;
        }
        let verdict = probe.finish(if scenario == 20 { E_FAIL } else { S_OK });
        if scenario == 17 {
            assert_eq!(verdict, Verdict::Busy);
        }
        assert_eq!(
            verdict == Verdict::Verified,
            scenario == 0 || scenario == 4,
            "scenario {scenario}: {verdict:?}"
        );
        let reads = state.reads.get();
        assert_eq!(probe.finish(S_OK), verdict);
        assert_eq!(state.reads.get(), reads);
        probe.observe(&c.thread);
        probe.capture_close(&c.thread);
        assert_eq!(probe.finish(S_OK), verdict);
        assert_eq!(
            c.host
                .trace
                .borrow()
                .iter()
                .filter(|n| **n == "GetFunctionProvider")
                .count(),
            queries
        );
        let retired = c.manager.diagnostic.take();
        drop(retired);
        let foreign = state.foreign_function.take();
        drop(foreign);
        let before = c.host.live_objects.get();
        drop(provider);
        drop(other);
        assert_eq!(
            c.host.live_objects.get(),
            before - 2,
            "retained endpoints drain at Finish"
        );
        oracle::document(&c, "", 7);
        oracle::no_effects(&c);
        c.host.drained();
    }
}

#[test]
fn shutdown_probe_reentry_at_all_native_phases() {
    let mut history = History::new();
    history.record(4, 2, 0);
    history.deactivate();
    for boundary in [
        "GetFunctionProvider",
        "identity",
        "interface",
        "function",
        "copy",
        "addref",
        "release",
    ] {
        for phase in 0..4 {
            let c = Rc::new(Client::new());
            let (state, provider) = diagnostic(&c, &history.before);
            install(&c, &provider);
            let probe = Rc::new(Probe::default());
            if phase != 0 {
                probe.observe(&c.thread);
            }
            if phase == 3 {
                probe.capture_close(&c.thread);
            }
            let nested = Rc::new(Cell::new(false));
            let hit = Rc::clone(&nested);
            let proof = Rc::clone(&probe);
            let weak = Rc::downgrade(&c);
            *c.host.observer.borrow_mut() = Some(Rc::new(move |name| {
                if name == boundary && !hit.replace(true) {
                    let c = weak.upgrade().unwrap();
                    proof.capture_close(&c.thread);
                    proof.observe(&c.thread);
                }
            }));
            if phase <= 1 {
                probe.observe(&c.thread);
            }
            if phase == 2 {
                probe.capture_close(&c.thread);
            }
            if phase == 3 {
                publish(&state, &history.snapshot(false));
            }
            let verdict = probe.finish(S_OK);
            if nested.get() {
                assert_ne!(verdict, Verdict::Verified, "{boundary}/{phase}");
            }
            c.host.observer.borrow_mut().take();
            oracle::document(&c, "", 7);
            oracle::no_effects(&c);
            c.host.drained();
        }
    }
    for (boundary, skip) in [("identity", 1), ("interface", 1), ("interface", 2)] {
        let c = Rc::new(Client::new());
        let (state, provider) = diagnostic(&c, &history.before);
        install(&c, &provider);
        let probe = Rc::new(Probe::default());
        let proof = Rc::clone(&probe);
        let weak = Rc::downgrade(&c);
        c.host.on(boundary, skip, move || {
            proof.capture_close(&weak.upgrade().unwrap().thread)
        });
        probe.observe(&c.thread);
        probe.capture_close(&c.thread);
        publish(&state, &history.snapshot(false));
        assert_ne!(probe.finish(S_OK), Verdict::Verified, "{boundary}/{skip}");
        c.host.drained();
    }
    let c = Rc::new(Client::new());
    let (state, provider) = diagnostic(&c, &history.before);
    install(&c, &provider);
    let probe = Rc::new(Probe::default());
    probe.observe(&c.thread);
    let proof = Rc::clone(&probe);
    let weak = Rc::downgrade(&c);
    *c.host.observer.borrow_mut() = Some(Rc::new(move |_| {
        proof.observe(&weak.upgrade().unwrap().thread)
    }));
    probe.capture_close(&c.thread);
    publish(&state, &history.snapshot(false));
    assert_eq!(probe.finish(S_OK), Verdict::Verified);
    c.host.observer.borrow_mut().take();
    c.host.drained();
}

#[test]
fn shutdown_reader_lifetime_both_release_orders() {
    let api = Api::load();
    // SAFETY: this versioned test export has the declared two-output ABI.
    let counts: unsafe extern "system" fn(*mut i32, *mut i32) -> HRESULT =
        unsafe { api.export(b"KeyrillTestCounts\0") };
    let count = || {
        let (mut objects, mut workers) = (0, 0);
        assert_eq!(unsafe { counts(&mut objects, &mut workers) }, S_OK);
        objects
    };
    for old_first in [false, true] {
        let initial = count();
        let (old, current, host) = {
            let c = Client::new();
            c.type_text("g");
            let old = c.service.cast::<SnapshotReader>().unwrap();
            let current = c.service.cast::<SnapshotReaderV2>().unwrap();
            let mut active = SnapshotV2::default();
            let mut legacy = Snapshot::default();
            unsafe {
                assert_eq!(old.read(220, 1, &mut legacy), S_OK);
                assert_eq!(current.read(564, 2, &mut active), S_OK);
            }
            assert_eq!(legacy, active.base);
            let composition = c.a.active.borrow().as_ref().unwrap().pointer;
            let sink = c.service.cast::<ITfCompositionSink>().unwrap();
            c.a.active.borrow_mut().take();
            unsafe {
                assert_eq!(
                    (sink.vtable().OnCompositionTerminated)(sink.as_raw(), 123, composition),
                    S_OK
                );
            }
            oracle::document(&c, "g", 8);
            (old, current, Rc::clone(&c.host))
        };
        assert_eq!(count(), initial + 1);
        #[repr(C)]
        struct Guarded {
            before: u32,
            value: Snapshot,
            after: u32,
        }
        let mut legacy = Guarded {
            before: 17,
            value: Snapshot::default(),
            after: 23,
        };
        let mut inactive = SnapshotV2::default();
        unsafe {
            assert_eq!(old.read(220, 1, &mut legacy.value), S_OK);
            assert_eq!(current.read(564, 2, &mut inactive), S_OK);
        }
        assert_eq!(inactive.base.active, 0);
        assert_eq!(inactive.lifecycle[4], 0);
        // SAFETY: Snapshot is the frozen repr(C) payload of exactly 55 DWORDs.
        let base: [u32; 55] = unsafe { std::mem::transmute(inactive.base) };
        assert!(
            ServiceSnapshot {
                base,
                lifecycle: inactive.lifecycle
            }
            .valid()
        );
        assert_eq!(legacy.value, inactive.base);
        assert_eq!((legacy.before, legacy.after), (17, 23));
        if old_first {
            drop(old);
            assert_eq!(count(), initial + 1);
            unsafe {
                assert_eq!(current.read(564, 2, &mut inactive), S_OK);
            }
            drop(current);
        } else {
            drop(current);
            assert_eq!(count(), initial + 1);
            unsafe {
                assert_eq!(old.read(220, 1, &mut legacy.value), S_OK);
            }
            drop(old);
        }
        assert_eq!(count(), initial);
        host.drained();
        assert_eq!(host.live_objects.get(), 0);
        assert_eq!(
            host.releases.get(),
            host.adds.get() + host.final_releases.get()
        );
    }
}
