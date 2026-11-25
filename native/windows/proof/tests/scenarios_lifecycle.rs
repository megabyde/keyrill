use crate::support::{client::Client, com_slots, dll::*, document::Span, oracle};
use std::{ffi::c_void, rc::Rc};
use windows::Win32::{Foundation::*, UI::TextServices::*};
use windows_core::{HRESULT, IUnknown, Interface};

fn owned(c: &Client) -> IUnknown {
    let pointer = c.a.active.borrow().as_ref().unwrap().pointer;
    unsafe { IUnknown::from_raw(com_slots::retain(pointer)) }
}
fn terminate(sink: &ITfCompositionSink, pointer: *mut c_void) -> HRESULT {
    unsafe { (sink.vtable().OnCompositionTerminated)(sink.as_raw(), 123, pointer) }
}
fn busy(reader: &SnapshotReaderV2) {
    let mut value = SnapshotV2::default();
    value.base.active = 1;
    unsafe {
        assert_eq!(
            reader.read(564, 2, &mut value),
            HRESULT(0x8000000Au32 as i32)
        );
    }
    assert_eq!(value, SnapshotV2::default());
}
#[test]
fn lifecycle_wrong_thread_does_not_query_or_change_ownership() {
    let c = Client::new();
    c.type_text("g");
    let sink = c.service.cast::<ITfCompositionSink>().unwrap();
    let composition = owned(&c);
    let before = snapshot_v2(&c.service);
    let trace = c.host.trace.borrow().clone();
    let pointer = composition.as_raw() as usize;
    let raw = sink.as_raw() as usize;
    std::thread::spawn(move || unsafe {
        let raw = raw as *mut _;
        let borrowed = ITfCompositionSink::from_raw_borrowed(&raw).unwrap();
        assert_eq!(terminate(borrowed, pointer as *mut _), RPC_E_WRONG_THREAD);
    })
    .join()
    .unwrap();
    assert_eq!(snapshot_v2(&c.service), before);
    assert_eq!(*c.host.trace.borrow(), trace);
    oracle::document(&c, "g", 8);
}
#[test]
fn lifecycle_exact_and_late_termination_preserve_distinct_serials() {
    let c = Client::new();
    c.type_text("g");
    let sink = c.service.cast::<ITfCompositionSink>().unwrap();
    let old = owned(&c);
    let before = snapshot_v2(&c.service);
    assert_eq!(before.lifecycle[4], 1);
    assert_eq!(before.last()[1], 1);
    c.a.active.borrow_mut().take();
    c.host.trace.borrow_mut().clear();
    assert_eq!(terminate(&sink, old.as_raw()), S_OK);
    let after = snapshot_v2(&c.service);
    assert_eq!(after.lifecycle[4], 0);
    assert_eq!(&after.last()[1..], &[4, before.lifecycle[5], 0, 0]);
    assert_eq!(
        c.host
            .trace
            .borrow()
            .iter()
            .filter(|n| **n == "composition.QI")
            .count(),
        1
    );
    assert!(
        !c.host
            .trace
            .borrow()
            .iter()
            .any(|n| matches!(*n, "GetText" | "SetText" | "EndComposition"))
    );
    oracle::document(&c, "g", 8);
    c.a.selection.set(Span {
        start: 7,
        length: 0,
    });
    c.type_text("x");
    let current = snapshot_v2(&c.service).lifecycle[5];
    assert_eq!(current, before.lifecycle[5] + 1);
    assert_eq!(terminate(&sink, old.as_raw()), S_OK);
    let after = snapshot_v2(&c.service);
    assert_eq!(&after.last()[1..], &[5, current, 0, 0]);
    assert_eq!(after.lifecycle[5], current);
    assert!(c.key(b'y' as u16, false));
    oracle::document(&c, "xyg", 9);
}
#[test]
fn lifecycle_identity_reentry_replaces_only_the_captured_serial() {
    for reentry in [false, true] {
        let c = Client::new();
        c.type_text("ghbdtn ");
        let sink = c.service.cast::<ITfCompositionSink>().unwrap();
        let old = owned(&c);
        let before = snapshot_v2(&c.service);
        let effects = (c.a.writes.get(), c.a.starts.get(), c.a.ends.get());
        if reentry {
            let event = c.api.event;
            let handle = c.handle as usize;
            let context = c.context_a.as_raw() as usize;
            c.host.on("composition.QI", 0, move || {
                let mut eaten = 0;
                unsafe {
                    assert_eq!(
                        event(
                            handle as *mut _,
                            context as *mut _,
                            &Event::text(b'x' as u16),
                            &mut eaten
                        ),
                        S_OK
                    );
                }
                assert_eq!(eaten, 1);
            });
            assert_eq!(terminate(&sink, old.as_raw()), S_OK);
        } else {
            assert!(c.key(b'x' as u16, false));
        }
        let after = snapshot_v2(&c.service);
        assert_eq!(after.lifecycle[4], 1);
        assert_eq!(after.lifecycle[5], before.lifecycle[5] + 1);
        assert_eq!(after.base.stopped, 0);
        assert_eq!(
            (c.a.writes.get(), c.a.starts.get(), c.a.ends.get()),
            (effects.0 + 1, effects.1 + 1, effects.2 + 1)
        );
        if reentry {
            assert_eq!(&after.last()[1..], &[7, before.lifecycle[5], 0, 3]);
        }
        oracle::document(&c, "привет x", 15);
        assert!(c.key(b'y' as u16, false));
        oracle::document(&c, "привет xy", 16);
    }
}
#[test]
fn lifecycle_unresolved_identity_classifies_hresult_null_and_reentry() {
    for case in 0..5 {
        let c = Client::new();
        c.type_text("g");
        let sink = c.service.cast::<ITfCompositionSink>().unwrap();
        let composition = owned(&c);
        let serial = snapshot_v2(&c.service).lifecycle[5];
        if case != 4 {
            let reader = c.service.cast::<SnapshotReaderV2>().unwrap();
            let focus = c.service.cast::<ITfKeyEventSink>().unwrap();
            let nested = sink.clone();
            let pointer = composition.as_raw() as usize;
            c.host.on("composition.QI", 0, move || {
                busy(&reader);
                if case == 0 {
                    unsafe {
                        assert_eq!(
                            (focus.vtable().OnSetFocus)(focus.as_raw(), false.into()),
                            S_OK
                        );
                    }
                }
                if case == 1 {
                    assert_eq!(terminate(&nested, pointer as *mut _), S_OK);
                }
            });
        }
        if case == 2 || case == 3 {
            c.a.results
                .borrow_mut()
                .insert("QI", if case == 2 { E_FAIL } else { S_FALSE });
        }
        assert_eq!(
            terminate(
                &sink,
                if case == 4 {
                    std::ptr::null_mut()
                } else {
                    composition.as_raw()
                }
            ),
            S_OK
        );
        c.a.results.borrow_mut().clear();
        let after = snapshot_v2(&c.service);
        assert_eq!(
            &after.last()[1..],
            &[
                7,
                serial,
                0,
                if case < 2 {
                    3
                } else if case == 4 {
                    1
                } else {
                    2
                }
            ],
            "case {case}"
        );
        assert_eq!(after.base.stopped, u32::from(case == 0));
        oracle::document(&c, "g", 8);
    }
}
#[test]
fn lifecycle_end_reentry_cannot_publish_verified_finish_or_new_acquisition() {
    for reentry in [false, true] {
        let c = Client::new();
        c.type_text("ghbdtn ");
        let before = snapshot_v2(&c.service);
        let writes = c.a.writes.get();
        if reentry {
            let sink = c.service.cast::<ITfCompositionSink>().unwrap();
            let composition = owned(&c);
            c.host.on("EndComposition", 0, move || {
                assert_eq!(terminate(&sink, composition.as_raw()), S_OK);
            });
        }
        assert_eq!(c.key(b'x' as u16, false), !reentry);
        let after = snapshot_v2(&c.service);
        assert_eq!(
            &after.kinds()[before.lifecycle[3] as usize..],
            if reentry { &[2, 4][..] } else { &[2, 3, 1][..] }
        );
        assert_eq!(c.a.ends.get(), 1);
        assert_eq!(c.a.writes.get(), writes + u32::from(!reentry));
        oracle::document(
            &c,
            if reentry {
                "привет "
            } else {
                "привет x"
            },
            if reentry { 14 } else { 15 },
        );
    }
}
#[test]
fn lifecycle_invalidation_and_detach_are_distinct_from_termination() {
    for detach in [false, true] {
        let c = Client::new();
        c.type_text("g");
        let serial = snapshot_v2(&c.service).lifecycle[5];
        if detach {
            let reader = c.service.cast::<SnapshotReaderV2>().unwrap();
            c.host.on("UnadviseSingleSink", 0, move || busy(&reader));
            assert_eq!(c.deactivate(), S_OK);
            let after = snapshot_v2(&c.service);
            assert_eq!(after.lifecycle[4], 0);
            assert_eq!(after.last()[1], 9);
            let count = after.lifecycle[3] as usize;
            assert_eq!(after.lifecycle[6 + (count - 2) * 5 + 3], 8);
        } else {
            let focus = c.service.cast::<ITfKeyEventSink>().unwrap();
            let manager = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
            unsafe {
                assert_eq!(
                    (focus.vtable().OnSetFocus)(focus.as_raw(), false.into()),
                    S_OK
                );
                assert_eq!(
                    (manager.vtable().OnPopContext)(manager.as_raw(), c.context_a.as_raw()),
                    S_OK
                );
            }
            let after = snapshot_v2(&c.service);
            assert_eq!(after.lifecycle[5], serial);
            assert_eq!(after.base.stopped, 1);
            assert_eq!(after.last()[3], 5);
            let count = after.lifecycle[3] as usize;
            assert_eq!(after.lifecycle[6 + (count - 2) * 5 + 3], 1);
            let sink = c.service.cast::<ITfCompositionSink>().unwrap();
            let composition = owned(&c);
            assert_eq!(terminate(&sink, composition.as_raw()), S_OK);
            assert_eq!(snapshot_v2(&c.service).base.stopped, 1);
        }
        oracle::document(&c, "g", 8);
    }
}
#[test]
fn lifecycle_readers_preserve_payload_canaries_and_exact_legacy_payload() {
    #[repr(C)]
    struct Guard<T> {
        before: u32,
        payload: T,
        after: u32,
    }
    let c = Client::new();
    c.type_text("g");
    let old = c.service.cast::<SnapshotReader>().unwrap();
    let new = c.service.cast::<SnapshotReaderV2>().unwrap();
    let mut legacy = Guard {
        before: 17,
        payload: Snapshot::default(),
        after: 23,
    };
    let mut current = Guard {
        before: 17,
        payload: SnapshotV2::default(),
        after: 23,
    };
    unsafe {
        assert_eq!(old.read(220, 1, &mut legacy.payload), S_OK);
        assert_eq!(legacy.payload, snapshot_v2(&c.service).base);
        assert_eq!(old.read(224, 1, &mut legacy.payload), E_INVALIDARG);
        assert_eq!(new.read(568, 2, &mut current.payload), E_INVALIDARG);
    }
    assert_eq!(
        (legacy.before, legacy.after, current.before, current.after),
        (17, 23, 17, 23)
    );
}
#[test]
fn lifecycle_reads_at_every_edit_boundary_leave_the_complete_trace_unchanged() {
    let mut expected = Vec::new();
    for observe in [false, true] {
        let c = Client::new();
        let reader = c.service.cast::<SnapshotReaderV2>().unwrap();
        c.host.trace.borrow_mut().clear();
        if observe {
            *c.host.observer.borrow_mut() = Some(Rc::new(move |_| busy(&reader)));
        }
        c.type_text("ghbdtn x");
        let observer = c.host.observer.borrow_mut().take();
        drop(observer);
        oracle::document(&c, "привет x", 15);
        if observe {
            assert_eq!(*c.host.trace.borrow(), expected);
        } else {
            expected = c.host.trace.borrow().clone();
        }
    }
}

#[test]
fn lifecycle_alias_and_unowned_callbacks_use_canonical_identity() {
    for alias in [false, true] {
        let c = Client::new();
        let sink = c.service.cast::<ITfCompositionSink>().unwrap();
        assert_eq!(terminate(&sink, std::ptr::null_mut()), S_OK);
        assert_eq!(snapshot_v2(&c.service).last()[4], 1);
        c.type_text("g");
        let composition = owned(&c);
        let serial = snapshot_v2(&c.service).lifecycle[5];
        let pointer = if alias {
            let provider = unsafe { com_slots::owner(composition.as_raw()) };
            (&provider.composition_alias as *const com_slots::Slot<ITfComposition_Vtbl>)
                .cast_mut()
                .cast()
        } else {
            composition.as_raw()
        };
        if alias {
            assert_ne!(pointer, composition.as_raw());
        }
        c.a.active.borrow_mut().take();
        assert_eq!(terminate(&sink, pointer), S_OK);
        assert_eq!(&snapshot_v2(&c.service).last()[1..], &[4, serial, 0, 0]);
        c.host.trace.borrow_mut().clear();
        assert_eq!(terminate(&sink, pointer), S_OK);
        assert_eq!(snapshot_v2(&c.service).last()[1], 6);
        assert!(c.host.trace.borrow().is_empty());
        oracle::document(&c, "g", 8);
    }
}

#[test]
fn lifecycle_identity_release_stays_inside_the_publication_guard() {
    for nested in [false, true] {
        let c = Client::new();
        c.type_text("g");
        let sink = c.service.cast::<ITfCompositionSink>().unwrap();
        let composition = owned(&c);
        let reader = c.service.cast::<SnapshotReaderV2>().unwrap();
        let seen = Rc::new(std::cell::Cell::new(0));
        let count = Rc::clone(&seen);
        *c.host.observer.borrow_mut() = Some(Rc::new(move |name| {
            if name == "Release.before" {
                busy(&reader);
                count.set(count.get() + 1);
            }
        }));
        if nested {
            let nested = sink.clone();
            let pointer = composition.as_raw() as usize;
            c.host.on("composition.QI", 0, move || {
                assert_eq!(terminate(&nested, pointer as *mut _), S_OK);
            });
        }
        assert_eq!(terminate(&sink, composition.as_raw()), S_OK);
        let observer = c.host.observer.borrow_mut().take();
        drop(observer);
        assert!(seen.get() > 0);
        oracle::document(&c, "g", 8);
    }
}

#[test]
fn stale_failed_identity_cannot_stop_a_reactivated_service() {
    let c = Client::new();
    c.type_text("g");
    let sink = c.service.cast::<ITfCompositionSink>().unwrap();
    let old = owned(&c);
    let service = c.service.clone();
    let thread = c.thread.clone();
    let document = Rc::clone(&c.a);
    let event = c.api.event;
    let handle = c.handle as usize;
    let context = c.context_a.as_raw() as usize;
    c.host.on("composition.QI", 0, move || unsafe {
        assert_eq!((service.vtable().base__.Deactivate)(service.as_raw()), S_OK);
        document.active.borrow_mut().take();
        document.selection.set(Span {
            start: 7,
            length: 0,
        });
        assert_eq!(
            (service.vtable().ActivateEx)(service.as_raw(), thread.as_raw(), 42, 0),
            S_OK
        );
        let mut eaten = 0;
        assert_eq!(
            event(
                handle as *mut _,
                context as *mut _,
                &Event::text(b'x' as u16),
                &mut eaten
            ),
            S_OK
        );
        assert_eq!(eaten, 1);
        document.results.borrow_mut().insert("QI", E_FAIL);
    });
    assert_eq!(terminate(&sink, old.as_raw()), S_OK);
    c.a.results.borrow_mut().clear();
    assert_eq!(snapshot(&c.service).stopped, 0);
    assert!(c.key(b'y' as u16, false));
    oracle::document(&c, "xyg", 9);
}
