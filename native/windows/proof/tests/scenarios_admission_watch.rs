use crate::support::{client::Client, oracle};
use std::{cell::Cell, rc::Rc};
use windows::Win32::{
    Foundation::*,
    UI::{Input::KeyboardAndMouse::*, TextServices::*},
};
use windows_core::{BOOL, Interface};

fn down(c: &Client) -> bool {
    let mut previous = [0; 256];
    unsafe {
        assert!(crate::support::keyboard_state(&mut previous));
        SetKeyboardState(&[0; 256]).unwrap();
        let sink = c.manager.key_sink.borrow().clone().unwrap();
        let mut eaten = BOOL(0);
        let result = (sink.sink.vtable().OnKeyDown)(
            sink.sink.as_raw(),
            c.context_a.as_raw(),
            WPARAM(71),
            LPARAM(((MapVirtualKeyW(71, MAPVK_VK_TO_VSC) as isize) << 16) | 1),
            &mut eaten,
        );
        SetKeyboardState(&previous).unwrap();
        assert_eq!(result, S_OK);
        c.host.check();
        eaten.as_bool()
    }
}

#[test]
fn admission_notifications_revoke_every_late_metadata_occurrence() {
    let baseline = Client::notepad();
    baseline.host.trace.borrow_mut().clear();
    assert!(down(&baseline));
    let trace = baseline.host.trace.borrow().clone();
    for boundary in [
        "GetWnd",
        "GetInputScopes",
        "capture",
        "GetStatus",
        "GetFocus",
        "GetTop",
        "GetActiveView",
        "range.GetContext",
        "GetAppProperty",
        "scope.GetValue",
        "GetSelection",
        "GetExtent",
        "Clone",
        "ShiftStart",
        "GetRange",
        "compartment.GetValue",
        "thread.GetValue",
    ] {
        let count = trace.iter().filter(|entry| **entry == boundary).count();
        assert!(count > 0, "metadata boundary {boundary} must be exercised");
        for skip in 0..count {
            for change in 0..4 {
                let c = Client::notepad();
                let document = Rc::clone(&c.a);
                let manager = Rc::clone(&c.manager);
                let host = Rc::clone(&c.host);
                let at = Rc::new(Cell::new(None));
                let observed = Rc::clone(&at);
                c.host.on(boundary, skip, move || {
                    observed.set(Some((
                        document.reads.get(),
                        document.writes.get(),
                        document.starts.get(),
                    )));
                    match change {
                        0 => document.status.set(TF_SD_READONLY),
                        1 => document.disabled.set(true),
                        2 => document.empty_disabled.set(true),
                        _ => manager.disabled.set(true),
                    }
                    let sinks = host
                        .watches
                        .borrow()
                        .iter()
                        .filter(|watch| {
                            watch.id
                                == if change == 0 {
                                    ITfStatusSink::IID
                                } else {
                                    ITfCompartmentEventSink::IID
                                }
                        })
                        .map(|watch| watch.sink.clone())
                        .collect::<Vec<_>>();
                    for unknown in sinks {
                        unsafe {
                            if change == 0 {
                                let sink = unknown.cast::<ITfStatusSink>().unwrap();
                                assert_eq!(
                                    (sink.vtable().OnStatusChange)(
                                        sink.as_raw(),
                                        document.context.get(),
                                        TF_SD_READONLY
                                    ),
                                    S_OK
                                );
                            } else {
                                let sink = unknown.cast::<ITfCompartmentEventSink>().unwrap();
                                let id = if change == 2 {
                                    GUID_COMPARTMENT_EMPTYCONTEXT
                                } else {
                                    GUID_COMPARTMENT_KEYBOARD_DISABLED
                                };
                                assert_eq!((sink.vtable().OnChange)(sink.as_raw(), &id), S_OK);
                            }
                        }
                    }
                });
                let eaten = down(&c);
                let (reads, writes, starts) =
                    at.get().expect("scheduled metadata occurrence reached");
                assert_eq!(
                    (c.a.reads.get(), c.a.writes.get(), c.a.starts.get()),
                    (reads, writes, starts),
                    "{boundary} occurrence {skip} change {change}"
                );
                assert_eq!(eaten, writes != 0, "key follows attempted character write");
                assert!(c.host.watches.borrow().is_empty());
                c.host.drained();
            }
        }
    }
}

#[test]
fn partial_watch_acquisition_and_failed_retirement_stop_authority() {
    for skip in 0..3 {
        let c = Client::notepad();
        let host = Rc::clone(&c.host);
        c.host.on("AdviseAdmission", skip, move || {
            host.watch_result.set(E_FAIL)
        });
        assert!(!down(&c));
        assert_eq!(c.a.reads.get(), 0);
        oracle::no_effects(&c);
        assert!(c.host.watches.borrow().is_empty());
        c.host.drained();
    }
    let c = Client::notepad();
    c.host.unwatch_result.set(E_FAIL);
    assert!(down(&c));
    let previous = (c.a.reads.get(), c.a.writes.get(), c.a.starts.get());
    assert!(!down(&c));
    assert_eq!(
        (c.a.reads.get(), c.a.writes.get(), c.a.starts.get()),
        previous
    );
    assert!(c.host.watches.borrow().is_empty());
    c.host.drained();
}

#[test]
fn missing_admission_watch_refuses_without_text_or_composition() {
    for result in [E_FAIL, S_FALSE] {
        let c = Client::notepad();
        c.host.watch_result.set(result);
        assert!(!down(&c));
        assert_eq!(c.a.reads.get(), 0);
        oracle::no_effects(&c);
        assert!(c.host.watches.borrow().is_empty());
        c.host.drained();
    }
}

#[test]
fn successful_watch_without_cookie_is_terminal_until_reactivation() {
    let c = Client::notepad();
    c.host.watch_missing_cookie.set(true);
    assert!(!down(&c));
    assert_eq!(c.host.watches.borrow().len(), 1);
    assert!(!down(&c));
    assert_eq!(
        c.host.watches.borrow().len(),
        1,
        "unknown acquisition must not be retried"
    );
    assert_eq!(c.a.reads.get(), 0);
    oracle::no_effects(&c);
    let retained = std::mem::take(&mut *c.host.watches.borrow_mut());
    drop(retained);
    c.host.drained();
}

#[test]
fn scope_revocation_at_every_late_getter_and_release_precedes_next_operation() {
    let baseline = Client::notepad();
    baseline.host.trace.borrow_mut().clear();
    assert!(down(&baseline));
    let trace = baseline.host.trace.borrow().clone();
    for boundary in [
        "GetWnd",
        "Release.before",
        "scope.GetValue",
        "GetInputScopes",
    ] {
        let count = trace.iter().filter(|entry| **entry == boundary).count();
        assert!(count > 0, "metadata boundary must be exercised");
        for skip in 0..count {
            let c = Client::notepad();
            let document = Rc::clone(&c.a);
            let at = Rc::new(Cell::new(None));
            let observed = Rc::clone(&at);
            c.host.on(boundary, skip, move || {
                observed.set(Some((
                    document.reads.get(),
                    document.writes.get(),
                    document.starts.get(),
                )));
                document.scope.set(IS_PRIVATE);
            });
            let eaten = down(&c);
            let (reads, writes, starts) = at.get().expect("scheduled metadata occurrence reached");
            assert_eq!(
                (c.a.reads.get(), c.a.writes.get(), c.a.starts.get()),
                (reads, writes, starts),
                "{boundary} occurrence {skip}"
            );
            assert_eq!(eaten, writes != 0);
            c.host.drained();
        }
    }
}

#[test]
fn advised_context_notifications_revoke_every_late_metadata_occurrence() {
    let baseline = Client::notepad();
    baseline.host.trace.borrow_mut().clear();
    assert!(down(&baseline));
    let trace = baseline.host.trace.borrow().clone();
    for boundary in ["GetWnd", "GetInputScopes", "Release.before"] {
        let count = trace.iter().filter(|entry| **entry == boundary).count();
        assert!(count > 0, "metadata boundary {boundary} must be exercised");
        for skip in 0..count {
            let c = Client::notepad();
            let document = Rc::clone(&c.a);
            let manager = Rc::clone(&c.manager);
            let other = c.context_b.cast::<windows_core::IUnknown>().unwrap();
            let at = Rc::new(Cell::new(None));
            let observed = Rc::clone(&at);
            c.host.on(boundary, skip, move || {
                observed.set(Some((
                    document.reads.get(),
                    document.writes.get(),
                    document.starts.get(),
                )));
                let retired = manager.top.replace(other.clone());
                drop(retired);
                let unknown = manager.thread_sink.borrow().clone().unwrap();
                let sink = unknown.cast::<ITfThreadMgrEventSink>().unwrap();
                let context = other.cast::<ITfContext>().unwrap();
                unsafe {
                    assert_eq!(
                        (sink.vtable().OnPushContext)(sink.as_raw(), context.as_raw()),
                        S_OK
                    );
                }
            });
            let eaten = down(&c);
            let (reads, writes, starts) = at.get().expect("scheduled metadata occurrence reached");
            assert_eq!(
                (c.a.reads.get(), c.a.writes.get(), c.a.starts.get()),
                (reads, writes, starts),
                "{boundary} occurrence {skip}"
            );
            assert_eq!(eaten, writes != 0);
            c.host.drained();
        }
    }
}
