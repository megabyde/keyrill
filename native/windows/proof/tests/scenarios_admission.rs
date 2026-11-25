use crate::support::{client::Client, oracle};
use std::rc::Rc;
use windows::Win32::{Foundation::*, System::Variant::*, UI::TextServices::*};
use windows_core::Interface;
fn no_admission(c: &Client) {
    oracle::document(c, "", 7);
    oracle::no_effects(c);
    assert_eq!(c.a.reads.get(), 0);
    assert!(!c.host.trace.borrow().contains(&"range.ACP"));
}

fn test_key_down(c: &Client, key: u8) -> bool {
    let sink = c.manager.key_sink.borrow().clone().unwrap().sink.clone();
    let mut eaten = windows_core::BOOL(1);
    unsafe {
        assert_eq!(
            (sink.vtable().OnTestKeyDown)(
                sink.as_raw(),
                c.context_a.as_raw(),
                WPARAM(key.into()),
                LPARAM(1),
                &mut eaten
            ),
            S_OK
        );
    }
    eaten.as_bool()
}

#[test]
fn early_key_test_leaves_native_typing_to_unrecognized_fields() {
    for class in ["Scintilla", "Edit"] {
        let c = Client::ordinary(class);
        for key in b"XYZ" {
            if !test_key_down(&c, *key) {
                c.a.native_text(&[u16::from(key.to_ascii_lowercase())]);
            }
        }
        oracle::document(&c, "xyz", 10);
        oracle::no_effects(&c);
        assert_eq!(c.a.reads.get(), 0);
        assert!(!c.host.trace.borrow().contains(&"RequestEditSession"));
    }

    let c = Client::notepad();
    assert!(test_key_down(&c, b'G'));
    // A file dialog in a supported process has a different focused field.
    c.host.field_facts.borrow_mut().as_mut().unwrap().focus = HWND::default();
    assert!(!test_key_down(&c, b'G'));
    no_admission(&c);
    assert!(!c.host.trace.borrow().contains(&"RequestEditSession"));

    for boundary in ["GetActiveView", "GetWnd", "capture"] {
        let c = Client::notepad();
        let sink = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
        let other = c.context_b.clone();
        c.host.on(boundary, 0, move || unsafe {
            assert_eq!(
                (sink.vtable().OnPushContext)(sink.as_raw(), other.as_raw()),
                S_OK
            );
        });
        assert!(!test_key_down(&c, b'G'), "{boundary}");
        c.host.drained();
        no_admission(&c);
        assert!(!c.host.trace.borrow().contains(&"RequestEditSession"));
    }
}
#[test]
fn explicit_ordinary_application_scopes_admit_one_owned_mark() {
    for scope in [IS_DEFAULT, IS_TEXT] {
        let c = Client::new();
        c.a.scope.set(scope);
        c.type_text("ghbdtn ");
        oracle::document(&c, "привет ", 14);
        assert_eq!(c.a.starts.get(), 1);
        assert_eq!(c.a.writes.get(), 7);
    }
}
#[test]
fn uncertain_application_scope_refuses_before_acp_or_text() {
    for version in [None, Some([11, 2607, 14, 0]), Some([11, 2609, 11, 0])] {
        for case in 0..22 {
            let c = version.map_or_else(Client::new, Client::notepad_version);
            let d = &c.a;
            match case {
                0 => {
                    d.results.borrow_mut().insert("GetAppProperty", S_FALSE);
                    d.nulls.borrow_mut().push("GetAppProperty");
                }
                1 | 2 => {
                    d.results
                        .borrow_mut()
                        .insert("GetAppProperty", if case == 1 { S_FALSE } else { E_FAIL });
                }
                3 => d.nulls.borrow_mut().push("GetAppProperty"),
                4 => {
                    d.results.borrow_mut().insert("scope.GetValue", S_FALSE);
                    d.scope_type.set(VT_EMPTY);
                }
                5 => {
                    d.results.borrow_mut().insert("scope.GetValue", E_FAIL);
                }
                6 => d.scope_type.set(VT_EMPTY),
                7 => d.scope_value_present.set(false),
                8 => d.scope_type.set(VT_I4),
                9 | 21 => {
                    d.results
                        .borrow_mut()
                        .insert("GetInputScopes", if case == 9 { E_FAIL } else { S_FALSE });
                }
                10 => d.scope_count.set(0),
                11 => d.nulls.borrow_mut().push("GetInputScopes"),
                12 => d.scope.set(InputScope(-1)),
                13 => d.scope.set(IS_EMAIL_SMTPEMAILADDRESS),
                14 => {
                    d.results.borrow_mut().insert("scope.GetValue", S_FALSE);
                }
                15 => d.scope.set(IS_PRIVATE),
                16 => d.scope.set(IS_PASSWORD),
                17 => d.scope.set(InputScope(i32::MAX)),
                18 => {
                    d.scope.set(IS_TEXT);
                    d.scope_count.set(2);
                    d.scope_second.set(IS_PRIVATE);
                }
                19 => d.scope_count.set(2),
                20 => {
                    d.scope.set(IS_TEXT);
                    d.scope_count.set(2);
                }
                _ => unreachable!(),
            };
            assert!(!c.key(b'g' as u16, false), "version{version:?}/case{case}");
            no_admission(&c);
        }
    }
}
#[test]
fn notepad_exact_unavailable_scope_has_bounded_authority() {
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        for unavailable in [false, true] {
            let c = Client::notepad_version(version);
            if unavailable {
                c.a.scope_type.set(VT_EMPTY);
                c.a.results.borrow_mut().insert("scope.GetValue", E_FAIL);
            }
            c.type_text("ghbdtn ");
            oracle::document(&c, "привет ", 14);
            assert_eq!(c.a.starts.get(), 1);
            assert_eq!(c.a.writes.get(), 7);
            assert_eq!(
                c.host.trace.borrow().contains(&"GetInputScopes"),
                !unavailable
            );
            let trace = c.host.trace.borrow();
            assert!(
                trace.iter().position(|s| *s == "scope.GetValue").unwrap()
                    < trace.iter().position(|s| *s == "range.ACP").unwrap()
            );
            drop(trace);
            assert!(c.key(b'x' as u16, false));
            oracle::document(&c, "привет x", 15);
        }
    }
    let c = Client::new();
    c.a.scope_type.set(VT_EMPTY);
    c.a.results.borrow_mut().insert("scope.GetValue", E_FAIL);
    assert!(!c.key(b'g' as u16, false));
    no_admission(&c);
}
#[test]
fn notepad_field_fact_mutations_refuse_before_acp() {
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        for unavailable in [false, true] {
            for case in 0..44 {
                let c = Client::notepad_version(version);
                if unavailable {
                    c.a.scope_type.set(VT_EMPTY);
                    c.a.results.borrow_mut().insert("scope.GetValue", E_FAIL);
                }
                {
                    let mut facts = c.host.field_facts.borrow_mut();
                    let f = facts.as_mut().unwrap();
                    match case {
                        0 => f.host.kind = 0,
                        1 => f.host.process += 1,
                        2 => f.host.thread += 1,
                        3 => f.process = 0,
                        4 => f.thread = 0,
                        5 => f.focus = HWND::default(),
                        6 => f.view = HWND::default(),
                        7 => f.foreground = HWND::default(),
                        8 => f.root = HWND::default(),
                        9 => f.field.style &= !0x40000000,
                        10 => f.field.style &= !4,
                        11 => f.field.style |= 0x20,
                        12 => f.field.style |= 0x800,
                        13 => f.field.thread += 1,
                        14 => f.parent.thread += 1,
                        15 => f.grandparent.handle = f.parent.handle,
                        16..=19 => f.host.version[case - 16] += 1,
                        _ => {
                            let node = (case - 20) / 8;
                            let field = match node {
                                0 => &mut f.field,
                                1 => &mut f.parent,
                                _ => &mut f.grandparent,
                            };
                            match (case - 20) % 8 {
                                0 => field.complete = 0,
                                1 => field.handle = HWND::default(),
                                2 => field.process += 1,
                                3 => field.thread = 0,
                                4 => field.control += 1,
                                5 => field.enabled = 0,
                                6 => field.style |= 0x08000000,
                                7 => field.class_name[0] = b'!' as u16,
                                _ => unreachable!(),
                            }
                        }
                    }
                }
                assert!(!c.key(b'g' as u16, false), "case{case}");
                no_admission(&c);
            }
        }
    }
}

#[test]
fn writable_policy_requires_exact_status_and_interfaces() {
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        for status in [S_FALSE, E_FAIL] {
            let c = Client::notepad_version(version);
            c.a.results.borrow_mut().insert("GetStatus", status);
            assert!(!c.key(b'g' as u16, false));
            assert!(!c.host.trace.borrow().contains(&"GetSelection"));
            no_admission(&c);
        }
        for thread in [false, true] {
            for case in 0..6 {
                let c = Client::notepad_version(version);
                let (results, nulls) = if thread {
                    (&c.manager.results, &c.manager.nulls)
                } else {
                    (&c.a.results, &c.a.nulls)
                };
                match case {
                    0 | 1 => {
                        results.borrow_mut().insert(
                            "compartment.QI",
                            if case == 0 { S_FALSE } else { E_NOINTERFACE },
                        );
                    }
                    2 => nulls.borrow_mut().push("compartment.QI"),
                    3 | 4 => {
                        results.borrow_mut().insert(
                            "GetCompartment",
                            if case == 3 { S_FALSE } else { E_ACCESSDENIED },
                        );
                    }
                    5 => nulls.borrow_mut().push("GetCompartment"),
                    _ => unreachable!(),
                };
                assert!(!c.key(b'g' as u16, false));
                assert!(!c.host.trace.borrow().contains(&"GetSelection"));
                no_admission(&c);
            }
        }
    }
}
#[test]
fn compartment_variant_signatures_distinguish_unset_from_malformed() {
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        let cases = [
            (S_OK, VT_EMPTY, true),
            (S_OK, VT_I4, true),
            (S_FALSE, VT_EMPTY, true),
            (S_FALSE, VT_I4, false),
            (S_OK, VT_BOOL, false),
            (S_OK, VT_UNKNOWN, false),
            (E_FAIL, VT_EMPTY, false),
            (E_ACCESSDENIED, VT_I4, false),
        ];
        for compartment in 0..3 {
            for (result, kind, allowed) in cases {
                let c = Client::notepad_version(version);
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
                assert_eq!(
                    c.key(b'g' as u16, false),
                    allowed,
                    "compartment{compartment},kind{kind:?},result{result:?}"
                );
                if allowed {
                    oracle::document(&c, "g", 8);
                    assert_eq!(c.a.starts.get(), 1);
                    assert_eq!(c.a.writes.get(), 1);
                } else {
                    assert!(!c.host.trace.borrow().contains(&"GetSelection"));
                    no_admission(&c);
                }
            }
        }
    }
}
#[test]
fn notepad_field_drift_blocks_boundary_and_owned_text_reads() {
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        for owned in [false, true] {
            let boundaries: &[&str] = if owned {
                &["GetRange", "GetExtent", "enumerator.Next"]
            } else {
                &[
                    "range.ACP",
                    "GetExtent",
                    "enumerator.Next",
                    "Clone",
                    "ShiftStart",
                ]
            };
            for &boundary in boundaries {
                for readonly in [false, true] {
                    let c = Client::notepad_version(version);
                    if owned {
                        c.type_text("g");
                    } else {
                        c.a.scope_type.set(VT_EMPTY);
                        c.a.results.borrow_mut().insert("scope.GetValue", E_FAIL);
                    }
                    let reads = c.a.reads.get();
                    let writes = c.a.writes.get();
                    let host = Rc::clone(&c.host);
                    c.host.on(boundary, 0, move || {
                        host.field_facts.borrow_mut().as_mut().unwrap().field.style |=
                            if readonly { 0x800 } else { 0x20 };
                    });
                    assert!(!c.key(if owned { b'h' } else { b'g' } as u16, false));
                    assert_eq!(c.a.reads.get(), reads);
                    assert_eq!(c.a.writes.get(), writes);
                    oracle::document(&c, if owned { "g" } else { "" }, if owned { 8 } else { 7 });
                }
            }
        }
    }
}
#[test]
fn notepad_selection_and_scope_failures_are_metadata_only() {
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        for case in 0..11 {
            let c = Client::notepad_version(version);
            c.a.scope_type.set(VT_EMPTY);
            c.a.results.borrow_mut().insert("scope.GetValue", E_FAIL);
            match case {
                0 => {
                    c.a.results.borrow_mut().insert("GetStatus", E_FAIL);
                }
                1 | 2 => {
                    c.a.results
                        .borrow_mut()
                        .insert("GetSelection", if case == 1 { E_FAIL } else { S_FALSE });
                }
                3 => c.a.nulls.borrow_mut().push("GetSelection"),
                4 | 5 => c.a.selection_fetched.set(if case == 4 { 0 } else { 2 }),
                6 => {
                    let mut s = c.a.style.get();
                    s.fInterimChar = true.into();
                    c.a.style.set(s);
                }
                7 => {
                    let mut s = c.a.style.get();
                    s.ase = TfActiveSelEnd(99);
                    c.a.style.set(s);
                }
                8 => c.a.range_context.set(c.context_b.as_raw()),
                9 => {
                    c.a.scope_type.set(VT_UNKNOWN);
                    c.a.results.borrow_mut().remove("scope.GetValue");
                    c.a.results.borrow_mut().insert("scope.QI", S_FALSE);
                }
                10 => {
                    let sink = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
                    let other = c.context_b.as_raw() as usize;
                    c.host.on("GetSelection", 0, move || unsafe {
                        assert_eq!(
                            (sink.vtable().OnPushContext)(sink.as_raw(), other as *mut _),
                            S_OK
                        );
                    });
                }
                _ => unreachable!(),
            };
            assert!(!c.key(b'g' as u16, false), "case{case}");
            no_admission(&c);
        }
    }
}
#[test]
fn notepad_native_policy_uncertainty_has_no_admission_effects() {
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        for case in 0..18 {
            let c = Client::notepad_version(version);
            match case {
                0 => c.a.status.set(TF_SD_READONLY),
                1 => c.a.disabled.set(true),
                2 => c.a.empty_disabled.set(true),
                3 => c.manager.disabled.set(true),
                4..=9 => {
                    let index = (case - 4) % 3;
                    let result = if case < 7 { E_ACCESSDENIED } else { E_FAIL };
                    if index == 2 {
                        c.manager.compartment_result.set(result);
                    } else {
                        let mut r = c.a.compartment_results.get();
                        r[index] = result;
                        c.a.compartment_results.set(r);
                    }
                }
                10..=15 => {
                    let index = (case - 10) % 3;
                    let kind = if case < 13 { VT_UNKNOWN } else { VT_BOOL };
                    if index == 2 {
                        c.manager.compartment_type.set(kind);
                    } else {
                        let mut kinds = c.a.compartment_types.get();
                        kinds[index] = kind;
                        c.a.compartment_types.set(kinds);
                    }
                }
                16 | 17 => {
                    c.a.scope_type.set(VT_EMPTY);
                    c.a.results.borrow_mut().insert(
                        "scope.GetValue",
                        if case == 16 { E_FAIL } else { E_ACCESSDENIED },
                    );
                    if case == 16 {
                        let host = Rc::clone(&c.host);
                        c.host.on("scope.GetValue", 0, move || {
                            host.field_facts.borrow_mut().as_mut().unwrap().focus = HWND::default()
                        });
                    }
                }
                _ => unreachable!(),
            };
            assert!(!c.key(b'g' as u16, false), "case{case}");
            no_admission(&c);
        }
    }
}
#[test]
fn exceptional_notepad_scope_requires_fresh_exact_failure() {
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        for case in 0..5 {
            let c = Client::notepad_version(version);
            c.a.scope_type.set(VT_EMPTY);
            c.a.results.borrow_mut().insert(
                "scope.GetValue",
                match case {
                    0 => S_OK,
                    1 => S_FALSE,
                    2 => E_ACCESSDENIED,
                    _ => E_FAIL,
                },
            );
            if case == 3 {
                c.a.results.borrow_mut().insert("GetAppProperty", S_FALSE);
            }
            if case == 4 {
                let sink = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
                let other = c.context_b.as_raw() as usize;
                c.host.on("scope.GetValue", 0, move || unsafe {
                    assert_eq!(
                        (sink.vtable().OnPushContext)(sink.as_raw(), other as *mut _),
                        S_OK
                    );
                });
            }
            assert!(!c.key(b'g' as u16, false));
            no_admission(&c);
        }
    }
}
#[test]
fn notepad_executor_refusals_expire_retained_callbacks() {
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        for case in 0..9 {
            let c = Client::notepad_version(version);
            c.a.retain_callback.set(true);
            c.a.scope_type.set(VT_EMPTY);
            c.a.results.borrow_mut().insert("scope.GetValue", E_FAIL);
            match case {
                0 => c.a.acp.set(false),
                1 => {
                    c.a.invoke.set(false);
                    c.a.results
                        .borrow_mut()
                        .insert("RequestEditSession", E_ACCESSDENIED);
                }
                2 | 8 => {
                    c.a.invoke.set(false);
                    if case == 8 {
                        c.a.results.borrow_mut().insert("session", TF_E_SYNCHRONOUS);
                    }
                }
                3 => c.a.foreign.set(true),
                4 => c.a.reject_composition.set(true),
                5 => {
                    let host = Rc::clone(&c.host);
                    c.host.on("capture", 2, move || {
                        host.field_facts
                            .borrow_mut()
                            .as_mut()
                            .unwrap()
                            .parent
                            .handle = HWND(3usize as *mut _)
                    });
                }
                6 | 7 => {
                    if case == 7 {
                        c.a.scope_type.set(VT_UNKNOWN);
                        c.a.results.borrow_mut().remove("scope.GetValue");
                    }
                    let sink = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
                    let other = c.context_b.as_raw() as usize;
                    c.host.on(
                        if case == 6 {
                            "range.GetContext"
                        } else {
                            "GetInputScopes"
                        },
                        0,
                        move || unsafe {
                            assert_eq!(
                                (sink.vtable().OnPushContext)(sink.as_raw(), other as *mut _),
                                S_OK
                            );
                        },
                    );
                }
                _ => unreachable!(),
            };
            assert!(!c.key(b'g' as u16, false), "case{case}");
            oracle::document(&c, "", 7);
            assert_eq!(c.a.writes.get(), 0);
            assert!(c.a.active.borrow().is_none());
            if c.a.retained.borrow().is_some() {
                let trace = c.host.trace.borrow().clone();
                let starts = c.a.starts.get();
                assert_eq!(c.delayed(), E_FAIL);
                assert_eq!(*c.host.trace.borrow(), trace);
                assert_eq!(c.a.starts.get(), starts);
                assert_eq!(c.a.writes.get(), 0);
                oracle::document(&c, "", 7);
            }
        }
    }
}

#[test]
fn unknown_notepad_versions_refuse_even_explicit_ordinary_scope() {
    let keyboard = Keyboard::new();
    for version in [
        [0; 4],
        [11, 2607, 13, 0],
        [11, 2607, 15, 0],
        [11, 2609, 10, 0],
        [11, 2609, 12, 0],
        [11, 2609, 11, 1],
    ] {
        for scope in [Some(IS_DEFAULT), Some(IS_TEXT), None] {
            let c = Client::notepad_version(version);
            if let Some(scope) = scope {
                c.a.scope.set(scope);
            } else {
                c.a.scope_type.set(VT_EMPTY);
                c.a.results.borrow_mut().insert("scope.GetValue", E_FAIL);
            }
            assert!(
                !keyboard.down(&c, b'G'),
                "version {version:?}, scope {scope:?}"
            );
            no_admission(&c);
            assert!(c.a.active.borrow().is_none());
            assert!(!c.host.trace.borrow().contains(&"GetSelection"));
            c.a.native_text(&[b'g' as u16]);
            oracle::document(&c, "g", 8);
            oracle::no_effects(&c);
        }
    }
}

struct Keyboard([u8; 256]);
impl Keyboard {
    fn new() -> Self {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        let mut previous = [0; 256];
        unsafe {
            assert!(crate::support::keyboard_state(&mut previous));
            SetKeyboardState(&[0; 256]).unwrap();
        }
        Self(previous)
    }
    fn down(&self, c: &Client, key: u8) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        let advised = c.manager.key_sink.borrow().clone().unwrap();
        let scan = unsafe { MapVirtualKeyW(key as u32, MAPVK_VK_TO_VSC) } as isize;
        let mut eaten = false.into();
        unsafe {
            assert_eq!(
                (advised.sink.vtable().OnKeyDown)(
                    advised.sink.as_raw(),
                    c.context_a.as_raw(),
                    WPARAM(key as usize),
                    LPARAM((scan << 16) | 1),
                    &mut eaten
                ),
                S_OK
            );
        }
        c.host.check();
        eaten.as_bool()
    }
}
impl Drop for Keyboard {
    fn drop(&mut self) {
        unsafe { windows::Win32::UI::Input::KeyboardAndMouse::SetKeyboardState(&self.0).unwrap() };
    }
}

#[test]
fn generic_default_scope_has_no_sensitive_field_authority() {
    let keyboard = Keyboard::new();
    let c = Client::ordinary("Chrome_WidgetWin_1");
    c.a.scope.set(IS_DEFAULT);
    let eaten = keyboard.down(&c, b'G');
    assert_eq!(
        (
            eaten,
            c.a.starts.get(),
            c.a.writes.get(),
            c.a.reads.get() > 0,
            c.host.trace.borrow().contains(&"range.ACP"),
        ),
        (false, 0, 0, false, false),
        "ordinary scope cannot establish sensitive-field authority"
    );
    no_admission(&c);
    assert!(c.a.active.borrow().is_none());
    assert!(!c.host.trace.borrow().contains(&"GetSelection"));
    c.host.drained();
}

#[test]
fn generic_native_classes_pass_normal_keys_without_document_access() {
    let keyboard = Keyboard::new();
    for class in [
        "ArbitraryTsfHost",
        "Chrome_WidgetWin_1",
        "Chrome_RenderWidgetHostHWND",
        "MozillaWindowClass",
        "_WwG",
        "CASCADIA_HOSTING_WINDOW_CLASS",
    ] {
        for scope in [IS_DEFAULT, IS_TEXT] {
            let c = Client::ordinary(class);
            c.a.scope.set(scope);
            c.host
                .field_facts
                .borrow_mut()
                .as_mut()
                .unwrap()
                .field
                .style |= 0x820;
            assert!(!keyboard.down(&c, b'G'), "class {class}, scope {scope:?}");
            no_admission(&c);
            assert!(!c.host.trace.borrow().contains(&"GetSelection"));
            assert!(c.a.active.borrow().is_none());
            c.a.native_text(&[b'g' as u16]);
            oracle::document(&c, "g", 8);
            oracle::no_effects(&c);
        }
    }
}

#[test]
fn generic_native_identity_and_sensitive_scope_refuse_without_text() {
    let keyboard = Keyboard::new();
    for case in 0..27 {
        let c = Client::ordinary("ArbitraryTsfHost");
        {
            let mut facts = c.host.field_facts.borrow_mut();
            let f = facts.as_mut().unwrap();
            match case {
                0 => f.view = HWND::default(),
                1 => f.focus = HWND::default(),
                2 => f.field.handle = HWND::default(),
                3 => f.field.complete = 0,
                4 => f.field.process += 1,
                5 => f.field.thread += 1,
                6 => f.field.enabled = 0,
                7 => f.field.style |= 0x08000000,
                8 => f.root = HWND::default(),
                9 => f.foreground = HWND::default(),
                10 => f.proof_root.handle = HWND::default(),
                11 => f.proof_root.complete = 0,
                12 => f.proof_root.process += 1,
                13 => f.proof_root.thread = 0,
                14 => f.proof_root.enabled = 0,
                15 => f.proof_root.style |= 0x08000000,
                16 => c.a.scope.set(IS_PASSWORD),
                17 => c.a.scope.set(IS_PRIVATE),
                18 => c.a.status.set(TF_SD_READONLY),
                19 => c.a.disabled.set(true),
                20 => c.a.empty_disabled.set(true),
                21 => c.manager.disabled.set(true),
                22 => {
                    c.manager.top.replace(c.context_b.cast().unwrap());
                }
                23 => c.a.range_context.set(c.context_b.as_raw()),
                24 => f.host.process += 1,
                25 => f.host.thread += 1,
                26 => f.host.started = 0,
                _ => unreachable!(),
            }
        }
        assert!(!keyboard.down(&c, b'G'), "case {case}");
        no_admission(&c);
    }
    for class in ["EDIT", "RichEdit20W", "RICHEDIT50W"] {
        for style in [0x20, 0x800] {
            let c = Client::ordinary(class);
            c.host
                .field_facts
                .borrow_mut()
                .as_mut()
                .unwrap()
                .field
                .style |= style;
            assert!(!keyboard.down(&c, b'G'));
            no_admission(&c);
        }
    }
}

#[test]
fn bounded_scope_and_focus_changes_revoke_range_permission() {
    let keyboard = Keyboard::new();
    for owned in [false, true] {
        for boundary in [
            "GetSelection",
            "range.GetContext",
            "GetInputScopes",
            "range.ACP",
            "GetExtent",
            "enumerator.Next",
            "Clone",
            "ShiftStart",
        ] {
            if owned && matches!(boundary, "Clone" | "ShiftStart") {
                continue;
            }
            for change in 0..3 {
                let c = Client::notepad();
                if owned {
                    assert!(keyboard.down(&c, b'G'));
                }
                let reads = c.a.reads.get();
                let writes = c.a.writes.get();
                let document = Rc::clone(&c.a);
                let manager = Rc::clone(&c.manager);
                let host = Rc::clone(&c.host);
                let other = c.context_b.cast::<windows_core::IUnknown>().unwrap();
                c.host.on(boundary, 0, move || match change {
                    0 => document.scope.set(IS_PRIVATE),
                    1 => host.field_facts.borrow_mut().as_mut().unwrap().focus = HWND::default(),
                    _ => {
                        manager.top.replace(other.clone());
                    }
                });
                assert!(
                    !keyboard.down(&c, if owned { b'H' } else { b'G' }),
                    "owned {owned}, {boundary}, change {change}"
                );
                assert_eq!(
                    c.a.reads.get(),
                    reads,
                    "owned {owned}, {boundary}, change {change}"
                );
                assert_eq!(c.a.writes.get(), writes);
                oracle::document(&c, if owned { "g" } else { "" }, if owned { 8 } else { 7 });
                c.host.drained();
            }
        }
    }
}

#[test]
fn bounded_exclusion_after_write_stops_readback_and_replay() {
    let keyboard = Keyboard::new();
    let c = Client::notepad();
    assert!(keyboard.down(&c, b'G'));
    let reads = c.a.reads.get();
    let document = Rc::clone(&c.a);
    let at_write = Rc::new(std::cell::Cell::new(0));
    let captured = Rc::clone(&at_write);
    c.host.on("SetText.after", 0, move || {
        captured.set(document.reads.get());
        document.scope.set(IS_PRIVATE);
    });
    assert!(keyboard.down(&c, b'H'));
    assert!(at_write.get() >= reads);
    assert_eq!(c.a.reads.get(), at_write.get());
    assert_eq!(c.a.writes.get(), 2);
    oracle::document(&c, "gh", 8);
    assert!(!keyboard.down(&c, b'J'));
    assert_eq!(c.a.writes.get(), 2);
    assert_eq!(c.a.reads.get(), at_write.get());
    c.host.drained();
}

#[test]
fn bounded_native_failures_preserve_content_free_refusal_evidence() {
    use crate::support::dll::snapshot;
    let keyboard = Keyboard::new();
    for boundary in [
        "GetFocus",
        "GetTop",
        "GetActiveView",
        "GetWnd",
        "GetStatus",
        "GetAppProperty",
        "scope.GetValue",
        "GetInputScopes",
    ] {
        for result in [S_FALSE, E_FAIL] {
            let c = Client::notepad();
            if matches!(boundary, "GetFocus" | "GetTop") {
                c.manager.results.borrow_mut().insert(boundary, result);
            } else {
                c.a.results.borrow_mut().insert(boundary, result);
            }
            assert!(!keyboard.down(&c, b'G'), "{boundary}, {result:?}");
            no_admission(&c);
            let observed = snapshot(&c.service);
            assert_eq!(observed.first[0], 1);
            assert_eq!(observed.first[2], 1);
            assert_eq!(observed.first[3], result.0 as u32);
            assert_eq!(observed.counts[8], 0);
        }
    }
    for boundary in ["GetFocus", "GetTop", "GetActiveView", "GetAppProperty"] {
        let c = Client::notepad();
        if matches!(boundary, "GetFocus" | "GetTop") {
            c.manager.nulls.borrow_mut().push(boundary);
        } else {
            c.a.nulls.borrow_mut().push(boundary);
        }
        assert!(!keyboard.down(&c, b'G'), "null {boundary}");
        no_admission(&c);
        let observed = snapshot(&c.service);
        assert_eq!(observed.first[0], 1);
        assert_eq!(observed.first[2], 8);
        assert_eq!(observed.first[3], 0);
    }
    let c = Client::new();
    c.a.scope_type.set(VT_EMPTY);
    c.a.results.borrow_mut().insert("scope.GetValue", E_FAIL);
    assert!(!keyboard.down(&c, b'G'));
    no_admission(&c);
    assert_eq!(snapshot(&c.service).first, [1, 18, 1, E_FAIL.0 as u32]);
}

#[test]
fn bounded_reentrant_context_callbacks_revoke_pending_admission() {
    let keyboard = Keyboard::new();
    for boundary in [
        "manager.GetFocus",
        "GetTop",
        "GetActiveView",
        "GetWnd",
        "capture",
        "GetStatus",
        "GetCompartment",
        "compartment.GetValue",
        "thread.GetValue",
        "GetSelection",
        "range.GetContext",
        "GetAppProperty",
        "scope.GetValue",
        "GetInputScopes",
    ] {
        let c = Client::notepad();
        let sink = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
        let other = c.context_b.clone();
        c.host.on(boundary, 0, move || unsafe {
            assert_eq!(
                (sink.vtable().OnPushContext)(sink.as_raw(), other.as_raw()),
                S_OK
            );
        });
        assert!(!keyboard.down(&c, b'G'), "{boundary}");
        no_admission(&c);
        c.host.drained();
    }
}
