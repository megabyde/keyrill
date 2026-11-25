use crate::support::{client::Client, dll::snapshot, oracle};
use keyrill_windows_proof::{
    diagnostics::{Publication, comparable},
    protocol::{Payload, complete_interval, decode_payload},
};
use keyrill_windows_proof::{
    mapping::{ArmedPage, Availability},
    native::capture_identity,
    protocol::ProbeSnapshot,
};
use std::rc::Rc;
use windows::Win32::{
    Foundation::*,
    System::Threading::*,
    UI::{Input::KeyboardAndMouse::*, TextServices::*},
};
use windows_core::{BOOL, GUID, Interface};

const MANUAL: GUID = GUID::from_u128(0xd7abce42_c8a9_4a43_990a_4a8d6c011ebb);
const REVERSE: GUID = GUID::from_u128(0x16531b70_516e_4dfb_9f18_cb760de590b4);
const OBSERVER: GUID = GUID::from_u128(0x997f7561_053d_475a_abb9_b11fcf16bb48);

struct Keyboard {
    saved: [u8; 256],
    state: [u8; 256],
}
impl Keyboard {
    fn new() -> Self {
        let mut saved = [0; 256];
        unsafe {
            assert!(crate::support::keyboard_state(&mut saved));
            SetKeyboardState(&[0; 256]).unwrap();
        }
        Self {
            saved,
            state: [0; 256],
        }
    }
    fn apply(&self) {
        unsafe {
            SetKeyboardState(&self.state).unwrap();
        }
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
        self.apply();
    }
    fn command(&mut self) {
        self.set(VK_LCONTROL, 0x80);
        self.set(VK_LMENU, 0x80);
    }
}
impl Drop for Keyboard {
    fn drop(&mut self) {
        unsafe {
            SetKeyboardState(&self.saved).unwrap();
        }
    }
}
fn armed() -> ArmedPage {
    ArmedPage::create(unsafe {
        capture_identity(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            GetCurrentThreadId(),
        )
        .unwrap()
    })
    .unwrap()
}
fn observed(page: &ArmedPage) -> ProbeSnapshot {
    let (availability, words) = page.read();
    assert_eq!(availability, Availability::Available);
    decode_payload(&words).unwrap()
}
fn rows(snapshot: &ProbeSnapshot) -> Vec<[u32; 10]> {
    snapshot.observations[3..3 + snapshot.observations[2] as usize * 10]
        .as_chunks::<10>()
        .0
        .to_vec()
}
fn preserved(c: &Client, id: GUID) -> bool {
    let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
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
fn down(c: &Client, key: VIRTUAL_KEY) -> bool {
    let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
    let mut eaten = BOOL(1);
    let scan = unsafe { MapVirtualKeyW(key.0 as u32, MAPVK_VK_TO_VSC) };
    unsafe {
        assert_eq!(
            (sink.vtable().OnKeyDown)(
                sink.as_raw(),
                c.context_a.as_raw(),
                WPARAM(key.0 as usize),
                LPARAM(((scan as isize) << 16) | 1),
                &mut eaten
            ),
            S_OK
        );
    }
    c.host.check();
    eaten.as_bool()
}
fn release(c: &Client, actual: bool, key: VIRTUAL_KEY, extended: bool) -> bool {
    let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
    let mut eaten = BOOL(1);
    let call = if actual {
        sink.vtable().OnKeyUp
    } else {
        sink.vtable().OnTestKeyUp
    };
    unsafe {
        assert_eq!(
            call(
                sink.as_raw(),
                c.context_a.as_raw(),
                WPARAM(key.0 as usize),
                LPARAM(if extended { 1 << 24 } else { 0 }),
                &mut eaten
            ),
            S_OK
        );
    }
    c.host.check();
    eaten.as_bool()
}
fn access(c: &Client) -> [u32; 8] {
    [
        c.a.reads.get(),
        c.a.writes.get(),
        c.a.starts.get(),
        c.a.ends.get(),
        c.b.reads.get(),
        c.b.writes.get(),
        c.b.starts.get(),
        c.b.ends.get(),
    ]
}

#[test]
fn command_observation_modifier_projection_all_512_masks() {
    let page = armed();
    let mut keyboard = Keyboard::new();
    for mask in 0..512u32 {
        let c = Client::new();
        keyboard.state.fill(0);
        for (bit, key) in [
            VK_LCONTROL,
            VK_LMENU,
            VK_RCONTROL,
            VK_RMENU,
            VK_SHIFT,
            VK_LSHIFT,
            VK_RSHIFT,
            VK_LWIN,
            VK_RWIN,
        ]
        .into_iter()
        .enumerate()
        {
            keyboard.state[key.0 as usize] = if mask & (1 << bit) != 0 { 0x80 } else { 0 };
        }
        keyboard.apply();
        let exact = mask == 3;
        assert_eq!(preserved(&c, MANUAL), exact, "mask {mask}");
        let first = rows(&observed(&page));
        let facts = 0x100
            | (mask & 15)
            | if mask & 0x70 != 0 { 16 } else { 0 }
            | if mask & 0x180 != 0 { 32 } else { 0 };
        assert_eq!(first[1][8], facts, "mask {mask}");
        assert_eq!(first[1][4], if exact { 5 } else { 3 });
        keyboard.state[VK_CAPITAL.0 as usize] = 0xff;
        keyboard.state[b'K' as usize] = 0xff;
        keyboard.state[VK_CONTROL.0 as usize] = 0x81;
        keyboard.state[VK_MENU.0 as usize] = 0x81;
        keyboard.apply();
        assert_eq!(preserved(&c, MANUAL), exact);
        let second = rows(&observed(&page));
        assert_eq!(second[first.len() + 1][8], facts);
        assert_eq!(access(&c)[1..], [0; 7]);
        if !exact {
            assert_eq!(c.a.reads.get(), 0);
        }
        oracle::document(&c, "", 7);
        c.host.drained();
    }
}

#[test]
fn command_observation_executor_completions_and_release_witness() {
    let page = armed();
    for scenario in 0..6 {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        if scenario != 4 {
            if scenario == 5 {
                keyboard.set(VK_RMENU, 0x80);
            }
            keyboard.set(VK_LMENU, 0x80);
            assert!(!preserved(&c, OBSERVER));
            keyboard.set(VK_RMENU, 0);
        }
        keyboard.command();
        if scenario == 1 {
            c.a.results.borrow_mut().insert("SetText", E_FAIL);
        }
        if scenario == 2 {
            c.a.invoke.set(false);
            c.a.results
                .borrow_mut()
                .insert("RequestEditSession", E_ACCESSDENIED);
        }
        let eaten = preserved(&c, if scenario == 0 { REVERSE } else { MANUAL });
        assert_eq!(eaten, scenario != 2);
        let value = observed(&page);
        let observed_rows = rows(&value);
        let completion = observed_rows.last().unwrap();
        assert_eq!(completion[4], 9);
        assert_eq!(completion[8], u32::from(scenario < 4));
        assert_eq!(
            completion[9],
            match scenario {
                0 => 3,
                1 => 2,
                2 => 1,
                _ => 4,
            }
        );
        if scenario != 4 {
            assert_eq!(observed_rows[0][4..9], [8, 0, 0, 0, 1]);
            assert_eq!(observed_rows[1][8], if scenario == 5 { 3 } else { 2 });
        }
        keyboard.set(VK_LMENU, 0);
        let before = access(&c);
        assert_eq!(release(&c, true, VK_MENU, false), scenario == 3);
        assert_eq!(access(&c), before);
        let value = observed(&page);
        let observed_rows = rows(&value);
        let release = observed_rows.last().unwrap();
        assert_eq!(release[4], 7);
        assert_eq!(release[5], u32::from(scenario == 3));
        assert_eq!(release[6], u32::from(scenario == 3));
        oracle::document(
            &c,
            if scenario == 1 || scenario >= 3 {
                "привет"
            } else {
                "ghbdtn"
            },
            13,
        );
        c.host.drained();
    }
}

#[test]
fn command_observation_content_independence_and_ordinary_text() {
    let page = armed();
    let mut first = None;
    for variation in [false, true] {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        let word = if variation { "hello" } else { "ghbdtn" };
        c.type_text(word);
        keyboard.set(VK_LCONTROL, 0x80);
        keyboard.set(VK_CAPITAL, u8::from(variation));
        keyboard.set(VIRTUAL_KEY(b'Q' as u16), if variation { 0x81 } else { 0 });
        assert!(!preserved(&c, MANUAL));
        let evidence = observed(&page).observations;
        if let Some(first) = first {
            assert_eq!(evidence, first);
        } else {
            first = Some(evidence);
        }
        oracle::document(&c, word, if variation { 12 } else { 13 });
    }
    let c = Client::new();
    let _keyboard = Keyboard::new();
    assert!(down(&c, VIRTUAL_KEY(b'K' as u16)));
    assert!(down(&c, VIRTUAL_KEY(b'Z' as u16)));
    assert!(!down(&c, VK_SHIFT));
    assert_eq!(observed(&page).observations[2], 0);
    oracle::document(&c, "kz", 9);
}

#[test]
fn command_observation_routes_release_sides_and_thread_arguments() {
    let page = armed();
    let c = Client::new();
    let mut keyboard = Keyboard::new();
    c.type_text("ghbdtn");
    keyboard.command();
    let old = snapshot(&c.service);
    let composition = c.a.active.borrow().as_ref().unwrap().pointer;
    let writes = c.a.writes.get();
    assert!(preserved(&c, MANUAL));
    assert!(preserved(&c, REVERSE));
    oracle::document(&c, "ghbdtn", 13);
    let value = observed(&page);
    let recorded = rows(&value);
    assert_eq!(recorded.len(), 6);
    assert_eq!(
        (
            recorded[0][4],
            recorded[0][5],
            recorded[0][6],
            recorded[0][8]
        ),
        (1, 2, 1, 0)
    );
    assert_eq!((recorded[1][4], recorded[1][8]), (5, 0x103));
    assert_eq!(recorded[3][6], 2);
    assert_eq!(snapshot(&c.service).counts[2], old.counts[2]);
    assert_eq!(c.a.writes.get(), writes + 2);
    assert_eq!(c.a.active.borrow().as_ref().unwrap().pointer, composition);
    let before = snapshot(&c.service);
    let profile_calls = || {
        c.host
            .trace
            .borrow()
            .iter()
            .filter(|name| **name == "GetActiveProfile")
            .count()
    };
    let calls = profile_calls();
    let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
    assert!(!preserved(&c, GUID::zeroed()));
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
    }
    assert_eq!(snapshot(&c.service), before);
    assert_eq!(observed(&page).observations, value.observations);
    assert_eq!(profile_calls(), calls);
    assert!(down(&c, VIRTUAL_KEY(b'K' as u16)));
    assert!(down(&c, VIRTUAL_KEY(b'Z' as u16)));
    let recorded = rows(&observed(&page));
    assert_eq!(recorded.len(), 12);
    assert_eq!(
        (recorded[6][5], recorded[6][8], recorded[9][6]),
        (1, 0x103, 2)
    );
    assert_eq!(snapshot(&c.service).counts[2], old.counts[2] + 2);
    assert_eq!(snapshot(&c.service).counts[3], old.counts[3] + 2);
    let before = snapshot(&c.service);
    let accesses = access(&c);
    for (key, extended, side) in [
        (VK_CONTROL, false, 1),
        (VK_CONTROL, true, 2),
        (VK_MENU, false, 3),
        (VK_MENU, true, 4),
        (VK_LCONTROL, false, 1),
        (VK_LCONTROL, true, 5),
        (VK_RCONTROL, false, 5),
        (VK_RCONTROL, true, 2),
        (VK_LMENU, false, 3),
        (VK_LMENU, true, 6),
        (VK_RMENU, false, 6),
        (VK_RMENU, true, 4),
    ] {
        for actual in [false, true] {
            let sequence = observed(&page).observations[0];
            assert!(!release(&c, actual, key, extended));
            let value = observed(&page);
            let recorded = rows(&value);
            assert_eq!(value.observations[0], sequence + 1);
            assert_eq!(
                recorded.last().unwrap()[8..10],
                [side, if actual { 2 } else { 1 }]
            );
        }
    }
    let release_history = observed(&page).observations;
    unsafe {
        assert_eq!(
            (sink.vtable().OnKeyUp)(
                sink.as_raw(),
                c.context_a.as_raw(),
                WPARAM(VK_CONTROL.0 as usize),
                LPARAM(0),
                std::ptr::null_mut()
            ),
            E_POINTER
        );
        assert_eq!(
            (sink.vtable().OnTestKeyUp)(
                sink.as_raw(),
                c.context_a.as_raw(),
                WPARAM(VK_MENU.0 as usize),
                LPARAM(0),
                std::ptr::null_mut()
            ),
            E_POINTER
        );
    }
    assert!(!release(&c, true, VIRTUAL_KEY(b'K' as u16), false));
    let raw = sink.as_raw() as usize;
    let context = c.context_a.as_raw() as usize;
    std::thread::spawn(move || unsafe {
        let raw = raw as *mut std::ffi::c_void;
        let table = &**raw.cast::<*const ITfKeyEventSink_Vtbl>();
        for (call, key) in [(table.OnKeyUp, VK_CONTROL), (table.OnTestKeyUp, VK_MENU)] {
            let mut eaten = BOOL(1);
            assert_eq!(
                call(
                    raw,
                    context as *mut _,
                    WPARAM(key.0 as usize),
                    LPARAM(0),
                    &mut eaten
                ),
                S_OK
            );
            assert!(!eaten.as_bool());
        }
    })
    .join()
    .unwrap();
    assert_eq!(observed(&page).observations, release_history);
    assert_eq!(snapshot(&c.service), before);
    assert_eq!(access(&c), accesses);
    assert_eq!(profile_calls(), calls + 2);
    assert_eq!(c.a.active.borrow().as_ref().unwrap().pointer, composition);
    let mut writes_per_request = std::collections::BTreeMap::new();
    for operation in c.a.operations.borrow().iter() {
        if operation.effect == crate::support::document::Effect::Text {
            *writes_per_request.entry(operation.request).or_insert(0) += 1;
        }
    }
    assert!(writes_per_request.values().all(|count| *count == 1));
    oracle::document(&c, "ghbdtn", 13);
    c.host.drained();
}

#[test]
fn command_observation_modifier_refusals() {
    let page = armed();
    for extra in [
        0,
        1,
        VK_RCONTROL.0,
        VK_RMENU.0,
        VK_LSHIFT.0,
        VK_RSHIFT.0,
        VK_LWIN.0,
        VK_RWIN.0,
    ] {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        if extra != 0 {
            keyboard.set(VK_LCONTROL, 0x80);
        }
        if extra != 1 {
            keyboard.set(VK_LMENU, 0x80);
        }
        if extra > 1 {
            keyboard.set(VIRTUAL_KEY(extra), 0x80);
        }
        let before = snapshot(&c.service);
        let accesses = access(&c);
        assert!(!preserved(&c, MANUAL));
        let recorded = rows(&observed(&page));
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[1][4], 3);
        assert_ne!(recorded[1][8], 0x103);
        assert_ne!(recorded[1][8], 0);
        assert_eq!(snapshot(&c.service), before);
        assert_eq!(access(&c), accesses);
        oracle::document(&c, "ghbdtn", 13);
        c.host.drained();
    }
}

#[test]
fn command_observation_admission_dispatch_and_reentry() {
    const RELEASE_REENTRY: usize = 4;
    const OBSERVATION_STAGE: usize = 4;
    const ADMISSION_REFUSED: u32 = 2;
    const COUNTS_OFFSET: usize = 9;
    const HANDLE_ENTERED: usize = 4;
    let page = armed();
    for scenario in 0..6 {
        let c = Rc::new(Client::new());
        let mut keyboard = Keyboard::new();
        c.type_text("ghbdtn");
        keyboard.command();
        let accesses = access(&c);
        let old = snapshot(&c.service);
        if scenario == 1 || scenario == 2 {
            c.profile_state.results.borrow_mut().insert(
                "GetActiveProfile",
                if scenario == 1 {
                    E_ACCESSDENIED
                } else {
                    E_FAIL
                },
            );
        }
        if scenario == 3 {
            c.a.invoke.set(false);
            c.a.results
                .borrow_mut()
                .insert("RequestEditSession", E_ACCESSDENIED);
        }
        if scenario >= RELEASE_REENTRY {
            let weak = Rc::downgrade(&c);
            c.host.on(
                if scenario == RELEASE_REENTRY {
                    "native.release"
                } else {
                    "GetActiveProfile"
                },
                0,
                move || {
                    let c = weak.upgrade().unwrap();
                    if scenario == RELEASE_REENTRY {
                        let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
                        unsafe {
                            assert_eq!((sink.vtable().OnSetFocus)(sink.as_raw(), BOOL(0)), S_OK);
                        }
                    } else {
                        assert!(!preserved(&c, REVERSE));
                    }
                },
            );
        }
        let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
        let mut eaten = BOOL(1);
        unsafe {
            assert_eq!(
                (sink.vtable().OnPreservedKey)(
                    sink.as_raw(),
                    if scenario == 0 {
                        std::ptr::null_mut()
                    } else {
                        c.context_a.as_raw()
                    },
                    &MANUAL,
                    &mut eaten
                ),
                S_OK
            );
        }
        assert!(!eaten.as_bool());
        assert_eq!(access(&c), accesses);
        let value = observed(&page);
        let recorded = rows(&value);
        let last = recorded[recorded.len() - if scenario == 3 { 2 } else { 1 }];
        match scenario {
            0..=2 => {
                assert_eq!(last[OBSERVATION_STAGE], ADMISSION_REFUSED);
                assert_eq!(last[8] != 0, scenario != 0);
            }
            3 => {
                assert_eq!(last[4], 5);
                assert_eq!(
                    value.service[COUNTS_OFFSET + HANDLE_ENTERED],
                    old.counts[HANDLE_ENTERED] + 1
                );
                assert_eq!(value.service[54], E_ACCESSDENIED.0 as u32);
            }
            RELEASE_REENTRY => {
                assert_eq!(last[OBSERVATION_STAGE], ADMISSION_REFUSED);
                assert_eq!(
                    value.service[COUNTS_OFFSET + HANDLE_ENTERED],
                    old.counts[HANDLE_ENTERED]
                );
            }
            5 => {
                assert_eq!(recorded.len(), 4);
                assert_ne!(recorded[0][7], recorded[1][7]);
                assert_eq!(recorded[2][8], 0);
                assert_eq!(last[8], 0x103);
                assert_eq!(last[7], recorded[0][7]);
            }
            _ => unreachable!(),
        }
        oracle::document(&c, "ghbdtn", 13);
        c.host.drained();
    }
}

#[test]
fn command_observation_exclusions_and_lifecycle_anchors() {
    let page = armed();
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
                c.host.field_facts.replace(Some(facts));
            }
            2 => c.a.scope.set(IS_PASSWORD),
            3 => c.a.disabled.set(true),
            4 => c.a.status.set(TF_SD_READONLY),
            6 => c.a.scope.set(IS_EMAIL_SMTPEMAILADDRESS),
            7 => c.manager.disabled.set(true),
            8 => c.a.foreign.set(true),
            _ => {
                let other = c.context_b.cast().unwrap();
                let retired = c.manager.top.replace(other);
                drop(retired);
            }
        }
        assert!(!preserved(&c, MANUAL));
        assert_eq!(access(&c), [0; 8]);
        let recorded = rows(&observed(&page));
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[1][4], 5);
        oracle::document(&c, "", 7);
        c.host.drained();
    }
    let c = Rc::new(Client::new());
    let mut keyboard = Keyboard::new();
    c.type_text("ghbdtn");
    keyboard.command();
    let weak = Rc::downgrade(&c);
    c.host.on("GetActiveProfile", 0, move || {
        let c = weak.upgrade().unwrap();
        assert!(!release(&c, false, VK_CONTROL, false));
        let composition = c.a.active.borrow().as_ref().unwrap().pointer;
        let sink = c.service.cast::<ITfCompositionSink>().unwrap();
        unsafe {
            assert_eq!(
                (sink.vtable().OnCompositionTerminated)(sink.as_raw(), 123, composition),
                S_OK
            );
        }
        assert!(!release(&c, true, VK_MENU, false));
    });
    assert!(!preserved(&c, MANUAL));
    let recorded = rows(&observed(&page));
    assert_eq!(recorded.len(), 4);
    assert_eq!(recorded[1][2], 1);
    assert_eq!(recorded[2][2], 0);
    assert!(recorded[1][1] < recorded[2][1]);
    assert_eq!(recorded[3][4], 2);
    oracle::document(&c, "ghbdtn", 13);
    c.host.drained();
}

fn publication() -> Publication {
    let mut value = Publication {
        publication: 1,
        source: 1,
        availability: 4,
        continuity: 2,
        ..Publication::default()
    };
    value.snapshot.base[0..2].copy_from_slice(&[1, 1]);
    value
}

fn lifecycle(events: &[[u32; 5]], allocated: u32, owned: u32) -> [u32; 86] {
    let mut result = [0; 86];
    let sequence = events.last().map_or(0, |e| e[0]);
    let retained = &events[events.len().saturating_sub(16)..];
    result[..6].copy_from_slice(&[
        sequence,
        allocated,
        u32::from(sequence == u32::MAX || allocated == u32::MAX),
        retained.len() as u32,
        u32::from(owned != 0),
        owned,
    ]);
    for (slot, event) in result[6..].as_chunks_mut::<5>().0.iter_mut().zip(retained) {
        slot.copy_from_slice(event);
    }
    result
}

#[test]
fn diagnostic_codec_canonical_refusal_and_negative_matrix() {
    let absent = Publication::default();
    assert_eq!(Publication::parse(&absent.format().unwrap()), Some(absent));
    let mut value = publication();
    value.continuity = 1;
    value.snapshot.base[38] = 1;
    value.snapshot.base[47..51].copy_from_slice(&[1, 17, 1, 1]);
    value.snapshot.base[51..55].copy_from_slice(&[1, 17, 1, 1]);
    let good = value.format().unwrap();
    assert_eq!(Publication::parse(&good), Some(value.clone()));
    for bad in [
        format!("{good}; unexpected=1"),
        good[1..].to_owned(),
        format!("{good} "),
        good.replace("service v2", "service v1"),
        good[..good.len() - 1].to_owned(),
        "x".repeat(8192),
    ] {
        assert!(Publication::parse(&bad).is_none());
    }
    for (from, to) in [
        ("active=1", "active=2"),
        ("source=1", "source=01"),
        ("result=0", "result=4294967296"),
        ("/Native/1", "/Predicate/1"),
        ("state=Available", "state=Fresh"),
    ] {
        assert!(good.contains(from));
        assert!(Publication::parse(&good.replace(from, to)).is_none());
    }
    value.snapshot.base[9] = u32::MAX;
    value.snapshot.base[4] = 1;
    value.continuity = 3;
    assert_eq!(
        Publication::parse(&value.format().unwrap()),
        Some(value.clone())
    );
    value.snapshot.base[4] = 0;
    assert!(value.format().is_none());
}

#[test]
fn lifecycle_codec_complete_window_and_negative_matrix() {
    let mut events = vec![
        [1, 1, 1, 0, 0],
        [2, 2, 1, 0, 0],
        [3, 3, 1, 0, 0],
        [4, 1, 2, 0, 0],
    ];
    let mut first = publication();
    first.snapshot.lifecycle = lifecycle(&events, 2, 2);
    let mut second = first.clone();
    second.publication += 1;
    assert!(comparable(&first, &second));
    let mut changed = second.clone();
    changed.snapshot.lifecycle[7] = 2;
    assert!(changed.valid());
    assert!(!comparable(&first, &changed));
    events.extend((5..=20).map(|number| [number, 8, 2, 2, 0]));
    second.snapshot.lifecycle = lifecycle(&events, 2, 2);
    assert!(comparable(&first, &second));
    events.push([21, 8, 2, 4, 0]);
    second.snapshot.lifecycle = lifecycle(&events, 2, 2);
    assert!(second.valid());
    assert!(!comparable(&first, &second));
    let good = second.format().unwrap();
    assert_eq!(Publication::parse(&good), Some(second.clone()));
    for scenario in 0..9 {
        let mut bad = second.clone();
        let l = &mut bad.snapshot.lifecycle;
        match scenario {
            0 => l[11] = l[6],
            1 => {
                for i in 0..5 {
                    l.swap(6 + i, 11 + i);
                }
            }
            2 => l[7] = 10,
            3 => l[9] = 0,
            4 => l[10] = 4,
            5 => l[8] = l[1] + 1,
            6 => l[5] = 0,
            7 => l[3] = 17,
            8 => l[4] = 0,
            _ => unreachable!(),
        }
        assert!(!bad.valid(), "scenario {scenario}");
        assert!(bad.format().is_none());
    }
    for (from, to) in [
        ("event_count=16", "event_count=17"),
        ("owned_serial=2", "owned_serial=0"),
        ("v2;", "v3;"),
        ("event=6/8/2/2/0", "event=6/99/2/2/0"),
    ] {
        assert!(good.contains(from));
        assert!(Publication::parse(&good.replace(from, to)).is_none());
        assert_eq!(Publication::parse(&good), Some(second.clone()));
    }
    for bad in [
        format!("{good}; event=0/0/0/0/0"),
        good[..good.len() - 1].into(),
        "x".repeat(8192),
    ] {
        assert!(Publication::parse(&bad).is_none());
    }
    let mut sparse = first.clone();
    sparse.snapshot.lifecycle[28] = 1;
    assert!(!sparse.valid());
    sparse = first.clone();
    sparse.snapshot.lifecycle[8] = 0;
    assert!(!sparse.valid());
    assert!(!comparable(&first, &first));
    for activation in [false, true] {
        let mut changed = first.clone();
        changed.publication += 1;
        if activation {
            changed.snapshot.base[0] += 1;
        } else {
            changed.source += 1;
        }
        assert!(!comparable(&first, &changed));
    }
    let mut maximum = second;
    maximum.publication = u32::MAX;
    maximum.source = u32::MAX;
    maximum.saturated = 1;
    maximum.continuity = 3;
    maximum.snapshot.base[0] = u32::MAX;
    maximum.snapshot.base[4] = 1;
    maximum.snapshot.base[5..9].copy_from_slice(&[1, u32::MAX, 1, u32::MAX]);
    maximum.snapshot.base[9..47].fill(u32::MAX);
    maximum.snapshot.base[47..51].copy_from_slice(&[1, 25, 1, u32::MAX]);
    maximum.snapshot.base[51..55].copy_from_slice(&[1, 25, 1, u32::MAX]);
    maximum.snapshot.lifecycle = lifecycle(
        &(u32::MAX - 15..=u32::MAX)
            .map(|n| [n, 7, u32::MAX, 0, 4])
            .collect::<Vec<_>>(),
        u32::MAX,
        u32::MAX,
    );
    let text = maximum.format().unwrap();
    assert!(text.len() < 8192);
    assert_eq!(Publication::parse(&text), Some(maximum));
}

fn payload() -> Payload {
    let mut words = [0; 512];
    words[0] = 1;
    words[1] = 3;
    words[500] = 0x8664;
    words[24..26].copy_from_slice(&[1, 1]);
    words
}

fn observations(words: &mut Payload, rows: &[[u32; 10]]) {
    words[177..500].fill(0);
    let sequence = rows.last().map_or(0, |r| r[0]);
    let retained = &rows[rows.len().saturating_sub(32)..];
    words[177..180].copy_from_slice(&[
        sequence,
        u32::from(sequence == u32::MAX),
        retained.len() as u32,
    ]);
    for (target, row) in words[180..500]
        .as_chunks_mut::<10>()
        .0
        .iter_mut()
        .zip(retained)
    {
        target.copy_from_slice(row);
    }
}

#[test]
fn observation_codec_golden_rows_and_hostile_fields() {
    let mut words = payload();
    words[79..165].copy_from_slice(&lifecycle(&[[1, 1, 1, 0, 0]], 1, 1));
    observations(
        &mut words,
        &[
            [1, 1, 1, 1, 1, 2, 1, 1, 0, 0],
            [2, 1, 1, 1, 5, 2, 1, 1, 0x103, 0],
            [3, 1, 1, 1, 6, 2, 1, 1, 0x103, 0],
            [4, 1, 1, 1, 7, 0, 0, 0, 4, 2],
        ],
    );
    let decoded = decode_payload(&words).unwrap();
    assert_eq!(decoded.observations, words[177..500]);
    for (field, value) in [
        (177, 0),
        (178, 1),
        (178, 2),
        (179, 33),
        (180, 0),
        (180, 2),
        (181, 2),
        (182, 2),
        (183, 2),
        (184, 0),
        (184, 8),
        (185, 0),
        (185, 3),
        (186, 0),
        (186, 3),
        (187, 0),
        (187, 2),
        (188, 0x100),
        (188, 1),
        (188, 0x140),
        (189, 1),
        (194, 3),
        (194, 2),
        (195, 1),
        (196, 2),
        (197, 2),
        (198, 0),
        (198, 0x100),
        (198, 0x203),
        (204, 5),
        (208, 0),
        (208, 0x102),
        (215, 1),
        (216, 1),
        (217, 1),
        (218, 0),
        (218, 7),
        (219, 0),
        (219, 3),
        (220, 1),
        (500, 1),
        (511, 1),
        (185, u32::MAX),
        (186, u32::MAX),
        (218, u32::MAX),
        (219, u32::MAX),
    ] {
        let mut malformed = words;
        malformed[field] = value;
        assert!(
            decode_payload(&malformed).is_err(),
            "field {field} value {value}"
        );
        assert_eq!(decode_payload(&words).unwrap(), decoded);
    }
}

#[test]
fn observation_codec_complete_terminal_grammar() {
    let progress = [(2, 0x103), (3, 0x100), (4, 0x103), (5, 0x103), (6, 0x103)];
    for ordinary in [false, true] {
        for first in 0..5 {
            for second in 0..=5 {
                let route = if ordinary { 1 } else { 2 };
                let mut rows = vec![[
                    1,
                    0,
                    0,
                    0,
                    1,
                    route,
                    1,
                    1,
                    if ordinary { 0x103 } else { 0 },
                    0,
                ]];
                rows.push([
                    2,
                    0,
                    0,
                    0,
                    progress[first].0,
                    route,
                    1,
                    1,
                    progress[first].1,
                    0,
                ]);
                if second != 5 {
                    rows.push([
                        3,
                        0,
                        0,
                        0,
                        progress[second].0,
                        route,
                        1,
                        1,
                        progress[second].1,
                        0,
                    ]);
                }
                let mut words = payload();
                observations(&mut words, &rows);
                let expected =
                    (!ordinary || first >= 2) && (second == 5 || (first == 3 && second == 4));
                assert_eq!(
                    decode_payload(&words).is_ok(),
                    expected,
                    "ordinary={ordinary} first={first} second={second}"
                );
            }
        }
    }
    let mut words = payload();
    observations(&mut words, &[[1, 0, 0, 0, 1, 2, 2, 1, 0, 0]]);
    assert!(decode_payload(&words).is_ok());
    observations(
        &mut words,
        &[
            [1, 0, 0, 0, 1, 2, 2, 1, 0, 0],
            [2, 0, 0, 0, 6, 2, 2, 1, 0x100, 0],
        ],
    );
    assert_eq!(decode_payload(&words).unwrap().observations[21], 0x100);
}

#[test]
fn observation_codec_retention_completions_and_negative_matrix() {
    for completion in 1..=4 {
        for witnessed in [0, 1] {
            let rows = [
                [1, 1, 1, 1, 8, 0, 0, 0, 1, 0],
                [2, 1, 1, 1, 8, 0, 0, 0, 3, 0],
                [3, 1, 1, 1, 1, 2, 1, 3, 0, 0],
                [4, 1, 1, 1, 5, 2, 1, 3, 0x103, 0],
                [5, 1, 1, 1, 9, 2, 1, 3, witnessed, completion],
                [6, 1, 1, 1, 7, 1, 1, 0, 3, 2],
            ];
            let mut words = payload();
            words[79..165].copy_from_slice(&lifecycle(&[[1, 1, 1, 0, 0]], 1, 1));
            observations(&mut words, &rows);
            let decoded = decode_payload(&words).unwrap();
            for (field, value) in [
                (184, 0),
                (185, 1),
                (186, 1),
                (187, 1),
                (188, 0),
                (188, 4),
                (189, 1),
                (224, 8),
                (225, 0),
                (226, 0),
                (227, 0),
                (227, 5),
                (228, 2),
                (229, 0),
                (229, 5),
                (235, 2),
                (236, 0),
                (236, 3),
                (237, 1),
                (238, 4),
            ] {
                let mut malformed = words;
                malformed[field] = value;
                assert!(
                    decode_payload(&malformed).is_err(),
                    "{completion}/{witnessed}: {field}={value}"
                );
                assert_eq!(decode_payload(&words).unwrap(), decoded);
            }
            let mut duplicate = rows.to_vec();
            duplicate.push([7, 1, 1, 1, 9, 2, 1, 3, witnessed, completion]);
            observations(&mut words, &duplicate);
            assert!(decode_payload(&words).is_err());
        }
    }
}

#[test]
fn observation_codec_windows_saturation_and_interval_continuity() {
    let mut words = payload();
    words[79..165].copy_from_slice(&lifecycle(&[[1, 1, 1, 0, 0]], 1, 1));
    let before = decode_payload(&words).unwrap();
    observations(
        &mut words,
        &[
            [1, 1, 1, 1, 1, 2, 1, 1, 0, 0],
            [2, 1, 1, 1, 5, 2, 1, 1, 0x103, 0],
        ],
    );
    let after = decode_payload(&words).unwrap();
    assert!(complete_interval(&before, &after));
    for field in [0, 6, 24] {
        let mut changed = words;
        changed[field] += 1;
        assert!(!complete_interval(
            &before,
            &decode_payload(&changed).unwrap()
        ));
    }
    let mut changed = after.clone();
    changed.observations[5..7].fill(0);
    assert!(!complete_interval(&after, &changed));
    let mut overflow: Vec<_> = (3..=33).map(|n| [n, 1, 1, 1, 7, 0, 0, 0, 1, 1]).collect();
    overflow.push([34, 1, 1, 1, 5, 2, 1, 1, 0x103, 0]);
    observations(&mut words, &overflow);
    let overflowed = decode_payload(&words).unwrap();
    assert_eq!(overflowed.observations[3], 3);
    assert_eq!(overflowed.observations[320], 1);
    assert!(!complete_interval(&before, &overflowed));
    observations(
        &mut words,
        &[
            [1, 1, 1, 1, 1, 2, 1, 1, 0, 0],
            [2, 1, 1, 1, 5, 2, 1, 1, 0x103, 0],
        ],
    );
    words[79..165].copy_from_slice(&lifecycle(
        &(3..=18).map(|n| [n, 8, 1, 2, 0]).collect::<Vec<_>>(),
        1,
        1,
    ));
    assert!(!complete_interval(
        &before,
        &decode_payload(&words).unwrap()
    ));
    let mut saturated = payload();
    observations(
        &mut saturated,
        &(u32::MAX - 31..=u32::MAX)
            .map(|n| [n, 0, 0, 0, 7, 0, 0, 0, 6, 2])
            .collect::<Vec<_>>(),
    );
    let decoded = decode_payload(&saturated).unwrap();
    assert_eq!(decoded.observations[0..3], [u32::MAX, 1, 32]);
    saturated[79..90].copy_from_slice(&[1, u32::MAX, 1, 1, 1, 0, 1, 1, 0, 0, 0]);
    observations(&mut saturated, &[[1, 1, 1, 0, 7, 0, 0, 0, 3, 1]]);
    assert!(decode_payload(&saturated).is_ok());
    let frozen_prefix = payload();
    assert_eq!(frozen_prefix[177..192], [0; 15]);
    assert_eq!(
        decode_payload(&frozen_prefix).unwrap().service[0..2],
        [1, 1]
    );
    assert_eq!(
        decode_payload(&frozen_prefix).unwrap().observations,
        [0; 323]
    );
}

#[test]
fn observation_codec_rejects_frozen_mapping_headers() {
    use keyrill_windows_proof::{
        native::{Handle, wide},
        protocol::mapping_name,
    };
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::Win32::System::Memory::*;
    use windows_core::PCWSTR;
    let identity = unsafe {
        capture_identity(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            GetCurrentThreadId(),
        )
        .unwrap()
    };
    let page = ArmedPage::create(identity).unwrap();
    let name = wide(&mapping_name(&identity));
    let mapping = Handle(unsafe {
        OpenFileMappingW(FILE_MAP_WRITE.0, false, PCWSTR(name.as_ptr())).unwrap()
    });
    let view = unsafe { MapViewOfFile(mapping.0, FILE_MAP_WRITE, 0, 0, 4096) };
    assert!(!view.Value.is_null());
    // SAFETY: this privately owned mapping has the frozen eight-DWORD atomic header.
    let header = unsafe { &*view.Value.cast::<[AtomicU32; 8]>() };
    for (version, extent) in [(1, 192), (2, 512), (3, 512)] {
        header[1].store(version, Ordering::SeqCst);
        header[3].store(extent, Ordering::SeqCst);
        assert_eq!(page.read().0, Availability::Incompatible);
    }
    header[1].store(4, Ordering::SeqCst);
    header[3].store(512, Ordering::SeqCst);
    assert_eq!(page.read().0, Availability::Missing);
    unsafe {
        UnmapViewOfFile(view).unwrap();
    }
}
