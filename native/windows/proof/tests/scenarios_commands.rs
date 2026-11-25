use crate::support::{client::Client, dll::snapshot, document::Span, oracle};
use std::{cell::Cell, rc::Rc};
use windows::Win32::System::Ole::CONNECT_E_NOCONNECTION;
use windows::Win32::{
    Foundation::*,
    UI::{Input::KeyboardAndMouse::*, TextServices::*},
};
use windows_core::{BOOL, GUID, Interface};

const MANUAL: GUID = GUID::from_u128(0xd7abce42_c8a9_4a43_990a_4a8d6c011ebb);
const REVERSE: GUID = GUID::from_u128(0x16531b70_516e_4dfb_9f18_cb760de590b4);
const OBSERVER: GUID = GUID::from_u128(0x997f7561_053d_475a_abb9_b11fcf16bb48);

struct Keyboard {
    previous: [u8; 256],
    state: [u8; 256],
}
impl Keyboard {
    fn new() -> Self {
        let mut previous = [0; 256];
        assert!(crate::support::keyboard_state(&mut previous));
        let keyboard = Self {
            previous,
            state: [0; 256],
        };
        unsafe {
            SetKeyboardState(&keyboard.state).unwrap();
        }
        keyboard
    }
    fn set(&mut self, key: VIRTUAL_KEY, value: u8) {
        self.state[key.0 as usize] = value;
        for (both, left, right) in [
            (VK_CONTROL, VK_LCONTROL, VK_RCONTROL),
            (VK_MENU, VK_LMENU, VK_RMENU),
            (VK_SHIFT, VK_LSHIFT, VK_RSHIFT),
        ] {
            self.state[both.0 as usize] =
                self.state[left.0 as usize] | self.state[right.0 as usize];
        }
        unsafe {
            SetKeyboardState(&self.state).unwrap();
        }
    }
    fn chord(&mut self, c: &Client, key: u16) -> bool {
        let before = document_access(c);
        self.set(VK_LCONTROL, 0x80);
        assert!(!down(c, VK_CONTROL.0, 1));
        self.set(VK_LMENU, 0x80);
        assert!(!down(c, VK_MENU.0, 1));
        assert_eq!(document_access(c), before);
        let eaten = down(c, key, 1);
        assert!(!release(c, true, key, 0xc0000001, false));
        self.set(VK_LMENU, 0);
        assert!(!release(c, true, VK_MENU.0, 0xc0000001, false));
        self.set(VK_LCONTROL, 0);
        assert!(!release(c, true, VK_CONTROL.0, 0xc0000001, false));
        eaten
    }
    fn command(&mut self) {
        self.set(VK_LCONTROL, 0x80);
        self.set(VK_LMENU, 0x80);
    }
    fn witness(&mut self, c: &Client) {
        self.set(VK_LMENU, 0x80);
        let before = document_access(c);
        assert!(!preserved(c, OBSERVER));
        assert_eq!(document_access(c), before);
    }
}
impl Drop for Keyboard {
    fn drop(&mut self) {
        unsafe {
            SetKeyboardState(&self.previous).unwrap();
        }
    }
}
fn sink(c: &Client) -> ITfKeyEventSink {
    c.service.cast().unwrap()
}
fn down(c: &Client, key: u16, repeat: isize) -> bool {
    let sink = sink(c);
    let scan = unsafe { MapVirtualKeyW(key as u32, MAPVK_VK_TO_VSC) } as isize;
    let mut eaten = BOOL(1);
    unsafe {
        assert_eq!(
            (sink.vtable().OnKeyDown)(
                sink.as_raw(),
                c.context_a.as_raw(),
                WPARAM(key as usize),
                LPARAM((scan << 16) | repeat),
                &mut eaten
            ),
            S_OK
        );
    }
    c.host.check();
    eaten.as_bool()
}
fn preserved(c: &Client, id: GUID) -> bool {
    let sink = sink(c);
    let mut eaten = BOOL(1);
    unsafe {
        assert_eq!(
            (sink.vtable().OnPreservedKey)(sink.as_raw(), c.context_a.as_raw(), &id, &mut eaten),
            S_OK
        );
    }
    c.host.check();
    eaten.as_bool()
}
fn document_access(c: &Client) -> [u32; 11] {
    [
        c.a.reads.get(),
        c.a.writes.get(),
        c.a.starts.get(),
        c.a.ends.get(),
        c.a.request.get(),
        c.b.reads.get(),
        c.b.writes.get(),
        c.b.starts.get(),
        c.b.ends.get(),
        c.b.request.get(),
        c.host
            .trace
            .borrow()
            .iter()
            .filter(|name| **name == "GetSelection")
            .count() as u32,
    ]
}
fn release(c: &Client, actual: bool, key: u16, flags: u32, other: bool) -> bool {
    let before = document_access(c);
    let sink = sink(c);
    let callback = if actual {
        sink.vtable().OnKeyUp
    } else {
        sink.vtable().OnTestKeyUp
    };
    let mut eaten = BOOL(1);
    let context = if other { &c.context_b } else { &c.context_a };
    unsafe {
        assert_eq!(
            callback(
                sink.as_raw(),
                context.as_raw(),
                WPARAM(key as usize),
                LPARAM(flags as isize),
                &mut eaten
            ),
            S_OK
        );
    }
    assert_eq!(
        document_access(c),
        before,
        "release must never access either document"
    );
    c.host.check();
    eaten.as_bool()
}
fn alt_release(c: &Client, actual: bool) -> bool {
    release(c, actual, VK_MENU.0, 0xc0000001, false)
}
fn composition(c: &Client) -> usize {
    c.a.active
        .borrow()
        .as_ref()
        .expect("owned composition")
        .pointer as usize
}
fn focus(c: &Client, foreground: bool) {
    let sink = sink(c);
    unsafe {
        assert_eq!(
            (sink.vtable().OnSetFocus)(sink.as_raw(), foreground.into()),
            S_OK
        );
    }
}
fn stopped(c: &Client) -> bool {
    let mut result = 0;
    unsafe {
        assert_eq!((c.api.stopped)(c.handle, &mut result), S_OK);
    }
    result != 0
}
fn terminate(c: &Client) {
    let pointer = c.a.active.borrow().as_ref().unwrap().pointer;
    let sink: ITfCompositionSink = c.service.cast().unwrap();
    unsafe {
        assert_eq!(
            (sink.vtable().OnCompositionTerminated)(sink.as_raw(), 123, pointer),
            S_OK
        );
    }
}

#[test]
fn command_key_sequences_conversion_idle_and_receipt() {
    let c = Client::new();
    let mut keyboard = Keyboard::new();
    c.type_text("ghbdtn");
    let mark = composition(&c);
    assert!(keyboard.chord(&c, b'K' as u16));
    oracle::document(&c, "привет", 13);
    assert_eq!(composition(&c), mark);
    assert!(keyboard.chord(&c, b'Z' as u16));
    oracle::document(&c, "ghbdtn", 13);
    assert_eq!(composition(&c), mark);
    for key in *b"KZ" {
        let c = Client::new();
        assert!(keyboard.chord(&c, key as u16));
        oracle::document(&c, "", 7);
        oracle::no_effects(&c);
        c.type_text("hello");
        if key == b'Z' {
            let writes = c.a.writes.get();
            assert!(keyboard.chord(&c, key as u16));
            assert_eq!(c.a.writes.get(), writes);
            oracle::document(&c, "hello", 12);
            assert!(c.a.active.borrow().is_some());
        }
    }
    let c = Client::new();
    c.type_text("ghbdtn ");
    assert!(keyboard.chord(&c, b'Z' as u16));
    oracle::document(&c, "ghbdtn ", 14);
    c.type_text("x");
    let writes = c.a.writes.get();
    assert!(keyboard.chord(&c, b'Z' as u16));
    assert_eq!(c.a.writes.get(), writes);
    oracle::document(&c, "ghbdtn x", 15);
}

#[test]
fn command_key_sequences_modifier_and_special_key_matrix() {
    for extra in [
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LWIN,
        VK_RWIN,
        VK_RMENU,
        VK_RCONTROL,
    ] {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        keyboard.set(extra, 0x80);
        assert!(!keyboard.chord(&c, b'K' as u16));
        assert!(c.a.active.borrow().is_none());
        oracle::document(&c, "ghbdtn", 13);
    }
    for key in [VK_BACK, VK_ESCAPE] {
        for shift in [false, true] {
            let c = Client::new();
            let mut keyboard = Keyboard::new();
            c.type_text("ghbdtn");
            keyboard.set(VK_LSHIFT, if shift { 0x80 } else { 0 });
            let edits = !shift && key == VK_BACK;
            assert_eq!(down(&c, key.0, 1), edits);
            assert_eq!(c.a.active.borrow().is_some(), edits);
            oracle::document(
                &c,
                if edits { "ghbdt" } else { "ghbdtn" },
                if edits { 12 } else { 13 },
            );
        }
    }
    for altgr in [false, true] {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        keyboard.set(VK_LCONTROL, 0x80);
        assert!(!down(&c, VK_CONTROL.0, 1));
        keyboard.set(if altgr { VK_RMENU } else { VK_LMENU }, 0x80);
        assert!(!down(&c, VK_MENU.0, 1));
        assert!(c.a.active.borrow().is_some());
        assert!(!down(&c, b'K' as u16, if altgr { 1 } else { 2 }));
        assert!(c.a.active.borrow().is_none());
        oracle::document(&c, "ghbdtn", 13);
    }
}

#[test]
fn command_key_sequences_failure_and_exclusion_matrix() {
    for failure in 0..5 {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        match failure {
            0 => {
                c.a.invoke.set(false);
                c.a.results
                    .borrow_mut()
                    .insert("RequestEditSession", TF_E_SYNCHRONOUS);
            }
            1 | 3 => {
                c.a.results
                    .borrow_mut()
                    .insert("RequestEditSession", E_FAIL);
            }
            2 => {
                c.a.results.borrow_mut().insert("session", E_FAIL);
            }
            _ => {
                let sink = sink(&c);
                c.host.on("GetSelection", 0, move || unsafe {
                    assert_eq!(
                        (sink.vtable().OnSetFocus)(sink.as_raw(), false.into()),
                        S_OK
                    );
                });
            }
        }
        assert!(!keyboard.chord(&c, b'K' as u16));
        oracle::document(&c, "", 7);
        oracle::no_effects(&c);
    }
    for failure in 0..4 {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        let writes = c.a.writes.get();
        if failure == 0 {
            c.a.selection.set(Span {
                start: 12,
                length: 0,
            });
        }
        if failure == 1 {
            c.a.results.borrow_mut().insert("SetText", E_FAIL);
        }
        if failure >= 2 {
            let sink = sink(&c);
            let context = c.context_a.clone();
            c.host.on(
                if failure == 2 {
                    "GetRange"
                } else {
                    "SetText.after"
                },
                0,
                move || unsafe {
                    let mut eaten = BOOL(1);
                    assert_eq!(
                        (sink.vtable().OnKeyDown)(
                            sink.as_raw(),
                            context.as_raw(),
                            WPARAM(VK_SHIFT.0 as usize),
                            LPARAM(1),
                            &mut eaten
                        ),
                        S_OK
                    );
                    assert!(!eaten.as_bool());
                },
            );
        }
        let attempted = failure == 1 || failure == 3;
        assert_eq!(keyboard.chord(&c, b'K' as u16), attempted);
        assert_eq!(c.a.writes.get(), writes + u32::from(attempted));
        assert!(stopped(&c));
        oracle::document(
            &c,
            if attempted { "привет" } else { "ghbdtn" },
            if failure == 0 { 12 } else { 13 },
        );
        assert!(!down(&c, b'X' as u16, 1));
        assert_eq!(c.a.writes.get(), writes + u32::from(attempted));
    }
    for scope in [IS_PASSWORD, IS_EMAIL_SMTPEMAILADDRESS] {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.a.scope.set(scope);
        assert!(!keyboard.chord(&c, b'K' as u16));
        assert_eq!(c.a.reads.get(), 0);
        oracle::document(&c, "", 7);
        oracle::no_effects(&c);
    }
}

#[test]
fn command_retention_regression_and_release_order_matrix() {
    for ordinary in [false, true] {
        for ctrl_first in [false, true] {
            let c = Client::new();
            let mut keyboard = Keyboard::new();
            c.type_text("ghbdtn");
            let mark = composition(&c);
            for (key, id, word) in [(b'K', MANUAL, "привет"), (b'Z', REVERSE, "ghbdtn")] {
                keyboard.witness(&c);
                assert!(!alt_release(&c, false));
                keyboard.set(VK_LCONTROL, 0x80);
                let writes = c.a.writes.get();
                assert!(if ordinary {
                    down(&c, key as u16, 1)
                } else {
                    preserved(&c, id)
                });
                oracle::document(&c, word, 13);
                assert_eq!(c.a.writes.get(), writes + 1);
                assert_eq!(composition(&c), mark);
                if key == b'Z' {
                    assert!(preserved(&c, REVERSE));
                    assert!(preserved(&c, REVERSE));
                    assert_eq!(c.a.writes.get(), writes + 1);
                }
                assert!(!release(&c, true, key as u16, 0xc0000001, false));
                if ctrl_first {
                    keyboard.set(VK_LCONTROL, 0);
                    assert!(!release(&c, true, VK_CONTROL.0, 0xc0000001, false));
                }
                keyboard.set(VK_LMENU, 0);
                assert!(!release(&c, false, VK_RMENU.0, 0xc1000001, false));
                assert!(alt_release(&c, false));
                assert!(alt_release(&c, false));
                assert!(alt_release(&c, true));
                assert!(!alt_release(&c, true));
                assert!(!alt_release(&c, false));
                keyboard.set(VK_LCONTROL, 0);
                assert!(!release(&c, true, VK_CONTROL.0, 0xc0000001, false));
                keyboard.witness(&c);
                keyboard.set(VK_LMENU, 0);
                assert!(!alt_release(&c, true));
                oracle::document(&c, word, 13);
            }
        }
    }
}

#[test]
fn command_retention_missing_observer_host_contract_boundary() {
    let c = Client::new();
    let mut keyboard = Keyboard::new();
    c.type_text("ghbdtn");
    keyboard.witness(&c);
    keyboard.set(VK_LCONTROL, 0x80);
    assert!(preserved(&c, MANUAL));
    assert!(alt_release(&c, false));
    keyboard.set(VK_LMENU, 0);
    keyboard.set(VK_LMENU, 0x80);
    keyboard.set(VK_LMENU, 0);
    assert!(alt_release(&c, true));
    oracle::document(&c, "привет", 13);
}

#[test]
fn command_retention_focus_gain_direct_and_reentrant() {
    for reentrant in [false, true] {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        let mark = composition(&c);
        keyboard.witness(&c);
        keyboard.set(VK_LCONTROL, 0x80);
        assert!(preserved(&c, MANUAL));
        if reentrant {
            let sink = sink(&c);
            c.host.on("GetActiveProfile", 0, move || unsafe {
                assert_eq!((sink.vtable().OnSetFocus)(sink.as_raw(), true.into()), S_OK);
            });
        } else {
            focus(&c, true);
        }
        keyboard.set(VK_LMENU, 0);
        assert!(!alt_release(&c, true));
        assert_eq!(composition(&c), mark);
        oracle::document(&c, "привет", 13);
    }
}

#[test]
fn preserved_dispatch_wrong_modifiers_do_not_touch_owned_text() {
    for extra in [
        Some(VK_LSHIFT),
        Some(VK_RSHIFT),
        Some(VK_RCONTROL),
        Some(VK_RMENU),
        Some(VK_LWIN),
        Some(VK_RWIN),
        None,
        Some(VK_LCONTROL),
        Some(VK_LMENU),
    ] {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        keyboard.command();
        if let Some(extra) = extra {
            keyboard.set(
                extra,
                if extra == VK_LCONTROL || extra == VK_LMENU {
                    0
                } else {
                    0x80
                },
            );
        } else {
            keyboard.set(VK_LMENU, 0);
            keyboard.set(VK_RMENU, 0x80);
        }
        let before = document_access(&c);
        let diagnostic_before = format!("{:?}", snapshot(&c.service));
        let mark = composition(&c);
        assert!(!preserved(&c, MANUAL));
        assert!(!preserved(&c, REVERSE));
        assert_eq!(document_access(&c), before);
        assert_eq!(format!("{:?}", snapshot(&c.service)), diagnostic_before);
        assert_eq!(composition(&c), mark);
        oracle::document(&c, "ghbdtn", 13);
    }
}

#[test]
fn preserved_dispatch_refusal_and_unknown_matrix() {
    for scenario in 0..8 {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        keyboard.command();
        let writes = c.a.writes.get();
        match scenario {
            0..=2 => {
                c.a.invoke.set(false);
                c.a.retain_callback.set(true);
                c.a.results.borrow_mut().insert(
                    if scenario == 0 {
                        "RequestEditSession"
                    } else {
                        "session"
                    },
                    if scenario == 0 {
                        E_ACCESSDENIED
                    } else if scenario == 1 {
                        TF_E_SYNCHRONOUS
                    } else {
                        TF_S_ASYNC
                    },
                );
            }
            3 => c.a.selection.set(Span {
                start: 12,
                length: 0,
            }),
            4 => {
                let active = c.a.active.borrow();
                let span = &active.as_ref().unwrap().span;
                let mut value = span.get();
                value.start += 1;
                span.set(value);
            }
            5 => c.a.text.borrow_mut()[7] = b'X' as u16,
            6 => c.a.foreign.set(true),
            _ => {
                c.a.results.borrow_mut().insert("SetText", E_FAIL);
            }
        }
        assert_eq!(preserved(&c, MANUAL), scenario == 7);
        assert_eq!(c.a.writes.get(), writes + u32::from(scenario == 7));
        assert_eq!(c.a.starts.get(), 1);
        assert_eq!(c.a.ends.get(), 0);
        assert!(stopped(&c));
        if scenario < 3 {
            assert_eq!(c.delayed(), E_FAIL);
        }
        assert!(!preserved(&c, MANUAL));
        assert!(!preserved(&c, REVERSE));
        oracle::document(
            &c,
            if scenario == 7 {
                "привет"
            } else if scenario == 5 {
                "Xhbdtn"
            } else {
                "ghbdtn"
            },
            if scenario == 3 { 12 } else { 13 },
        );
    }
}

#[test]
fn preserved_dispatch_termination_and_retained_callback() {
    let c = Client::new();
    let mut keyboard = Keyboard::new();
    c.type_text("ghbdtn");
    terminate(&c);
    keyboard.command();
    let writes = c.a.writes.get();
    preserved(&c, MANUAL);
    preserved(&c, REVERSE);
    assert_eq!(c.a.writes.get(), writes);
    let retained = sink(&c);
    assert_eq!(c.deactivate(), S_OK);
    let before = document_access(&c);
    let mut eaten = BOOL(1);
    unsafe {
        assert_eq!(
            (retained.vtable().OnPreservedKey)(
                retained.as_raw(),
                c.context_a.as_raw(),
                &MANUAL,
                &mut eaten
            ),
            S_OK
        );
    }
    assert!(!eaten.as_bool());
    assert_eq!(document_access(&c), before);
    oracle::document(&c, "ghbdtn", 13);
}

#[test]
fn command_retention_missing_or_refused_witness_matrix() {
    for scenario in 0..6 {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        let profile = c.profile_state.profile.get();
        if scenario != 0 {
            keyboard.set(VK_LMENU, if scenario == 1 { 0 } else { 0x80 });
            if scenario == 2 {
                keyboard.set(VK_RMENU, 0x80);
            }
            if scenario == 3 {
                *c.manager.top.borrow_mut() = c.context_b.cast().unwrap();
            }
            if scenario == 4 {
                let mut changed = profile;
                changed.clsid = GUID::zeroed();
                c.profile_state.profile.set(changed);
            }
            if scenario == 5 {
                c.a.status.set(TF_SD_READONLY);
            }
            assert!(!preserved(&c, OBSERVER));
            keyboard.set(VK_RMENU, 0);
            *c.manager.top.borrow_mut() = c.context_a.cast().unwrap();
            c.profile_state.profile.set(profile);
            c.a.status.set(0);
        }
        keyboard.command();
        assert!(preserved(&c, MANUAL));
        keyboard.set(VK_LMENU, 0);
        assert!(!alt_release(&c, false));
        assert!(!alt_release(&c, true));
        oracle::document(&c, "привет", 13);
    }
}

#[test]
fn command_retention_authority_invalidation_matrix() {
    for scenario in 0..17 {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        keyboard.witness(&c);
        keyboard.set(VK_LCONTROL, 0x80);
        assert!(preserved(&c, MANUAL));
        let profile = c.profile_state.profile.get();
        match scenario {
            0 => *c.manager.top.borrow_mut() = c.context_b.cast().unwrap(),
            1 => {
                let mut changed = profile;
                changed.clsid = GUID::zeroed();
                c.profile_state.profile.set(changed);
            }
            2 => c.a.status.set(TF_SD_READONLY),
            3 => focus(&c, false),
            4 => notify_profile(&c, false),
            5 => terminate(&c),
            6 => assert_eq!(c.deactivate(), S_OK),
            7 => keyboard.witness(&c),
            8 => {
                keyboard.set(VK_LCONTROL, 0);
                keyboard.set(VK_LMENU, 0);
                assert!(down(&c, b'X' as u16, 1));
            }
            9 => {
                let sink: ITfThreadMgrEventSink = c.service.cast().unwrap();
                let manager: ITfDocumentMgr = c.thread.cast().unwrap();
                for _ in 0..2 {
                    unsafe {
                        assert_eq!(
                            (sink.vtable().OnSetFocus)(
                                sink.as_raw(),
                                manager.as_raw(),
                                manager.as_raw()
                            ),
                            S_OK
                        );
                    }
                }
            }
            10 => assert!(!down(&c, VK_MENU.0, 1)),
            11 => {
                let mut facts = c.captured_facts();
                facts.field.control = 104;
                *c.host.field_facts.borrow_mut() = Some(facts);
            }
            15 => c.a.disabled.set(true),
            16 => c.manager.disabled.set(true),
            _ => {}
        }
        keyboard.set(VK_LMENU, 0);
        let key = if scenario == 12 {
            VK_RMENU
        } else if scenario == 13 {
            VK_LMENU
        } else {
            VK_MENU
        };
        assert!(!release(
            &c,
            true,
            key.0,
            if scenario == 12 || scenario == 13 {
                0xc1000001
            } else {
                0xc0000001
            },
            scenario == 14
        ));
        *c.manager.top.borrow_mut() = c.context_a.cast().unwrap();
        c.profile_state.profile.set(profile);
        c.a.status.set(0);
        c.a.disabled.set(false);
        c.manager.disabled.set(false);
        c.host.field_facts.borrow_mut().take();
        assert!(!alt_release(&c, true));
        oracle::document(
            &c,
            if scenario == 8 {
                "приветx"
            } else {
                "привет"
            },
            if scenario == 8 { 14 } else { 13 },
        );
    }
}
fn notify_profile(c: &Client, ru: bool) {
    let sink: ITfInputProcessorProfileActivationSink = c.service.cast().unwrap();
    unsafe {
        assert_eq!(
            (sink.vtable().OnActivated)(
                sink.as_raw(),
                TF_PROFILETYPE_INPUTPROCESSOR,
                if ru { 0x419 } else { 0x409 },
                &crate::support::providers::SERVICE_ID,
                &GUID_TFCAT_TIP_KEYBOARD,
                &if ru {
                    GUID::from_u128(0x4c1e583c_f8e5_4b7d_b66c_b7e7319aed51)
                } else {
                    crate::support::providers::PROFILE_EN
                },
                HKL::default(),
                TF_IPSINK_FLAG_ACTIVE
            ),
            S_OK
        );
    }
}

#[test]
fn command_retention_nonverified_command_never_arms() {
    for scenario in 0..8 {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        keyboard.witness(&c);
        keyboard.set(VK_LCONTROL, 0x80);
        match scenario {
            1 => {
                c.a.invoke.set(false);
                c.a.results
                    .borrow_mut()
                    .insert("RequestEditSession", E_ACCESSDENIED);
            }
            2 => {
                c.a.results.borrow_mut().insert("session", E_FAIL);
            }
            3 | 6 => {
                c.a.results
                    .borrow_mut()
                    .insert("RequestEditSession", E_FAIL);
            }
            4 => {
                c.a.results.borrow_mut().insert("SetText", E_FAIL);
            }
            5 => c.a.invoke.set(false),
            7 => {
                c.a.results
                    .borrow_mut()
                    .insert("RequestEditSession", S_FALSE);
            }
            _ => {}
        }
        preserved(&c, if scenario == 0 { REVERSE } else { MANUAL });
        keyboard.set(VK_LMENU, 0);
        assert!(!alt_release(&c, true));
        if c.a.retained.borrow().is_some() {
            let writes = c.a.writes.get();
            assert_eq!(c.delayed(), E_FAIL);
            assert_eq!(c.a.writes.get(), writes);
            assert!(!alt_release(&c, true));
        }
        oracle::document(
            &c,
            if matches!(scenario, 0 | 1 | 5) {
                "ghbdtn"
            } else {
                "привет"
            },
            13,
        );
    }
}

#[test]
fn command_retention_reentrant_cancellation_matrix() {
    for boundary in ["SetText.after", "GetSelection", "RequestEditSession.after"] {
        for nested in 0..3 {
            let c = Rc::new(Client::new());
            let mut keyboard = Keyboard::new();
            c.type_text("ghbdtn");
            keyboard.witness(&c);
            keyboard.set(VK_LCONTROL, 0x80);
            let weak = Rc::downgrade(&c);
            c.host.on(boundary, 0, move || {
                let c = weak.upgrade().unwrap();
                assert!(!match nested {
                    0 => alt_release(&c, true),
                    1 => preserved(&c, OBSERVER),
                    _ => preserved(&c, REVERSE),
                });
            });
            preserved(&c, MANUAL);
            assert!(!alt_release(&c, true));
            oracle::document(
                &c,
                if boundary == "GetSelection" && nested == 2 {
                    "ghbdtn"
                } else {
                    "привет"
                },
                13,
            );
        }
    }
    for boundary in [
        "GetActiveProfile",
        "context.QI",
        "GetWnd",
        "Release.before",
        "Release.after",
    ] {
        let c = Rc::new(Client::new());
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        keyboard.witness(&c);
        keyboard.set(VK_LCONTROL, 0x80);
        assert!(preserved(&c, MANUAL));
        let weak = Rc::downgrade(&c);
        c.host.on(boundary, 0, move || {
            assert!(!preserved(&weak.upgrade().unwrap(), OBSERVER));
        });
        assert!(!alt_release(&c, true));
        assert!(!alt_release(&c, true));
        oracle::document(&c, "привет", 13);
    }
    let c = Rc::new(Client::new());
    let mut keyboard = Keyboard::new();
    c.type_text("ghbdtn");
    keyboard.witness(&c);
    keyboard.set(VK_LCONTROL, 0x80);
    let weak = Rc::downgrade(&c);
    c.host.on("RequestEditSession.after", 0, move || {
        let c = weak.upgrade().unwrap();
        let weak = Rc::downgrade(&c);
        c.host.on("Release.before", 0, move || {
            assert!(!alt_release(&weak.upgrade().unwrap(), true));
        });
    });
    assert!(preserved(&c, MANUAL));
    assert!(!alt_release(&c, true));
    oracle::document(&c, "привет", 13);
}

#[test]
fn command_release_balanced_reference_reentry_matrix() {
    for boundary in ["Release.before", "Release.after"] {
        let baseline = Rc::new(Client::new());
        let mut keyboard = Keyboard::new();
        baseline.type_text("ghbdtn");
        keyboard.witness(&baseline);
        keyboard.set(VK_LCONTROL, 0x80);
        assert!(preserved(&baseline, MANUAL));
        keyboard.set(VK_LMENU, 0);
        baseline.host.trace.borrow_mut().clear();
        assert!(alt_release(&baseline, true));
        let boundaries = baseline
            .host
            .trace
            .borrow()
            .iter()
            .filter(|name| **name == boundary)
            .count();
        assert!(boundaries > 0);
        oracle::document(&baseline, "привет", 13);
        for skip in 0..boundaries {
            let c = Rc::new(Client::new());
            keyboard.witness(&c);
            keyboard.set(VK_LCONTROL, 0);
            c.type_text("ghbdtn");
            keyboard.witness(&c);
            keyboard.set(VK_LCONTROL, 0x80);
            assert!(preserved(&c, MANUAL));
            keyboard.set(VK_LMENU, 0);
            let weak = Rc::downgrade(&c);
            c.host.on(boundary, skip, move || {
                focus(&weak.upgrade().unwrap(), true)
            });
            assert!(!alt_release(&c, true));
            oracle::document(&c, "привет", 13);
        }
    }
}

#[test]
fn preserved_activation_registration_failure_prefix_matrix() {
    for position in 0..3 {
        for failure in [E_FAIL, S_FALSE, TF_E_ALREADY_EXISTS] {
            let c = Client::new();
            assert_eq!(c.deactivate(), S_OK);
            c.manager.preserve_calls.borrow_mut().clear();
            c.manager.unpreserved_keys.borrow_mut().clear();
            c.manager.preserve_results[position].set(failure);
            let unadvises = c.manager.unadvises.each_ref().map(|count| count.get());
            assert!(c.reactivate().is_err());
            assert_eq!(c.manager.preserve_calls.borrow().len(), position + 1);
            assert_eq!(c.manager.unpreserved_keys.borrow().len(), position);
            assert_eq!(
                c.manager.unadvises.each_ref().map(|count| count.get()),
                unadvises.map(|count| count + 1)
            );
            for (cleaned, acquired) in c
                .manager
                .unpreserved_keys
                .borrow()
                .iter()
                .zip(c.manager.preserve_calls.borrow()[..position].iter().rev())
            {
                assert_eq!(cleaned.id, acquired.id);
                assert_eq!(cleaned.key, acquired.key);
            }
            assert_eq!(c.deactivate(), S_OK);
            assert_eq!(c.manager.unpreserved_keys.borrow().len(), position);
            c.manager.preserve_results[position].set(S_OK);
            assert_eq!(c.reactivate(), S_OK);
            oracle::document(&c, "", 7);
            oracle::no_effects(&c);
            assert_eq!(c.a.reads.get(), 0);
        }
    }
}

#[test]
fn preserved_activation_sink_failure_and_cancellation_matrix() {
    for position in 0..3 {
        for failure in [S_FALSE, E_FAIL] {
            let c = Client::new();
            assert_eq!(c.deactivate(), S_OK);
            let before = c.manager.unadvises.each_ref().map(|count| count.get());
            c.manager.sink_results[position].set(failure);
            assert!(c.reactivate().is_err());
            for (index, old) in before.iter().enumerate() {
                assert_eq!(
                    c.manager.unadvises[index].get(),
                    old + u32::from(index < position)
                );
            }
            c.manager.sink_results[position].set(S_OK);
            assert_eq!(c.reactivate(), S_OK);
            oracle::document(&c, "", 7);
            oracle::no_effects(&c);
        }
    }
    for (boundary, skip, successful_pairs) in [
        ("AdviseSink", 0, 0),
        ("AdviseSink", 1, 0),
        ("AdviseKeyEventSink", 0, 0),
        ("PreserveKey", 0, 1),
        ("PreserveKey", 1, 2),
        ("PreserveKey", 2, 3),
        ("AdviseSingleSink", 0, 3),
    ] {
        let c = Rc::new(Client::new());
        assert_eq!(c.deactivate(), S_OK);
        c.manager.unpreserved_keys.borrow_mut().clear();
        let cancelled = Rc::new(Cell::new(false));
        let release_count = Rc::new(Cell::new(0));
        let observed_cancelled = Rc::clone(&cancelled);
        let observed_releases = Rc::clone(&release_count);
        let weak = Rc::downgrade(&c);
        *c.host.observer.borrow_mut() = Some(Rc::new(move |name| {
            if name == "Release.before" && observed_cancelled.get() {
                let c = weak.upgrade().unwrap();
                observed_releases.set(observed_releases.get() + 1);
                assert_eq!(c.deactivate(), S_OK);
                assert_eq!(c.reactivate(), E_INVALIDARG);
            }
        }));
        let weak = Rc::downgrade(&c);
        c.host.on(boundary, skip, move || {
            let c = weak.upgrade().unwrap();
            cancelled.set(true);
            assert_eq!(c.deactivate(), S_OK);
            assert_eq!(c.reactivate(), E_INVALIDARG);
        });
        assert!(c.reactivate().is_err());
        c.host.observer.borrow_mut().take();
        assert!(release_count.get() >= 3);
        assert_eq!(c.manager.unpreserved_keys.borrow().len(), successful_pairs);
        assert_eq!(c.reactivate(), S_OK);
        oracle::document(&c, "", 7);
        oracle::no_effects(&c);
    }
}

#[test]
fn preserved_activation_cleanup_failure_and_rollback_matrix() {
    for position in 0..3 {
        for failure in [E_FAIL, S_FALSE, CONNECT_E_NOCONNECTION] {
            let c = Rc::new(Client::new());
            c.manager.unpreserve_results[position].set(failure);
            let release_count = Rc::new(Cell::new(0));
            let unpreserve_count = Rc::new(Cell::new(0));
            let observed_releases = Rc::clone(&release_count);
            let observed_unpreserves = Rc::clone(&unpreserve_count);
            let weak = Rc::downgrade(&c);
            *c.host.observer.borrow_mut() = Some(Rc::new(move |name| {
                if name == "UnpreserveKey" || name == "Release.before" {
                    let c = weak.upgrade().unwrap();
                    if name == "UnpreserveKey" {
                        observed_unpreserves.set(observed_unpreserves.get() + 1);
                        assert_eq!(c.deactivate(), S_OK);
                    } else {
                        observed_releases.set(observed_releases.get() + 1);
                    }
                    assert_eq!(c.reactivate(), E_INVALIDARG);
                }
            }));
            assert!(c.deactivate().is_err());
            c.host.observer.borrow_mut().take();
            assert_eq!(unpreserve_count.get(), 3);
            assert!(release_count.get() >= 4);
            assert_eq!(c.manager.unpreserved_keys.borrow().len(), 3);
            assert_eq!(
                c.manager.unadvises.each_ref().map(|count| count.get()),
                [1; 3]
            );
            for (cleaned, acquired) in c
                .manager
                .unpreserved_keys
                .borrow()
                .iter()
                .zip(c.manager.preserve_calls.borrow().iter().rev())
            {
                assert_eq!(cleaned.id, acquired.id);
                assert_eq!(cleaned.key, acquired.key);
            }
            c.manager.unpreserve_results[position].set(S_OK);
            assert_eq!(c.deactivate(), S_OK);
            assert_eq!(c.manager.unpreserved_keys.borrow().len(), 3);
            if failure == CONNECT_E_NOCONNECTION {
                assert_eq!(c.reactivate(), S_OK);
            } else {
                assert!(c.reactivate().is_err());
            }
            oracle::document(&c, "", 7);
            oracle::no_effects(&c);
        }
        let c = Client::new();
        c.manager.unsink_results[position].set(S_FALSE);
        assert!(c.deactivate().is_err());
        assert_eq!(c.manager.unpreserved_keys.borrow().len(), 3);
        assert_eq!(
            c.manager.unadvises.each_ref().map(|count| count.get()),
            [1; 3]
        );
        assert!(c.reactivate().is_err());
        oracle::document(&c, "", 7);
    }
    for cancel in [false, true] {
        let c = Rc::new(Client::new());
        assert_eq!(c.deactivate(), S_OK);
        c.manager.unpreserved_keys.borrow_mut().clear();
        c.manager.unpreserve_results[0].set(E_FAIL);
        if cancel {
            let weak = Rc::downgrade(&c);
            c.host.on("PreserveKey", 1, move || {
                assert_eq!(weak.upgrade().unwrap().deactivate(), S_OK);
            });
        } else {
            c.manager.preserve_results[1].set(E_FAIL);
        }
        assert!(c.reactivate().is_err());
        assert_eq!(
            c.manager.unpreserved_keys.borrow().len(),
            if cancel { 2 } else { 1 }
        );
        c.manager.unpreserve_results[0].set(S_OK);
        c.manager.preserve_results[1].set(S_OK);
        assert!(c.reactivate().is_err());
        oracle::document(&c, "", 7);
    }
}

#[test]
fn dedicated_layout_regression_and_negative_matrix() {
    for ru in [false, true] {
        let c = Client::new();
        set_mode(&c, ru);
        for substitute in [0, 1] {
            let mut profile = c.profile_state.profile.get();
            profile.hklSubstitute = HKL(substitute as *mut _);
            c.profile_state.profile.set(profile);
            let before = snapshot(&c.service);
            assert!(!down(&c, VK_F12.0, 1));
            assert_eq!(snapshot(&c.service).counts[4], before.counts[4] + 1);
            oracle::document(&c, "", 7);
            oracle::no_effects(&c);
        }
        assert_eq!(
            c.host
                .trace
                .borrow()
                .iter()
                .filter(|name| **name == "GetSubstituteKeyboardLayout")
                .count(),
            2
        );
    }
    for preserved_route in [false, true] {
        for scenario in 0..22 {
            let c = if scenario == 14 {
                Client::without_layouts()
            } else {
                Client::new()
            };
            let mut keyboard = Keyboard::new();
            if preserved_route {
                keyboard.command();
            }
            let mut profile = c.profile_state.profile.get();
            match scenario {
                0 | 1 => {
                    c.profile_state.results.borrow_mut().insert(
                        "GetActiveProfile",
                        if scenario == 0 { E_FAIL } else { S_FALSE },
                    );
                }
                2 | 3 => {
                    c.profile_state.results.borrow_mut().insert(
                        "QI",
                        if scenario == 2 {
                            E_NOINTERFACE
                        } else {
                            S_FALSE
                        },
                    );
                }
                4 => c.profile_state.nulls.borrow_mut().push("QI"),
                5 | 6 => {
                    c.profile_state.results.borrow_mut().insert(
                        "GetSubstituteKeyboardLayout",
                        if scenario == 5 { E_FAIL } else { S_FALSE },
                    );
                }
                7 => c.profile_state.substitute.set(HKL::default()),
                8 => c
                    .profile_state
                    .substitute
                    .set(HKL(0xe0200409usize as *mut _)),
                9 => c
                    .profile_state
                    .substitute
                    .set(HKL(0x04190419usize as *mut _)),
                10 => {
                    set_mode(&c, true);
                    profile = c.profile_state.profile.get();
                    *c.host.layouts.borrow_mut() = vec![0x04090409];
                }
                11 => *c.host.layout_results.borrow_mut() = vec![2, 1],
                12 => *c.host.layout_results.borrow_mut() = vec![65],
                13 => *c.host.layout_results.borrow_mut() = vec![0],
                14 => {}
                15 => profile.dwProfileType = TF_PROFILETYPE_KEYBOARDLAYOUT,
                16 => profile.clsid = GUID::zeroed(),
                17 => profile.dwFlags = 0,
                18 => profile.guidProfile = GUID::zeroed(),
                19 => c
                    .profile_state
                    .substitute
                    .set(HKL(
                        usize::try_from(0xffffffff04090409u64).unwrap_or(0x84090409) as *mut _,
                    )),
                20 => notify_profile(&c, true),
                _ => profile.langid = 0x419,
            }
            c.profile_state.profile.set(profile);
            let before = snapshot(&c.service);
            assert!(
                !admission(&c, preserved_route),
                "dedicated layout scenario {scenario}"
            );
            let after = snapshot(&c.service);
            for index in [3, 4, 5, 8] {
                assert_eq!(after.counts[index], before.counts[index]);
            }
            if scenario < 20 {
                let cause = match scenario {
                    0..=3 | 5..=6 => 1,
                    4 | 7 | 14 => 8,
                    15 => 9,
                    16 => 10,
                    17 => 11,
                    18 => 12,
                    _ => 13,
                };
                let result = match scenario {
                    0 | 5 => E_FAIL,
                    1 | 3 | 6 => S_FALSE,
                    2 => E_NOINTERFACE,
                    _ => S_OK,
                };
                assert_eq!(
                    &after.latest[1..],
                    &[if scenario < 2 { 3 } else { 4 }, cause, result.0 as u32],
                    "layout diagnostics {scenario}"
                );
            }
            assert_eq!(c.a.reads.get(), 0);
            assert_eq!(c.a.request.get(), 0);
            oracle::document(&c, "", 7);
            oracle::no_effects(&c);
            if matches!(scenario, 15..=18 | 20..=21) {
                assert!(
                    !c.host
                        .trace
                        .borrow()
                        .contains(&"GetSubstituteKeyboardLayout")
                );
            }
        }
    }
}
fn set_mode(c: &Client, ru: bool) {
    let mut profile = c.profile_state.profile.get();
    profile.guidProfile = if ru {
        GUID::from_u128(0x4c1e583c_f8e5_4b7d_b66c_b7e7319aed51)
    } else {
        crate::support::providers::PROFILE_EN
    };
    profile.langid = if ru { 0x419 } else { 0x409 };
    c.profile_state.profile.set(profile);
    c.profile_state.substitute.set(HKL(
        if ru { 0x04190419usize } else { 0x04090409usize } as *mut _
    ));
    notify_profile(c, ru);
}
fn admission(c: &Client, preserved_route: bool) -> bool {
    if preserved_route {
        preserved(c, MANUAL)
    } else {
        down(c, VK_F12.0, 1)
    }
}

#[test]
fn preserved_dispatch_both_modes_caps_repeat_and_receipt() {
    for ru in [false, true] {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        set_mode(&c, ru);
        let original = if ru { "руддщ" } else { "ghbdtn" };
        let converted = if ru { "hello" } else { "привет" };
        for unit in original.encode_utf16() {
            let mut event = crate::support::dll::Event::text(unit);
            event.mode = if ru { 2 } else { 1 };
            assert!(c.event(event));
        }
        let caret = if ru { 12 } else { 13 };
        oracle::document(&c, original, caret);
        let mark = composition(&c);
        let writes = c.a.writes.get();
        let ordinary_callbacks = snapshot(&c.service).counts[2];
        keyboard.command();
        keyboard.set(VK_CAPITAL, 1);
        for (repeat, word) in [converted, original, converted].into_iter().enumerate() {
            assert!(preserved(&c, MANUAL));
            assert_eq!(c.a.writes.get(), writes + repeat as u32 + 1);
            assert_eq!(composition(&c), mark);
            assert_eq!(c.a.starts.get(), 1);
            assert_eq!(c.a.ends.get(), 0);
            oracle::document(&c, word, caret);
        }
        assert!(preserved(&c, REVERSE));
        assert_eq!(c.a.writes.get(), writes + 4);
        for _ in 0..3 {
            assert!(preserved(&c, REVERSE));
            assert_eq!(c.a.writes.get(), writes + 4);
        }
        oracle::document(&c, original, caret);
        assert_eq!(snapshot(&c.service).counts[2], ordinary_callbacks);
    }
}

#[test]
fn preserved_dispatch_pointer_guid_thread_and_exclusion_matrix() {
    let c = Client::new();
    let _keyboard = Keyboard::new();
    let sink = sink(&c);
    let mut eaten = BOOL(1);
    unsafe {
        assert_eq!(
            (sink.vtable().OnPreservedKey)(
                sink.as_raw(),
                c.context_a.as_raw(),
                &MANUAL,
                std::ptr::null_mut()
            ),
            E_POINTER
        );
        assert_eq!(
            (sink.vtable().OnPreservedKey)(
                sink.as_raw(),
                std::ptr::null_mut(),
                &MANUAL,
                &mut eaten
            ),
            S_OK
        );
        assert!(!eaten.as_bool());
        assert_eq!(
            (sink.vtable().OnPreservedKey)(
                sink.as_raw(),
                std::ptr::null_mut(),
                &GUID::zeroed(),
                &mut eaten
            ),
            S_OK
        );
        assert!(!eaten.as_bool());
    }
    let raw_sink = sink.as_raw() as usize;
    let raw_context = c.context_a.as_raw() as usize;
    std::thread::spawn(move || unsafe {
        let raw = raw_sink as *mut std::ffi::c_void;
        let table = &**raw.cast::<*const ITfKeyEventSink_Vtbl>();
        let mut eaten = BOOL(1);
        assert_eq!(
            (table.OnPreservedKey)(raw, raw_context as *mut _, &MANUAL, &mut eaten),
            RPC_E_WRONG_THREAD
        );
        assert!(!eaten.as_bool());
    })
    .join()
    .unwrap();
    oracle::document(&c, "", 7);
    oracle::no_effects(&c);
    for scenario in 0..10 {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        keyboard.command();
        match scenario {
            0 | 1 | 5 => {
                let mut facts = c.captured_facts();
                facts.field.control = if scenario == 0 {
                    103
                } else if scenario == 1 {
                    104
                } else {
                    0
                };
                if scenario == 0 {
                    facts.field.style |=
                        windows::Win32::UI::WindowsAndMessaging::ES_PASSWORD as isize;
                }
                if scenario == 1 {
                    facts.field.style |=
                        windows::Win32::UI::WindowsAndMessaging::ES_READONLY as isize;
                }
                if scenario == 5 {
                    facts.field.class_name = facts.proof_root.class_name;
                    facts.focus = facts.root;
                }
                *c.host.field_facts.borrow_mut() = Some(facts);
            }
            2 => c.a.scope.set(IS_PASSWORD),
            3 => c.a.disabled.set(true),
            4 => c.a.status.set(TF_SD_READONLY),
            6 => c.a.scope.set(IS_EMAIL_SMTPEMAILADDRESS),
            7 => c.manager.disabled.set(true),
            8 => c.a.foreign.set(true),
            _ => *c.manager.top.borrow_mut() = c.context_b.cast().unwrap(),
        }
        assert!(!preserved(&c, MANUAL));
        assert!(!preserved(&c, REVERSE));
        assert_eq!(c.a.reads.get(), 0);
        assert_eq!(c.b.reads.get(), 0);
        oracle::document(&c, "", 7);
        oracle::no_effects(&c);
    }
}

#[test]
fn preserved_dispatch_continuation_and_release_reactivation() {
    let c = Client::new();
    let mut keyboard = Keyboard::new();
    c.type_text("ghbdtn ");
    keyboard.command();
    assert!(preserved(&c, REVERSE));
    oracle::document(&c, "ghbdtn ", 14);
    keyboard.set(VK_LCONTROL, 0);
    keyboard.set(VK_LMENU, 0);
    assert!(down(&c, b'X' as u16, 1));
    let writes = c.a.writes.get();
    keyboard.command();
    assert!(preserved(&c, REVERSE));
    assert_eq!(c.a.writes.get(), writes);
    oracle::document(&c, "ghbdtn x", 15);
    let c = Rc::new(Client::new());
    c.type_text("ghbdtn");
    let writes = c.a.writes.get();
    let weak = Rc::downgrade(&c);
    c.host.on("Release.after", 0, move || {
        let c = weak.upgrade().unwrap();
        assert_eq!(c.deactivate(), S_OK);
        assert_eq!(c.reactivate(), S_OK);
    });
    assert!(!preserved(&c, MANUAL));
    assert_eq!(c.a.writes.get(), writes);
    oracle::document(&c, "ghbdtn", 13);
}

#[test]
fn command_modifier_identity_caps_and_missing_layout() {
    let c = Client::new();
    let mut keyboard = Keyboard::new();
    assert!(down(&c, b'G' as u16, 1));
    let mark = composition(&c);
    let before = document_access(&c);
    keyboard.set(VK_LSHIFT, 0x80);
    assert!(!down(&c, VK_SHIFT.0, 1));
    assert_eq!(document_access(&c), before);
    assert!(down(&c, b'H' as u16, 1));
    keyboard.set(VK_LSHIFT, 0);
    assert!(!release(&c, true, VK_SHIFT.0, 0xc0000001, false));
    assert!(down(&c, b'B' as u16, 1));
    keyboard.set(VK_LWIN, 0x80);
    assert!(!down(&c, VK_LWIN.0, 1));
    keyboard.set(VK_LWIN, 0);
    assert!(!release(&c, true, VK_LWIN.0, 0xc0000001, false));
    assert_eq!(composition(&c), mark);
    assert_eq!(c.a.starts.get(), 1);
    oracle::document(&c, "gHb", 10);
    let c = Client::new();
    c.type_text("ghbdtn");
    keyboard.set(VK_CAPITAL, 1);
    assert!(keyboard.chord(&c, b'K' as u16));
    oracle::document(&c, "привет", 13);
    let c = Client::new();
    c.type_text("ghbdtn");
    let before = document_access(&c);
    c.profile_state.substitute.set(HKL::default());
    keyboard.set(VK_LCONTROL, 0x80);
    assert!(!down(&c, VK_CONTROL.0, 1));
    assert!(stopped(&c));
    assert_eq!(document_access(&c), before);
    oracle::document(&c, "ghbdtn", 13);
}

#[test]
fn preserved_activation_provisional_observer_and_profile_notifications() {
    {
        let c = Rc::new(Client::new());
        let mut keyboard = Keyboard::new();
        keyboard.command();
        assert_eq!(c.deactivate(), S_OK);
        let callbacks = Rc::new(Cell::new(0));
        let observed = Rc::clone(&callbacks);
        let weak = Rc::downgrade(&c);
        *c.host.observer.borrow_mut() = Some(Rc::new(move |name| {
            if name == "PreserveKey" {
                let c = weak.upgrade().unwrap();
                observed.set(observed.get() + 1);
                assert!(!preserved(&c, MANUAL));
                assert_eq!(document_access(&c), [0; 11]);
            }
        }));
        assert_eq!(c.reactivate(), S_OK);
        c.host.observer.borrow_mut().take();
        assert_eq!(callbacks.get(), 3);
        assert_eq!(c.manager.preserve_calls.borrow().len(), 6);
        oracle::document(&c, "", 7);
    }
    for during_query in [false, true] {
        let c = Rc::new(Client::new());
        assert_eq!(c.deactivate(), S_OK);
        let weak = Rc::downgrade(&c);
        c.host.on(
            if during_query {
                "GetActiveProfile"
            } else {
                "PreserveKey"
            },
            0,
            move || {
                let c = weak.upgrade().unwrap();
                notify_profile(&c, true);
                if !during_query {
                    let mut profile = c.profile_state.profile.get();
                    profile.guidProfile = GUID::from_u128(0x4c1e583c_f8e5_4b7d_b66c_b7e7319aed51);
                    profile.langid = 0x419;
                    c.profile_state.profile.set(profile);
                }
            },
        );
        assert_eq!(c.reactivate(), S_OK);
        assert_eq!(c.manager.preserve_calls.borrow().len(), 6);
        assert_eq!(snapshot(&c.service).mode, 2);
        oracle::document(&c, "", 7);
    }
}

#[test]
fn dedicated_layout_reentry_at_every_profile_and_layout_boundary() {
    for preserved_route in [false, true] {
        for (boundary, skip) in [
            ("GetActiveProfile", 0),
            ("profiles.QI", 0),
            ("GetSubstituteKeyboardLayout", 0),
            ("Layouts", 0),
            ("Layouts", 1),
            ("Layouts", 2),
        ] {
            for schedule in 0..6 {
                let c = Rc::new(Client::new());
                let mut keyboard = Keyboard::new();
                if preserved_route {
                    keyboard.command();
                }
                let weak = Rc::downgrade(&c);
                c.host.on(boundary, skip, move || {
                    let c = weak.upgrade().unwrap();
                    match schedule {
                        0 => assert!(!down(&c, VK_F12.0, 1)),
                        1 => focus(&c, false),
                        2 => {
                            assert_eq!(c.deactivate(), S_OK);
                            assert_eq!(c.reactivate(), S_OK);
                        }
                        3 => {
                            if boundary == "Layouts" {
                                *c.host.layouts.borrow_mut() = vec![];
                            } else {
                                c.profile_state.results.borrow_mut().insert(
                                    if boundary == "profiles.QI" {
                                        "QI"
                                    } else {
                                        boundary
                                    },
                                    E_FAIL,
                                );
                            }
                        }
                        4 => assert_eq!(c.deactivate(), S_OK),
                        _ => notify_profile(&c, true),
                    }
                });
                assert!(
                    !admission(&c, preserved_route),
                    "{boundary}/{skip}/{schedule}/{preserved_route}"
                );
                assert_eq!(c.a.reads.get(), 0);
                assert_eq!(c.a.request.get(), 0);
                oracle::document(&c, "", 7);
                oracle::no_effects(&c);
                if schedule < 3 {
                    assert_eq!(admission(&c, preserved_route), preserved_route);
                }
            }
        }
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        if preserved_route {
            keyboard.command();
        }
        *c.host.layout_results.borrow_mut() = vec![2, 2, 1];
        assert!(!admission(&c, preserved_route));
        assert_eq!(c.host.layout_calls.get(), 3);
        oracle::document(&c, "", 7);
        oracle::no_effects(&c);
    }
}
