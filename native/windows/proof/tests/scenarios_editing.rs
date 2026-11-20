use crate::support::{
    client::Client,
    dll::{Event, snapshot},
    document::Span,
    oracle,
};
use std::rc::Rc;
use windows::Win32::{Foundation::*, UI::TextServices::*};
use windows_core::{IUnknown, Interface};
fn stopped(c: &Client) -> bool {
    snapshot(&c.service).stopped != 0
}
#[test]
fn correction_and_reversal_preserve_owned_range() {
    let c = Client::new();
    c.type_text("ghbdtn ");
    oracle::document(&c, "привет ", 14);
    assert!(c.event(Event::command(3)));
    oracle::document(&c, "ghbdtn ", 14);
    assert!(c.a.active.borrow().is_none());
}
#[test]
fn native_whitespace_uses_ucrt_boundary_classification() {
    unsafe extern "C" {
        fn iswspace(unit: u16) -> i32;
    }
    let boundary = unsafe { iswspace(0x180e) != 0 };
    let c = Client::new();
    *c.a.text.borrow_mut() = "prefix\u{180e}suffix".encode_utf16().collect();
    for unit in "ghbdtn ".encode_utf16() {
        assert_eq!(c.key(unit, true), boundary);
    }
    assert_eq!(
        *c.a.text.borrow(),
        format!(
            "prefix\u{180e}{}suffix",
            if boundary { "привет " } else { "ghbdtn " }
        )
        .encode_utf16()
        .collect::<Vec<_>>()
    );
    assert_eq!(
        c.a.selection.get(),
        Span {
            start: 14,
            length: 0
        }
    );
    assert_eq!(c.a.starts.get(), u32::from(boundary));
    assert_eq!(
        *c.b.text.borrow(),
        "prefix suffix".encode_utf16().collect::<Vec<_>>()
    );
}
#[test]
fn denied_requests_and_rejected_compositions_preserve_key_order() {
    for case in 0..4 {
        let c = Client::new();
        c.a.invoke.set(false);
        match case {
            0 => {
                c.a.results
                    .borrow_mut()
                    .insert("RequestEditSession", E_FAIL);
            }
            1 => {
                c.a.results.borrow_mut().insert("session", TF_E_SYNCHRONOUS);
            }
            2 => {
                c.a.results.borrow_mut().insert("session", TF_S_ASYNC);
            }
            _ => (),
        };
        assert!(!c.key(b'g' as u16, true));
        oracle::document(&c, "g", 8);
        assert_eq!(c.a.reads.get(), 0);
        oracle::no_effects(&c);
        assert_eq!(c.delayed(), E_FAIL);
        oracle::no_effects(&c);
    }
    let c = Client::new();
    c.a.reject_composition.set(true);
    assert!(!c.key(b'g' as u16, true));
    oracle::document(&c, "g", 8);
    assert_eq!(c.a.writes.get(), 0);
}
#[test]
fn owned_denial_stops_without_cleanup_or_later_repair() {
    let c = Client::new();
    c.type_text("gh");
    let writes = c.a.writes.get();
    c.a.invoke.set(false);
    c.a.results.borrow_mut().insert("session", TF_E_SYNCHRONOUS);
    assert!(!c.key(b'b' as u16, false));
    assert_eq!(c.a.writes.get(), writes);
    assert_eq!(c.a.ends.get(), 0);
    assert!(stopped(&c));
    c.a.invoke.set(true);
    assert!(!c.key(b'x' as u16, true));
    oracle::document(&c, "ghx", 10);
    assert_eq!(c.a.writes.get(), writes);
}
#[test]
fn failed_outer_request_after_mark_keeps_consumption_and_expires_callback() {
    for continuation in [false, true] {
        let c = Client::new();
        if continuation {
            c.type_text("ghbdtn ");
        }
        c.a.retain_callback.set(true);
        c.a.results
            .borrow_mut()
            .insert("RequestEditSession", E_FAIL);
        assert!(c.key(if continuation { b'x' } else { b'g' } as u16, false));
        assert!(stopped(&c));
        assert_eq!(c.delayed(), E_FAIL);
        oracle::document(
            &c,
            if continuation { "привет x" } else { "g" },
            if continuation { 15 } else { 8 },
        );
    }
}
#[test]
fn composition_drift_refuses_without_document_changes() {
    for notepad in [false, true] {
        for case in 0..5 {
            let c = if notepad {
                Client::notepad()
            } else {
                Client::new()
            };
            c.type_text("ghbdtn");
            let writes = c.a.writes.get();
            match case {
                0 => {
                    let mut s = c.a.selection.get();
                    s.start -= 1;
                    c.a.selection.set(s);
                }
                1 => {
                    let span = Rc::clone(&c.a.active.borrow().as_ref().unwrap().span);
                    let mut s = span.get();
                    s.start += 1;
                    span.set(s);
                }
                2 => c.a.text.borrow_mut()[7] = b'!' as u16,
                3 => {
                    c.a.active.borrow_mut().take();
                }
                4 => c.a.foreign.set(true),
                _ => unreachable!(),
            };
            let text = c.a.text.borrow().clone();
            let selection = c.a.selection.get();
            assert!(!c.key(b' ' as u16, false));
            assert_eq!(*c.a.text.borrow(), text);
            assert_eq!(c.a.selection.get(), selection);
            assert_eq!(c.a.writes.get(), writes);
            assert!(stopped(&c));
            assert_eq!(
                *c.b.text.borrow(),
                "prefix suffix".encode_utf16().collect::<Vec<_>>()
            );
        }
    }
}
#[test]
fn unknown_mark_consumes_once_and_never_repairs() {
    for notepad in [false, true] {
        for case in 0..3 {
            let c = if notepad {
                Client::notepad()
            } else {
                Client::new()
            };
            c.type_text("ghbdtn");
            let writes = c.a.writes.get();
            if case == 0 {
                c.a.results.borrow_mut().insert("SetText", E_FAIL);
            } else {
                let d = Rc::clone(&c.a);
                c.host.on("SetText.after", 0, move || {
                    if case == 1 {
                        d.text.borrow_mut()[7] = b'!' as u16;
                    } else {
                        let span = Rc::clone(&d.active.borrow().as_ref().unwrap().span);
                        let mut s = span.get();
                        s.length += 1;
                        span.set(s);
                    }
                });
            }
            assert!(c.key(b' ' as u16, false));
            assert_eq!(c.a.writes.get(), writes + 1);
            assert!(stopped(&c));
            let text = c.a.text.borrow().clone();
            assert!(!c.key(b'x' as u16, false));
            assert_eq!(*c.a.text.borrow(), text);
            assert_eq!(c.a.writes.get(), writes + 1);
            assert_eq!(
                *c.b.text.borrow(),
                "prefix suffix".encode_utf16().collect::<Vec<_>>()
            );
        }
    }
}
#[test]
fn unknown_finish_passes_ordinary_keys_without_replan() {
    for case in 0..5 {
        let c = Client::new();
        c.type_text("ghbdtn ");
        let writes = c.a.writes.get();
        match case {
            0 => {
                c.a.results.borrow_mut().insert("EndComposition", E_FAIL);
            }
            1 => c.a.keep_composition.set(true),
            2 => {
                c.a.results
                    .borrow_mut()
                    .insert("RequestEditSession", E_FAIL);
            }
            3 => {
                c.a.results.borrow_mut().insert("session", E_FAIL);
            }
            4 => {
                let d = Rc::clone(&c.a);
                c.host.on("EndComposition.after", 0, move || {
                    d.text.borrow_mut()[7] = b'!' as u16
                });
            }
            _ => unreachable!(),
        };
        let eaten = if case == 2 || case == 3 {
            c.event(Event::command(6))
        } else {
            c.key(b'x' as u16, false)
        };
        assert!(!eaten);
        assert_eq!(c.a.writes.get(), writes);
        assert!(stopped(&c));
        oracle::document(
            &c,
            if case == 4 {
                "!ривет "
            } else {
                "привет "
            },
            14,
        );
    }
}
#[test]
fn unknown_reserved_escape_stays_consumed() {
    for result in ["SetText", "EndComposition", "RequestEditSession"] {
        let c = Client::new();
        c.type_text("ghbdtn ");
        c.a.results.borrow_mut().insert(result, E_FAIL);
        assert!(c.event(Event::command(3)));
        assert!(stopped(&c));
        assert_eq!(
            *c.a.text.borrow(),
            "prefix ghbdtn suffix".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            *c.b.text.borrow(),
            "prefix suffix".encode_utf16().collect::<Vec<_>>()
        );
    }
}
#[test]
fn hresult_before_and_after_native_effect_replaces_throw_schedules() {
    for boundary in ["GetRange", "SetText.before", "SetText.after"] {
        let c = Client::new();
        c.type_text("gh");
        let writes = c.a.writes.get();
        if boundary == "SetText.after" {
            let d = Rc::clone(&c.a);
            c.host.on(boundary, 0, move || {
                d.results.borrow_mut().insert("SetText", E_FAIL);
            });
        } else {
            c.a.results.borrow_mut().insert(boundary, E_FAIL);
        }
        assert_eq!(c.key(b'b' as u16, false), boundary != "GetRange");
        assert!(stopped(&c));
        assert_eq!(
            c.a.writes.get(),
            writes + u32::from(boundary == "SetText.after")
        );
        assert_eq!(
            *c.a.text.borrow(),
            if boundary == "SetText.after" {
                "prefix ghbsuffix"
            } else {
                "prefix ghsuffix"
            }
            .encode_utf16()
            .collect::<Vec<_>>()
        );
    }
}
#[test]
fn stale_edit_across_reactivation_cannot_stop_new_ownership() {
    for failed_after in [false, true] {
        let c = Client::new();
        c.a.retain_callback.set(true);
        c.a.results
            .borrow_mut()
            .insert("RequestEditSession", E_FAIL);
        let service = c.service.clone();
        let manager = c.thread.clone();
        c.host.on("RequestEditSession", 0, move || unsafe {
            assert_eq!((service.vtable().base__.Deactivate)(service.as_raw()), S_OK);
            assert_eq!(
                (service.vtable().ActivateEx)(service.as_raw(), manager.as_raw(), 42, 0),
                S_OK
            );
        });
        if failed_after {
            let d = Rc::clone(&c.a);
            c.host.on("RequestEditSession.after", 0, move || {
                d.results
                    .borrow_mut()
                    .insert("RequestEditSession", E_ACCESSDENIED);
            });
        }
        assert!(!c.key(b'g' as u16, false));
        assert_eq!(c.a.writes.get(), 0);
        assert!(!stopped(&c));
        let retained = c.a.retained.borrow_mut().take().unwrap();
        c.a.retain_callback.set(false);
        c.a.results.borrow_mut().clear();
        c.type_text("ghbdtn ");
        oracle::document(&c, "привет ", 14);
        let writes = c.a.writes.get();
        let trace = c.host.trace.borrow().clone();
        let identity = c.a.active.borrow().as_ref().unwrap().pointer;
        let session = retained.cast::<ITfEditSession>().unwrap();
        unsafe {
            assert_eq!(
                (session.vtable().DoEditSession)(session.as_raw(), 123),
                E_FAIL
            );
        }
        assert_eq!(*c.host.trace.borrow(), trace);
        assert_eq!(c.a.writes.get(), writes);
        assert_eq!(c.a.active.borrow().as_ref().unwrap().pointer, identity);
        drop(session);
        drop(retained);
        assert!(c.event(Event::command(5)));
        oracle::document(&c, "ghbdtn ", 14);
    }
}
#[test]
fn late_old_termination_and_focus_roundtrip_cannot_revive_authority() {
    let c = Client::new();
    c.type_text("gh");
    let pointer = c.a.active.borrow().as_ref().unwrap().pointer;
    let old = unsafe { IUnknown::from_raw(crate::support::com_slots::retain(pointer)) };
    c.a.active.borrow_mut().take();
    let termination = c.service.cast::<ITfCompositionSink>().unwrap();
    unsafe {
        assert_eq!(
            (termination.vtable().OnCompositionTerminated)(termination.as_raw(), 123, pointer),
            S_OK
        );
    }
    c.a.selection.set(Span {
        start: 7,
        length: 0,
    });
    c.type_text("x");
    let identity = c.a.active.borrow().as_ref().unwrap().pointer;
    let writes = c.a.writes.get();
    unsafe {
        assert_eq!(
            (termination.vtable().OnCompositionTerminated)(termination.as_raw(), 123, old.as_raw()),
            S_OK
        );
    }
    assert!(c.key(b'y' as u16, false));
    assert_eq!(c.a.active.borrow().as_ref().unwrap().pointer, identity);
    assert_eq!(c.a.writes.get(), writes + 1);
    let other = c.context_b.cast::<IUnknown>().unwrap();
    let prior = c.manager.top.replace(other);
    drop(prior);
    let writes = c.a.writes.get();
    assert!(!c.key(b'z' as u16, false));
    assert_eq!(c.a.writes.get(), writes);
    let focus = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
    let manager = c.thread.cast::<ITfDocumentMgr>().unwrap();
    unsafe {
        assert_eq!(
            (focus.vtable().OnSetFocus)(focus.as_raw(), manager.as_raw(), std::ptr::null_mut()),
            S_OK
        );
    }
    assert!(stopped(&c));
    let prior = c
        .manager
        .top
        .replace(c.context_a.cast::<IUnknown>().unwrap());
    drop(prior);
    assert!(!c.key(b'z' as u16, false));
    assert_eq!(c.a.writes.get(), writes);
    assert_eq!(
        *c.a.text.borrow(),
        "prefix xyghsuffix".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        *c.b.text.borrow(),
        "prefix suffix".encode_utf16().collect::<Vec<_>>()
    );
}
#[test]
fn changing_finish_caret_requires_verified_restoration() {
    for case in 0..6 {
        let c = Client::new();
        c.type_text("ghbdtn ");
        let before = snapshot(&c.service);
        let writes = c.a.writes.get();
        let d = Rc::clone(&c.a);
        c.host.on("SetText.after", 0, move || {
            let start = d.active.borrow().as_ref().unwrap().span.get().start;
            d.selection.set(Span { start, length: 0 });
        });
        if case == 1 {
            c.a.results.borrow_mut().insert("SetSelection", E_FAIL);
        }
        if case == 2 {
            let d = Rc::clone(&c.a);
            c.host.on("SetSelection.after", 0, move || {
                d.selection.set(Span {
                    start: 7,
                    length: 0,
                })
            });
        }
        if case == 3 || case == 4 {
            let d = Rc::clone(&c.a);
            c.host.on("EndComposition.after", 0, move || {
                d.selection.set(if case == 3 {
                    Span {
                        start: 7,
                        length: 0,
                    }
                } else {
                    Span {
                        start: 14,
                        length: 1,
                    }
                })
            });
        }
        if case == 5 {
            let event = c.api.event;
            let handle = c.handle as usize;
            let context = c.context_a.as_raw() as usize;
            c.host.on("SetSelection.after", 0, move || {
                let mut eaten = 1;
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
                assert_eq!(eaten, 0);
            });
        }
        assert!(c.event(Event::command(3)));
        assert_eq!(
            *c.a.text.borrow(),
            "prefix ghbdtn suffix".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            *c.b.text.borrow(),
            "prefix suffix".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(c.a.writes.get(), writes + 1);
        let after = snapshot(&c.service);
        if case == 0 {
            oracle::document(&c, "ghbdtn ", 14);
            assert!(c.a.active.borrow().is_none());
            assert!(!stopped(&c));
            assert_eq!(after.counts[9], before.counts[9] + 1);
            assert_eq!(after.counts[11], before.counts[11]);
        } else {
            assert!(stopped(&c));
            assert_eq!(after.counts[9], before.counts[9]);
            assert_eq!(after.counts[11], before.counts[11] + 1);
            let selection = c.a.selection.get();
            assert!(!c.key(b'x' as u16, false));
            assert_eq!(c.a.selection.get(), selection);
            assert_eq!(c.a.writes.get(), writes + 1);
        }
    }
}
#[test]
fn every_exercised_native_boundary_stops_writes_after_nested_input() {
    let boundaries = {
        let c = Client::new();
        c.type_text("ghbdtn");
        c.host.trace.borrow_mut().clear();
        assert!(c.key(b' ' as u16, false));
        let mut names = c.host.trace.borrow().clone();
        names.retain(|s| {
            !s.starts_with("AddRef")
                && !s.starts_with("Release")
                && !matches!(
                    *s,
                    "QI" | "QI.after"
                        | "Next"
                        | "SetText"
                        | "SetText.after"
                        | "GetFocus"
                        | "capture"
                        | "Layouts"
                )
        });
        names.sort();
        names.dedup();
        names
    };
    for boundary in boundaries {
        let c = Client::new();
        c.type_text("ghbdtn");
        let before = c.a.writes.get();
        let event = c.api.event;
        let handle = c.handle as usize;
        let context = c.context_a.as_raw() as usize;
        let d = Rc::clone(&c.a);
        let observed = Rc::new(std::cell::Cell::new(None));
        let record = Rc::clone(&observed);
        c.host.on(boundary, 0, move || {
            record.set(Some(
                d.writes.get() + u32::from(boundary == "SetText.before"),
            ));
            let mut eaten = 1;
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
            assert_eq!(eaten, 0);
        });
        let eaten = c.key(b' ' as u16, false);
        assert_eq!(
            c.a.writes.get(),
            observed.get().unwrap(),
            "boundary{boundary}"
        );
        assert_eq!(eaten, c.a.writes.get() > before, "boundary{boundary}");
        assert_eq!(
            *c.a.text.borrow(),
            if c.a.writes.get() > before {
                "prefix привет suffix"
            } else {
                "prefix ghbdtnsuffix"
            }
            .encode_utf16()
            .collect::<Vec<_>>()
        );
        assert_eq!(
            *c.b.text.borrow(),
            "prefix suffix".encode_utf16().collect::<Vec<_>>()
        );
    }
}
