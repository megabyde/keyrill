use crate::support::{client::Client, dll::Metadata, oracle};
use std::{cell::Cell, ffi::c_void, rc::Rc};
use windows::Win32::{
    Foundation::*,
    System::{Memory::*, Variant::*},
    UI::{TextServices::*, WindowsAndMessaging::*},
};
use windows_core::{HRESULT, Interface, w};

type Observe = unsafe extern "system" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    u32,
    *const u32,
    *mut Metadata,
) -> HRESULT;
struct Window(HWND);
impl Window {
    fn new(password: bool) -> Self {
        Self(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!(""),
                WS_POPUP | WINDOW_STYLE(if password { ES_PASSWORD as u32 } else { 0 }),
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        })
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0).unwrap();
        }
    }
}
fn run(c: &Client, alive: *const u32) -> Metadata {
    c.a.request_flags.set(TF_ES_SYNC | TF_ES_READ);
    let call: Observe = unsafe { c.api.export(b"KeyrillTestMetadata\0") };
    let mut output = Metadata::default();
    unsafe {
        assert_eq!(
            call(
                c.handle,
                c.context_a.as_raw(),
                c.thread.as_raw(),
                42,
                alive,
                &mut output
            ),
            S_OK
        );
    }
    c.host.check();
    output
}
fn unchanged(c: &Client) {
    oracle::document(c, "", 7);
    oracle::no_effects(c);
    assert_eq!(c.a.reads.get(), 0);
    for boundary in ["GetExtent", "ShiftStart", "EnumCompositions", "range.ACP"] {
        assert!(
            !c.host.trace.borrow().contains(&boundary),
            "unexpected {boundary}"
        );
    }
}
#[test]
fn unknown_notepad_metadata_has_scope_without_acp_or_editing_authority() {
    for scope in [IS_DEFAULT, IS_TEXT] {
        let window = Window::new(false);
        let c = Client::notepad_version([11, 2609, 12, 0]);
        c.a.view_window.set(window.0);
        c.a.scope.set(scope);
        let observed = run(&c, &1);
        assert_eq!(
            (
                observed.reason,
                observed.entered,
                observed.completed,
                observed.selection,
                observed.scope,
                observed.acp,
            ),
            (1, 1, 1, 1, if scope == IS_DEFAULT { 2 } else { 3 }, 0)
        );
        assert!(c.host.trace.borrow().contains(&"GetInputScopes"));
        unchanged(&c);
        c.a.request_flags.set(TF_ES_SYNC | TF_ES_READWRITE);
        assert!(!c.key(b'g' as u16, false));
        unchanged(&c);
    }
}

#[test]
fn metadata_refusals_are_read_only_and_preserve_exact_reason() {
    for case in 0..12 {
        let window = Window::new(case == 11);
        let c = Client::new();
        c.a.view_window.set(if case == 10 {
            HWND::default()
        } else {
            window.0
        });
        let reason = match case {
            1 => {
                c.a.acp.set(false);
                1
            }
            2 => {
                c.a.status.set(TF_SD_READONLY);
                5
            }
            3 => {
                c.a.disabled.set(true);
                6
            }
            4 => {
                c.a.scope.set(IS_PASSWORD);
                9
            }
            5 => {
                c.a.nulls.borrow_mut().push("GetAppProperty");
                8
            }
            6 => {
                c.a.scope_count.set(2);
                9
            }
            7 => {
                c.a.invoke.set(false);
                c.a.results
                    .borrow_mut()
                    .insert("RequestEditSession", E_ACCESSDENIED);
                12
            }
            8 => {
                c.a.invoke.set(false);
                c.a.results.borrow_mut().insert("session", TF_E_SYNCHRONOUS);
                13
            }
            9 => {
                c.a.invoke.set(false);
                14
            }
            11 => 15,
            _ => 1,
        };
        let observed = run(&c, &1);
        assert_eq!(observed.reason, reason, "case {case}: {observed:?}");
        assert_eq!(observed.acp, 0);
        if case == 1 {
            assert_eq!((observed.selection, observed.scope), (1, 2));
        }
        if [2, 3, 11].contains(&case) {
            assert!(!c.host.trace.borrow().contains(&"GetSelection"));
        }
        if (2..=6).contains(&case) {
            assert!(!c.host.trace.borrow().contains(&"range.ACP"));
        }
        if (7..=9).contains(&case) {
            let trace = c.host.trace.borrow().clone();
            assert_eq!(c.delayed(), E_FAIL);
            assert_eq!(*c.host.trace.borrow(), trace);
        }
        unchanged(&c);
    }
}
#[test]
fn metadata_all_compartments_preserve_variant_and_hresult_meaning() {
    for compartment in 0..3 {
        for case in 0..6 {
            let window = Window::new(false);
            let c = Client::new();
            c.a.view_window.set(window.0);
            let alive = Rc::new(Cell::new(1));
            let (result, kind, reason) = match case {
                0 => (S_FALSE, VT_EMPTY, 1),
                1 => (S_OK, VT_EMPTY, 1),
                2 => (S_FALSE, VT_I4, 3),
                3 => (E_UNEXPECTED, VT_EMPTY, 3),
                4 => (S_OK, VT_BOOL, 6),
                _ => (S_FALSE, VT_EMPTY, 2),
            };
            if compartment == 2 {
                c.manager.compartment_type.set(kind);
                c.manager.compartment_result.set(result);
            } else {
                let mut kinds = c.a.compartment_types.get();
                kinds[compartment] = kind;
                c.a.compartment_types.set(kinds);
                let mut results = c.a.compartment_results.get();
                results[compartment] = result;
                c.a.compartment_results.set(results);
            }
            if case == 5 {
                let flag = Rc::clone(&alive);
                c.host.on(
                    if compartment == 2 {
                        "thread.GetValue"
                    } else {
                        "compartment.GetValue"
                    },
                    if compartment == 2 { 0 } else { compartment },
                    move || flag.set(0),
                );
            }
            let observed = run(&c, alive.as_ptr());
            assert_eq!(
                observed.reason, reason,
                "compartment {compartment} case {case}"
            );
            if reason == 1 {
                assert_eq!(
                    (observed.selection, observed.scope, observed.acp),
                    (1, 2, 0)
                );
            } else {
                assert_eq!((observed.selection, observed.acp), (0, 0));
                assert!(!c.host.trace.borrow().contains(&"GetSelection"));
            }
            if reason == 3 {
                assert_eq!(observed.result, result.0 as u32);
            }
            unchanged(&c);
        }
    }
}
#[test]
fn metadata_liveness_is_rechecked_at_every_native_query() {
    for boundary in [
        "context.QI",
        "manager.GetFocus",
        "GetTop",
        "GetActiveView",
        "GetWnd",
        "GetStatus",
        "GetCompartment",
        "compartment.GetValue",
        "manager.QI",
        "thread.GetCompartment",
        "thread.GetValue",
        "GetSelection",
        "range.GetContext",
        "GetAppProperty",
        "scope.GetValue",
        "GetInputScopes",
    ] {
        let window = Window::new(false);
        let c = Client::new();
        c.a.view_window.set(window.0);
        let alive = Rc::new(Cell::new(1));
        let flag = Rc::clone(&alive);
        c.host.on(boundary, 0, move || flag.set(0));
        assert_eq!(run(&c, alive.as_ptr()).reason, 2, "boundary {boundary}");
        assert_eq!(alive.get(), 0);
        unchanged(&c);
    }
}
#[test]
fn metadata_duplicate_callback_cannot_overwrite_completed_observation() {
    let window = Window::new(false);
    let c = Client::new();
    c.a.view_window.set(window.0);
    c.a.retain_callback.set(true);
    let document = Rc::clone(&c.a);
    let host = Rc::clone(&c.host);
    c.host.on("GetSelection", 0, move || {
        let retained = document.retained.borrow().as_ref().unwrap().clone();
        let session = retained.cast::<ITfEditSession>().unwrap();
        let trace = host.trace.borrow().clone();
        unsafe {
            assert_eq!(
                (session.vtable().DoEditSession)(session.as_raw(), 123),
                E_FAIL
            );
        }
        assert_eq!(*host.trace.borrow(), trace);
    });
    assert_eq!(run(&c, &1).reason, 1);
    c.a.drain_retained();
    unchanged(&c);
}
#[test]
fn metadata_late_callback_never_reads_the_expired_caller_flag() {
    struct Flag(*mut u32);
    impl Drop for Flag {
        fn drop(&mut self) {
            unsafe {
                VirtualFree(self.0.cast(), 0, MEM_RELEASE).unwrap();
            }
        }
    }
    let window = Window::new(false);
    let c = Client::new();
    c.a.view_window.set(window.0);
    c.a.invoke.set(false);
    let flag =
        Flag(unsafe { VirtualAlloc(None, 4, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE).cast() });
    assert!(!flag.0.is_null());
    unsafe {
        flag.0.write(1);
    }
    assert_eq!(run(&c, flag.0).reason, 14);
    let mut previous = PAGE_PROTECTION_FLAGS::default();
    unsafe {
        VirtualProtect(flag.0.cast(), 4, PAGE_NOACCESS, &mut previous).unwrap();
    }
    let trace = c.host.trace.borrow().clone();
    assert_eq!(c.delayed(), E_FAIL);
    assert_eq!(*c.host.trace.borrow(), trace);
    c.a.drain_retained();
    unchanged(&c);
}
