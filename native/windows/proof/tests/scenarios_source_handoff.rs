use super::*;
use crate::support::providers::{PROFILE_EN, SERVICE_ID, source_profile};
use std::rc::Rc;
use windows::Win32::UI::TextServices::*;

fn source_status(c: &Client) -> u32 {
    let read: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT =
        unsafe { c.api.export(b"KeyrillTestSourceStatus\0") };
    let mut value = 0;
    assert_eq!(unsafe { read(c.handle, &mut value) }, S_OK);
    value
}

fn select_controlled(c: &Client, mode: u32) {
    let profile = source_profile((mode == 2) as usize);
    c.profile_state.profile.set(profile);
    c.profile_state.language.set(profile.langid);
    let language = profile.langid as usize;
    c.profile_state
        .substitute
        .set(HKL((language << 16 | language) as *mut _));
    notify(c, mode, true);
}

fn notify(c: &Client, mode: u32, active: bool) {
    let sink = c
        .service
        .cast::<ITfInputProcessorProfileActivationSink>()
        .unwrap();
    let profile = source_profile((mode == 2) as usize);
    unsafe {
        sink.OnActivated(
            TF_PROFILETYPE_INPUTPROCESSOR,
            profile.langid,
            &SERVICE_ID,
            &GUID_TFCAT_TIP_KEYBOARD,
            &profile.guidProfile,
            profile.hkl,
            if active { TF_IPSINK_FLAG_ACTIVE } else { 0 },
        )
        .unwrap();
    }
}

struct Fixture {
    c: Client,
    keyboard: Keyboard,
    settings: Settings,
    mode: u32,
}
impl Fixture {
    fn new(mode: u32) -> Self {
        let settings = Settings::new();
        settings.save(true, "en.add", None);
        let keyboard = Keyboard::new();
        let c = Client::new();
        select_controlled(&c, mode);
        until(&c, |status| status.effective == 2);
        Self {
            c,
            keyboard,
            settings,
            mode,
        }
    }
    fn boundary(&self) {
        type_text(&self.c, if self.mode == 1 { "cgbcjr " } else { "world " });
        let text = self.word();
        oracle::document(&self.c, text, 7 + text.encode_utf16().count() as i32);
        assert_eq!(source_status(&self.c), 1);
        assert_eq!(self.c.profile_state.language_calls.get(), 0);
        assert_eq!(self.c.profile_state.activation_calls.get(), 0);
        assert_eq!(
            self.c.profile_state.profile.get().guidProfile,
            if self.mode == 1 {
                PROFILE_EN
            } else {
                source_profile(1).guidProfile
            }
        );
    }
    fn word(&self) -> &'static str {
        if self.mode == 1 {
            "список "
        } else {
            "world "
        }
    }
    fn source_calls(&self) -> (u32, u32) {
        (
            self.c.profile_state.language_calls.get(),
            self.c.profile_state.activation_calls.get(),
        )
    }
}

#[test]
fn source_preexisting_target_binding_refuses_before_source_mutation() {
    for mode in [1, 2] {
        for failure in [
            "unsupported",
            "missing",
            "nonexact",
            "failed",
            "disagreeing",
            "profile-missing",
            "high-bits",
            "unloaded",
        ] {
            if failure == "high-bits" && usize::BITS == 32 {
                continue;
            }
            let fixture = Fixture::new(mode);
            fixture.boundary();
            let index = (mode == 1) as usize;
            let mut available = source_profile(index);
            let standard = available.hklSubstitute;
            let actual = match failure {
                "unsupported" => HKL((0x00020000 | available.langid as usize) as *mut _),
                "missing" => HKL::default(),
                "high-bits" => HKL(((1u64 << 32) | standard.0 as u64) as usize as *mut _),
                _ => standard,
            };
            available.hklSubstitute = match failure {
                "disagreeing" => source_profile(1 - index).hklSubstitute,
                "profile-missing" => HKL::default(),
                _ => actual,
            };
            fixture.c.profile_state.available[index].set(available);
            fixture.c.profile_state.bindings[index].set(Some(actual));
            if matches!(failure, "nonexact" | "failed") {
                let state = Rc::clone(&fixture.c.profile_state);
                fixture.c.host.on("target.read", 0, move || {
                    state.results.borrow_mut().insert(
                        "GetSubstituteKeyboardLayout",
                        if failure == "nonexact" {
                            S_FALSE
                        } else {
                            E_FAIL
                        },
                    );
                });
            }
            if failure == "unloaded" {
                let host = Rc::clone(&fixture.c.host);
                fixture.c.host.on("target.read", 0, move || {
                    host.layouts
                        .borrow_mut()
                        .retain(|layout| *layout != standard.0 as isize);
                });
            }
            let before = fixture.c.a.writes.get();
            let eaten = down(&fixture.c, b'G' as u16);
            println!(
                "target binding mode={mode} case={failure} eaten={eaten} source_calls={:?} added_writes={}",
                fixture.source_calls(),
                fixture.c.a.writes.get() - before
            );
            assert_eq!(fixture.source_calls(), (0, 0), "mode={mode}/{failure}");
            assert!(!eaten, "mode={mode}/{failure}");
            assert_eq!(fixture.c.a.writes.get(), before, "mode={mode}/{failure}");
            assert!(fixture.c.a.active.borrow().is_none());
            oracle::document(
                &fixture.c,
                fixture.word(),
                7 + fixture.word().encode_utf16().count() as i32,
            );
            fixture.c.host.drained();
        }
    }
    println!("scenario source.handoff.target-binding.preflight.exact.coherent.loaded.zero-source");
}

#[test]
fn source_target_metadata_callbacks_revoke_before_source_mutation() {
    for mode in [1, 2] {
        for cut in 0..4 {
            let fixture = Fixture::new(mode);
            fixture.boundary();
            let sink = fixture.c.service.cast::<ITfThreadMgrEventSink>().unwrap();
            let host = Rc::clone(&fixture.c.host);
            let injected = Rc::new(std::cell::Cell::new(false));
            let seen = Rc::clone(&injected);
            fixture.c.host.on("target.read", 0, move || {
                let sink = sink.clone();
                let seen = Rc::clone(&seen);
                host.on(
                    if cut == 0 {
                        if mode == 1 {
                            "substitute.ru"
                        } else {
                            "substitute.en"
                        }
                    } else {
                        "Layouts"
                    },
                    if cut == 0 { 0 } else { cut - 1 },
                    move || unsafe {
                        seen.set(true);
                        sink.OnSetFocus(None, None).unwrap();
                    },
                );
            });
            let before = fixture.c.a.writes.get();
            assert!(!down(&fixture.c, b'G' as u16), "mode={mode}/cut={cut}");
            assert!(injected.get(), "mode={mode}/cut={cut}");
            assert_eq!(fixture.source_calls(), (0, 0), "mode={mode}/cut={cut}");
            assert_eq!(fixture.c.a.writes.get(), before, "mode={mode}/cut={cut}");
            oracle::document(
                &fixture.c,
                fixture.word(),
                7 + fixture.word().encode_utf16().count() as i32,
            );
            fixture.c.host.drained();
        }
    }
    println!(
        "scenario source.handoff.target-substitute.loaded-count-fill-recount.focus-reentry.zero-source"
    );
}

#[test]
fn source_both_directions_defer_until_verified_finish_and_retranslate_same_key() {
    for mode in [1, 2] {
        let fixture = Fixture::new(mode);
        fixture.boundary();
        let before = fixture.c.a.writes.get();
        let configuration = runtime(&fixture.c.service).revision;
        let trace_start = fixture.c.host.trace.borrow().len();
        assert!(down(&fixture.c, b'G' as u16));
        let text = format!("{}{}", fixture.word(), if mode == 1 { "п" } else { "g" });
        oracle::document(&fixture.c, &text, 7 + text.encode_utf16().count() as i32);
        assert_eq!(fixture.c.a.writes.get(), before + 1);
        assert_eq!(fixture.source_calls(), (1, 1));
        assert_eq!(source_status(&fixture.c), 3);
        assert_eq!(runtime(&fixture.c.service).revision, configuration);
        let active = fixture.c.profile_state.profile.get();
        assert_eq!(active.langid, if mode == 1 { 0x419 } else { 0x409 });
        assert_eq!(
            active.guidProfile,
            if mode == 1 {
                source_profile(1).guidProfile
            } else {
                PROFILE_EN
            }
        );
        assert!(fixture.c.a.active.borrow().is_some());
        let handoff_end = fixture.c.host.trace.borrow().len();
        let before_reverse = fixture.c.a.writes.get();
        fixture.keyboard.command(&fixture.c, b'Z' as u16);
        oracle::document(&fixture.c, &text, 7 + text.encode_utf16().count() as i32);
        assert_eq!(fixture.c.a.writes.get(), before_reverse);
        assert_eq!(fixture.source_calls(), (1, 1));
        assert_eq!(
            fixture.c.profile_state.profile.get().guidProfile,
            active.guidProfile
        );
        let trace = fixture.c.host.trace.borrow();
        let trace = &trace[trace_start..handoff_end];
        let returned = trace
            .iter()
            .position(|entry| *entry == "RequestEditSession.after")
            .unwrap();
        let source = trace
            .iter()
            .position(|entry| *entry == "language.change")
            .unwrap();
        let second = trace
            .iter()
            .rposition(|entry| *entry == "RequestEditSession")
            .unwrap();
        assert!(returned < source && source < second);
        println!(
            "scenario source.handoff.mode-{mode}.deferred.finish-lock-release.fresh-mark.exact-once"
        );
    }
}

#[test]
fn source_boundary_reversal_escape_manual_and_abstention_never_switch() {
    for key in [b'Z' as u16, VK_ESCAPE.0, b'K' as u16] {
        let fixture = Fixture::new(1);
        fixture.boundary();
        if key == VK_ESCAPE.0 {
            assert!(down(&fixture.c, key));
        } else {
            fixture.keyboard.command(&fixture.c, key);
        }
        assert_eq!(fixture.source_calls(), (0, 0));
        assert_eq!(source_status(&fixture.c), 4);
        oracle::document(&fixture.c, "cgbcjr ", 14);
        assert!(down(&fixture.c, b'G' as u16));
        assert_eq!(fixture.source_calls(), (0, 0));
        oracle::document(&fixture.c, "cgbcjr g", 15);
    }
    for word in ["hello ", "publicword "] {
        let fixture = Fixture::new(1);
        type_text(&fixture.c, word);
        assert_eq!(source_status(&fixture.c), 0);
        assert!(down(&fixture.c, b'G' as u16));
        assert_eq!(fixture.source_calls(), (0, 0));
        let expected = format!("{word}g");
        oracle::document(
            &fixture.c,
            &expected,
            7 + expected.encode_utf16().count() as i32,
        );
    }
    let fixture = Fixture::new(1);
    type_text(&fixture.c, "cgbcjr");
    fixture.keyboard.command(&fixture.c, b'K' as u16);
    assert!(down(&fixture.c, VK_SPACE.0));
    assert_eq!(source_status(&fixture.c), 0);
    assert!(down(&fixture.c, b'G' as u16));
    assert_eq!(fixture.source_calls(), (0, 0));
    oracle::document(&fixture.c, "список g", 15);
    println!("scenario source.handoff.retained-Z.Escape.manual-K.unchanged.abstention");
}

#[test]
fn source_shift_caps_and_nonqualifying_physical_keys_are_bounded() {
    for modifier in [VK_SHIFT, VK_CAPITAL] {
        let fixture = Fixture::new(1);
        fixture.boundary();
        let mut keyboard = [0; 256];
        keyboard[modifier.0 as usize] = if modifier == VK_CAPITAL { 1 } else { 0x80 };
        if modifier == VK_SHIFT {
            keyboard[VK_LSHIFT.0 as usize] = 0x80;
        }
        unsafe {
            SetKeyboardState(&keyboard).unwrap();
        }
        assert!(down(&fixture.c, b'G' as u16));
        oracle::document(&fixture.c, "список П", 15);
        assert_eq!(fixture.source_calls(), (1, 1));
    }
    for (key, literal) in [
        (VK_TAB.0, ""),
        (VK_RETURN.0, ""),
        (VK_OEM_2.0, ""),
        (VK_OEM_5.0, ""),
    ] {
        let fixture = Fixture::new(1);
        fixture.boundary();
        assert_eq!(down(&fixture.c, key), !literal.is_empty());
        assert_eq!(fixture.source_calls(), (0, 0));
        let expected = format!("{}{literal}", fixture.word());
        oracle::document(
            &fixture.c,
            &expected,
            7 + expected.encode_utf16().count() as i32,
        );
    }
    println!("scenario source.handoff.A-Z.Shift.Caps.punctuation-tab-enter-no-source");
}

#[test]
fn source_first_finish_denial_unknown_and_request_mismatch_never_authorize_source() {
    for failure in ["denied", "unknown", "request", "session", "acknowledgement"] {
        let fixture = Fixture::new(1);
        fixture.boundary();
        let before = fixture.c.a.writes.get();
        match failure {
            "denied" => {
                fixture.c.a.invoke.set(false);
                fixture
                    .c
                    .a
                    .results
                    .borrow_mut()
                    .insert("session", TF_E_SYNCHRONOUS);
            }
            "unknown" => {
                fixture
                    .c
                    .a
                    .results
                    .borrow_mut()
                    .insert("EndComposition", E_FAIL);
            }
            "session" => {
                fixture.c.a.results.borrow_mut().insert("session", E_FAIL);
            }
            "acknowledgement" => {
                let sink = fixture.c.service.cast::<ITfThreadMgrEventSink>().unwrap();
                fixture
                    .c
                    .host
                    .on("EndComposition.after", 0, move || unsafe {
                        sink.OnSetFocus(None, None).unwrap();
                    });
            }
            _ => {
                fixture
                    .c
                    .a
                    .results
                    .borrow_mut()
                    .insert("RequestEditSession", S_FALSE);
            }
        }
        assert!(!down(&fixture.c, b'G' as u16));
        assert_eq!(fixture.source_calls(), (0, 0));
        assert_eq!(fixture.c.a.writes.get(), before);
        oracle::document(&fixture.c, fixture.word(), 14);
    }
    println!("scenario source.handoff.first-lock-denial.unknown.invalid-ack");
}

#[test]
fn source_failures_stop_without_retry_rollback_or_new_mark() {
    for failure in [
        "unavailable",
        "disabled",
        "language-before",
        "language-after",
        "activate-before",
        "activate-after",
        "language-mismatch",
        "profile-mismatch",
        "substitute",
        "readback",
    ] {
        let fixture = Fixture::new(1);
        fixture.boundary();
        let before = fixture.c.a.writes.get();
        match failure {
            "unavailable" => {
                fixture
                    .c
                    .profile_state
                    .results
                    .borrow_mut()
                    .insert("GetProfile", E_FAIL);
            }
            "disabled" => fixture.c.profile_state.enabled.set(BOOL(0)),
            "language-before" => {
                fixture.c.profile_state.language_effect.set(false);
                fixture
                    .c
                    .profile_state
                    .results
                    .borrow_mut()
                    .insert("ChangeCurrentLanguage", E_FAIL);
            }
            "language-after" => {
                fixture
                    .c
                    .profile_state
                    .results
                    .borrow_mut()
                    .insert("ChangeCurrentLanguage", E_FAIL);
            }
            "activate-before" => {
                fixture.c.profile_state.activation_effect.set(false);
                fixture
                    .c
                    .profile_state
                    .results
                    .borrow_mut()
                    .insert("ActivateProfile", E_FAIL);
            }
            "activate-after" => {
                fixture
                    .c
                    .profile_state
                    .results
                    .borrow_mut()
                    .insert("ActivateProfile", E_FAIL);
            }
            "language-mismatch" => {
                let state = Rc::clone(&fixture.c.profile_state);
                fixture
                    .c
                    .host
                    .on("language.change", 0, move || state.language.set(0x409));
            }
            "profile-mismatch" => {
                let state = Rc::clone(&fixture.c.profile_state);
                fixture.c.host.on("profile.activate", 0, move || {
                    let mut profile = state.profile.get();
                    profile.guidProfile = PROFILE_EN;
                    state.profile.set(profile);
                });
            }
            "substitute" => {
                let state = Rc::clone(&fixture.c.profile_state);
                fixture.c.host.on("profile.activate", 0, move || {
                    state.substitute.set(HKL::default())
                });
            }
            _ => {
                let state = Rc::clone(&fixture.c.profile_state);
                fixture.c.host.on("language.change", 0, move || {
                    state
                        .results
                        .borrow_mut()
                        .insert("GetCurrentLanguage", E_FAIL);
                });
            }
        }
        assert!(!down(&fixture.c, b'G' as u16), "{failure}");
        assert_eq!(fixture.c.a.writes.get(), before, "{failure}");
        assert!(fixture.source_calls().0 <= 1 && fixture.source_calls().1 <= 1);
        assert!(fixture.c.a.active.borrow().is_none());
        oracle::document(&fixture.c, fixture.word(), 14);
        fixture.c.host.drained();
    }
    println!(
        "scenario source.handoff.unavailable.disabled.failure-before-after.readback-mismatch.no-retry"
    );
}

#[test]
fn source_reentry_focus_activation_and_callbacks_cancel_irrevocably() {
    for boundary in [
        "target.read",
        "substitute.ru",
        "enabled.read",
        "language.change",
        "profile.activate",
    ] {
        for event in [
            "input",
            "release",
            "focus",
            "activation",
            "reactivation",
            "duplicate",
            "foreign",
            "type",
            "language",
            "category",
            "order",
        ] {
            let fixture = Fixture::new(1);
            fixture.boundary();
            let service = fixture.c.service.as_raw() as usize;
            let sink = fixture.c.manager.key_sink.borrow().clone().unwrap();
            let context = fixture.c.context_a.clone();
            let thread_sink = fixture.c.service.cast::<ITfThreadMgrEventSink>().unwrap();
            let manager = fixture.c.thread.clone();
            let profile_sink = fixture
                .c
                .service
                .cast::<ITfInputProcessorProfileActivationSink>()
                .unwrap();
            fixture.c.host.on(boundary, 0, move || unsafe {
                match event {
                    "input" => {
                        let mut eaten = BOOL(0);
                        assert_eq!(
                            (sink.sink.vtable().OnKeyDown)(
                                sink.sink.as_raw(),
                                context.as_raw(),
                                WPARAM(b'H' as usize),
                                LPARAM(1 | 0x23 << 16),
                                &mut eaten,
                            ),
                            S_OK
                        );
                    }
                    "focus" => {
                        thread_sink.OnSetFocus(None, None).unwrap();
                        thread_sink.OnSetFocus(None, None).unwrap();
                    }
                    "release" => {
                        let mut eaten = BOOL(1);
                        assert_eq!(
                            (sink.sink.vtable().OnKeyUp)(
                                sink.sink.as_raw(),
                                context.as_raw(),
                                WPARAM(b'H' as usize),
                                LPARAM(0xc0000001u32 as isize),
                                &mut eaten
                            ),
                            S_OK
                        );
                        assert!(!eaten.as_bool());
                    }
                    "activation" | "reactivation" => {
                        let pointer = service as *mut c_void;
                        let table = *(pointer as *const *const ITfTextInputProcessorEx_Vtbl);
                        assert_eq!(((*table).base__.Deactivate)(pointer), S_OK);
                        if event == "reactivation" {
                            assert_eq!(
                                ((*table).ActivateEx)(pointer, manager.as_raw(), 42, 0),
                                E_INVALIDARG
                            );
                        }
                    }
                    "duplicate" => {
                        for _ in 0..2 {
                            profile_sink
                                .OnActivated(
                                    TF_PROFILETYPE_INPUTPROCESSOR,
                                    0x419,
                                    &SERVICE_ID,
                                    &GUID_TFCAT_TIP_KEYBOARD,
                                    &source_profile(1).guidProfile,
                                    HKL::default(),
                                    TF_IPSINK_FLAG_ACTIVE,
                                )
                                .unwrap();
                        }
                    }
                    _ => {
                        profile_sink
                            .OnActivated(
                                if event == "type" {
                                    TF_PROFILETYPE_KEYBOARDLAYOUT
                                } else {
                                    TF_PROFILETYPE_INPUTPROCESSOR
                                },
                                if event == "language" { 0x407 } else { 0x419 },
                                &if event == "foreign" {
                                    windows_core::GUID::zeroed()
                                } else {
                                    SERVICE_ID
                                },
                                &if event == "category" {
                                    windows_core::GUID::zeroed()
                                } else {
                                    GUID_TFCAT_TIP_KEYBOARD
                                },
                                &source_profile(1).guidProfile,
                                HKL::default(),
                                TF_IPSINK_FLAG_ACTIVE,
                            )
                            .unwrap();
                        if event == "order" {
                            profile_sink
                                .OnActivated(
                                    TF_PROFILETYPE_INPUTPROCESSOR,
                                    0x409,
                                    &SERVICE_ID,
                                    &GUID_TFCAT_TIP_KEYBOARD,
                                    &PROFILE_EN,
                                    HKL::default(),
                                    0,
                                )
                                .unwrap();
                        }
                    }
                }
            });
            assert!(!down(&fixture.c, b'G' as u16), "{boundary}/{event}");
            assert!(fixture.source_calls().0 <= 1 && fixture.source_calls().1 <= 1);
            if matches!(boundary, "target.read" | "substitute.ru" | "enabled.read") {
                assert_eq!(fixture.source_calls(), (0, 0), "{boundary}/{event}");
            }
            let nested_mark = event == "input"
                && matches!(boundary, "target.read" | "substitute.ru" | "enabled.read");
            oracle::document(
                &fixture.c,
                if nested_mark {
                    "список h"
                } else {
                    "список "
                },
                if nested_mark { 15 } else { 14 },
            );
            fixture.c.host.drained();
            if event == "reactivation" {
                assert_eq!(fixture.c.reactivate(), S_OK);
            }
        }
    }
    println!("scenario source.handoff.reentrant-input.focus-A-B-A.activation.duplicate.foreign");
}

#[test]
fn source_callback_capable_retirement_revokes_before_source_mutation() {
    let fixture = Fixture::new(1);
    fixture.boundary();
    let read: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT =
        unsafe { fixture.c.api.export(b"KeyrillTestSourceStatus\0") };
    let handle = fixture.c.handle as usize;
    let thread = fixture.c.service.cast::<ITfThreadMgrEventSink>().unwrap();
    let injected = Rc::new(std::cell::Cell::new(false));
    let seen = Rc::clone(&injected);
    *fixture.c.host.observer.borrow_mut() = Some(Rc::new(move |boundary| {
        if boundary != "Release.before" || seen.get() {
            return;
        }
        let mut status = 0;
        assert_eq!(unsafe { read(handle as *mut c_void, &mut status) }, S_OK);
        if status == 2 {
            seen.set(true);
            unsafe {
                thread.OnSetFocus(None, None).unwrap();
            }
        }
    }));
    assert!(!down(&fixture.c, b'G' as u16));
    fixture.c.host.observer.borrow_mut().take();
    assert!(injected.get());
    assert_eq!(fixture.source_calls(), (0, 0));
    assert_eq!(source_status(&fixture.c), 4);
    oracle::document(&fixture.c, fixture.word(), 14);
    println!(
        "scenario source.handoff.callback-capable-frame-reference-retirement.revokes-before-source"
    );
}

#[test]
fn source_pending_disable_vetoes_before_mutation_and_during_language_call() {
    for during in [false, true] {
        for preference in ["source", "automatic", "manual", "off"] {
            let fixture = Fixture::new(1);
            fixture.boundary();
            let mut bytes = fixture.settings.bytes(
                preference != "automatic",
                "en.add",
                match preference {
                    "manual" => Some(1),
                    "off" => Some(2),
                    _ => None,
                },
            );
            if preference == "source" {
                bytes[29] = 0;
            }
            if during {
                let key = fixture.settings.key;
                fixture.c.host.on("language.change", 0, move || unsafe {
                    RegSetValueExW(key, w!("Preferences"), None, REG_BINARY, Some(&bytes))
                        .ok()
                        .unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(1100));
                });
            } else {
                fixture.settings.write(&bytes);
                std::thread::sleep(std::time::Duration::from_millis(1100));
            }
            let before = fixture.c.a.writes.get();
            assert!(!down(&fixture.c, b'G' as u16));
            assert_eq!(fixture.c.a.writes.get(), before, "{during}/{preference}");
            assert_eq!(fixture.source_calls(), (u32::from(during), 0));
            assert_eq!(source_status(&fixture.c), 4);
            oracle::document(&fixture.c, fixture.word(), 14);
        }
    }
    println!(
        "scenario source.handoff.pending-source-disable.global-off.manual.application-off.before-during"
    );
}

#[test]
fn source_target_already_selected_skips_activation_and_late_callbacks_expire_authority() {
    let fixture = Fixture::new(1);
    fixture.boundary();
    let state = Rc::clone(&fixture.c.profile_state);
    fixture.c.host.on("language.change", 0, move || {
        state.profile.set(source_profile(1))
    });
    assert!(down(&fixture.c, b'G' as u16));
    assert_eq!(fixture.source_calls(), (1, 0));
    oracle::document(&fixture.c, "список п", 15);
    let before = fixture.c.a.writes.get();
    notify(&fixture.c, 2, true);
    let mut keyboard = [0; 256];
    for modifier in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_LMENU] {
        keyboard[modifier.0 as usize] = 0x80;
    }
    unsafe {
        SetKeyboardState(&keyboard).unwrap();
    }
    assert!(!down(&fixture.c, b'Z' as u16));
    assert_eq!(fixture.c.a.writes.get(), before);
    assert_eq!(fixture.source_calls(), (1, 0));
    oracle::document(&fixture.c, "список п", 15);
    println!(
        "scenario source.handoff.already-target.no-redundant-activation.late-callback-no-receipt"
    );
}

#[test]
fn source_second_lock_denial_and_attempted_unknown_keep_effect_derived_disposition() {
    for unknown in [false, true] {
        let fixture = Fixture::new(1);
        fixture.boundary();
        let document = Rc::clone(&fixture.c.a);
        if unknown {
            fixture.c.host.on("SetText.before", 0, move || {
                document.results.borrow_mut().insert("SetText", E_FAIL);
            });
        } else {
            fixture.c.host.on("RequestEditSession", 1, move || {
                document.invoke.set(false);
                document
                    .results
                    .borrow_mut()
                    .insert("session", TF_E_SYNCHRONOUS);
            });
        }
        let before = fixture.c.a.writes.get();
        assert_eq!(down(&fixture.c, b'G' as u16), unknown);
        assert_eq!(fixture.source_calls(), (1, 1));
        assert_eq!(fixture.c.a.writes.get(), before + u32::from(unknown));
        oracle::document(
            &fixture.c,
            if unknown {
                "список п"
            } else {
                "список "
            },
            14,
        );
        fixture.c.host.drained();
    }
    println!("scenario source.handoff.second-lock-denial.second-mark-unknown.effect-disposition");
}

#[test]
fn source_sensitive_or_uncertain_controls_have_zero_new_reads_and_mutations() {
    for after_boundary in [false, true] {
        for kind in ["password", "readonly", "unknown", "foreign"] {
            let fixture = Fixture::new(1);
            if after_boundary {
                fixture.boundary();
            }
            match kind {
                "password" => fixture.c.a.scope.set(IS_PASSWORD),
                "readonly" => fixture.c.a.status.set(TF_SD_READONLY),
                "unknown" => fixture
                    .c
                    .a
                    .scope_type
                    .set(windows::Win32::System::Variant::VT_EMPTY),
                _ => fixture.c.a.foreign.set(true),
            }
            let before = (
                text_reads(&fixture.c),
                fixture.c.a.writes.get(),
                fixture.c.a.starts.get(),
            );
            assert!(!down(&fixture.c, b'G' as u16));
            assert_eq!(
                (
                    text_reads(&fixture.c),
                    fixture.c.a.writes.get(),
                    fixture.c.a.starts.get()
                ),
                before
            );
            assert_eq!(fixture.source_calls(), (0, 0));
        }
    }
    println!("scenario source.handoff.sensitive.uncertain.foreign.zero-inspection.zero-source");
}

#[test]
fn source_scope_change_inside_native_call_grants_no_character_authority() {
    for observed in [false, true] {
        let fixture = Fixture::new(1);
        fixture.boundary();
        let document = Rc::clone(&fixture.c.a);
        let host = Rc::clone(&fixture.c.host);
        let context = fixture.c.context_a.clone();
        let snapshot = Rc::new(std::cell::Cell::new(None));
        let snapshot_at_call = Rc::clone(&snapshot);
        fixture.c.host.on("language.change", 0, move || {
            let reads = host
                .trace
                .borrow()
                .iter()
                .filter(|boundary| boundary.starts_with("GetText"))
                .count();
            snapshot_at_call.set(Some((reads, document.writes.get(), document.starts.get())));
            document.scope.set(IS_PASSWORD);
            if observed {
                let sinks: Vec<_> = host
                    .watches
                    .borrow()
                    .iter()
                    .filter(|watch| watch.id == ITfStatusSink::IID)
                    .map(|watch| watch.sink.clone())
                    .collect();
                for sink in sinks {
                    unsafe {
                        sink.cast::<ITfStatusSink>()
                            .unwrap()
                            .OnStatusChange(&context, 0)
                            .unwrap();
                    }
                }
            }
        });
        assert!(!down(&fixture.c, b'G' as u16));
        assert_eq!(
            (
                text_reads(&fixture.c),
                fixture.c.a.writes.get(),
                fixture.c.a.starts.get()
            ),
            snapshot.get().unwrap()
        );
        assert_eq!(fixture.source_calls(), (1, u32::from(!observed)));
        oracle::document(&fixture.c, fixture.word(), 14);
        fixture.c.host.drained();
    }
    println!(
        "scenario source.handoff.unlocked-scope-last-verified-at-finish.silent-private-change.zero-character-authority"
    );
}

#[test]
fn source_unavailable_layout_repeats_keyboard_drift_and_expired_callbacks_do_not_switch() {
    let fixture = Fixture::new(1);
    fixture.boundary();
    fixture
        .c
        .host
        .layouts
        .borrow_mut()
        .retain(|layout| *layout != 0x04190419);
    assert!(down(&fixture.c, b'G' as u16));
    assert_eq!(fixture.source_calls(), (0, 0));
    oracle::document(&fixture.c, "список g", 15);
    drop(fixture);
    for repeat in [1u32 | 0x40000000, 2] {
        let fixture = Fixture::new(1);
        fixture.boundary();
        let sink = fixture.c.manager.key_sink.borrow().clone().unwrap();
        let mut eaten = BOOL(0);
        unsafe {
            assert_eq!(
                (sink.sink.vtable().OnKeyDown)(
                    sink.sink.as_raw(),
                    fixture.c.context_a.as_raw(),
                    WPARAM(b'G' as usize),
                    LPARAM((repeat | 0x22 << 16) as isize),
                    &mut eaten
                ),
                S_OK
            );
        }
        assert_eq!(eaten.as_bool(), repeat != 2);
        assert_eq!(fixture.source_calls(), (0, 0));
        oracle::document(
            &fixture.c,
            if repeat == 2 {
                "список "
            } else {
                "список g"
            },
            if repeat == 2 { 14 } else { 15 },
        );
    }
    for boundary in ["target.read", "language.change", "profile.activate"] {
        let fixture = Fixture::new(1);
        fixture.boundary();
        fixture.c.host.on(boundary, 0, || unsafe {
            let mut keyboard = [0; 256];
            keyboard[VK_SHIFT.0 as usize] = 0x80;
            keyboard[VK_LSHIFT.0 as usize] = 0x80;
            SetKeyboardState(&keyboard).unwrap();
        });
        assert!(!down(&fixture.c, b'G' as u16));
        assert_eq!(
            fixture.source_calls(),
            match boundary {
                "target.read" => (0, 0),
                "language.change" => (1, 0),
                _ => (1, 1),
            }
        );
        oracle::document(&fixture.c, fixture.word(), 14);
    }
    let fixture = Fixture::new(1);
    fixture.boundary();
    fixture.c.a.invoke.set(false);
    fixture
        .c
        .a
        .results
        .borrow_mut()
        .insert("session", TF_E_SYNCHRONOUS);
    assert!(!down(&fixture.c, b'G' as u16));
    let retained = fixture
        .c
        .a
        .retained
        .borrow_mut()
        .take()
        .unwrap()
        .cast::<ITfEditSession>()
        .unwrap();
    let before = (text_reads(&fixture.c), fixture.c.a.writes.get());
    unsafe {
        assert_eq!(
            (retained.vtable().DoEditSession)(retained.as_raw(), 123),
            E_FAIL
        );
    }
    assert_eq!((text_reads(&fixture.c), fixture.c.a.writes.get()), before);
    assert_eq!(fixture.source_calls(), (0, 0));
    oracle::document(&fixture.c, fixture.word(), 14);
    println!("scenario source.handoff.missing-target-layout.repeats.keyboard-drift.expired-ticket");
}

#[test]
fn source_matching_observations_require_actual_readback_and_fresh_authority() {
    for mode in [1, 2] {
        for already in [false, true] {
            let fixture = Fixture::new(mode);
            fixture.boundary();
            let sink = fixture
                .c
                .service
                .cast::<ITfInputProcessorProfileActivationSink>()
                .unwrap();
            let state = Rc::clone(&fixture.c.profile_state);
            fixture.c.host.on("language.change", 0, move || unsafe {
                let from = source_profile((mode == 2) as usize);
                sink.OnActivated(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    from.langid,
                    &SERVICE_ID,
                    &GUID_TFCAT_TIP_KEYBOARD,
                    &from.guidProfile,
                    HKL::default(),
                    0,
                )
                .unwrap();
                if already {
                    let target = source_profile((mode == 1) as usize);
                    state.profile.set(target);
                    sink.OnActivated(
                        TF_PROFILETYPE_INPUTPROCESSOR,
                        target.langid,
                        &SERVICE_ID,
                        &GUID_TFCAT_TIP_KEYBOARD,
                        &target.guidProfile,
                        HKL::default(),
                        TF_IPSINK_FLAG_ACTIVE,
                    )
                    .unwrap();
                }
            });
            if !already {
                let sink = fixture
                    .c
                    .service
                    .cast::<ITfInputProcessorProfileActivationSink>()
                    .unwrap();
                fixture.c.host.on("profile.activate", 0, move || unsafe {
                    let target = source_profile((mode == 1) as usize);
                    sink.OnActivated(
                        TF_PROFILETYPE_INPUTPROCESSOR,
                        target.langid,
                        &SERVICE_ID,
                        &GUID_TFCAT_TIP_KEYBOARD,
                        &target.guidProfile,
                        HKL::default(),
                        TF_IPSINK_FLAG_ACTIVE,
                    )
                    .unwrap();
                });
            }
            assert!(down(&fixture.c, b'G' as u16));
            assert_eq!(fixture.source_calls(), (1, u32::from(!already)));
            let text = format!("{}{}", fixture.word(), if mode == 1 { "п" } else { "g" });
            oracle::document(&fixture.c, &text, 7 + text.encode_utf16().count() as i32);
            fixture.c.host.drained();
        }
    }
    let fixture = Fixture::new(1);
    fixture.boundary();
    fixture.c.profile_state.activation_effect.set(false);
    let sink = fixture
        .c
        .service
        .cast::<ITfInputProcessorProfileActivationSink>()
        .unwrap();
    fixture.c.host.on("language.change", 0, move || unsafe {
        sink.OnActivated(
            TF_PROFILETYPE_INPUTPROCESSOR,
            0x419,
            &SERVICE_ID,
            &GUID_TFCAT_TIP_KEYBOARD,
            &source_profile(1).guidProfile,
            HKL::default(),
            TF_IPSINK_FLAG_ACTIVE,
        )
        .unwrap();
    });
    assert!(!down(&fixture.c, b'G' as u16));
    assert_eq!(fixture.source_calls(), (1, 1));
    oracle::document(&fixture.c, fixture.word(), 14);
    println!(
        "scenario source.handoff.bounded-matching-observations.no-causal-attribution.readback-required"
    );
}

#[test]
fn source_unchanged_worker_publications_keep_intent_and_pending_snapshot() {
    let fixture = Fixture::new(1);
    fixture.boundary();
    let before = runtime(&fixture.c.service);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let maintain: unsafe extern "system" fn(*mut c_void) -> HRESULT =
        unsafe { fixture.c.api.export(b"KeyrillTestMaintain\0") };
    assert_eq!(unsafe { maintain(fixture.c.handle) }, S_OK);
    assert_eq!(source_status(&fixture.c), 1);
    assert_eq!(runtime(&fixture.c.service).revision, before.revision);
    assert!(down(&fixture.c, b'G' as u16));
    assert_eq!(fixture.source_calls(), (1, 1));
    assert_eq!(
        (
            runtime(&fixture.c.service).activation,
            runtime(&fixture.c.service).revision
        ),
        (before.activation, before.revision)
    );
    oracle::document(&fixture.c, "список п", 15);
    assert!(!down(&fixture.c, VK_TAB.0));
    until(&fixture.c, |status| status.revision > before.revision);
    oracle::document(&fixture.c, "список п", 15);
    println!("scenario source.handoff.normalized-configuration-identity.pending-ready-preserved");
}

#[test]
fn source_pc_oem_letters_use_actual_finish_and_target_translation() {
    let letters = [
        (VK_OEM_4, 'х', 'Х'),
        (VK_OEM_6, 'ъ', 'Ъ'),
        (VK_OEM_1, 'ж', 'Ж'),
        (VK_OEM_7, 'э', 'Э'),
        (VK_OEM_COMMA, 'б', 'Б'),
        (VK_OEM_PERIOD, 'ю', 'Ю'),
        (VK_OEM_3, 'ё', 'Ё'),
    ];
    for mode in [1, 2] {
        for modifier in [None, Some(VK_SHIFT), Some(VK_CAPITAL)] {
            for (key, lower, upper) in letters {
                let fixture = Fixture::new(mode);
                fixture.boundary();
                let mut keyboard = [0; 256];
                if let Some(modifier) = modifier {
                    keyboard[modifier.0 as usize] = if modifier == VK_CAPITAL { 1 } else { 0x80 };
                    if modifier == VK_SHIFT {
                        keyboard[VK_LSHIFT.0 as usize] = 0x80;
                    }
                }
                unsafe {
                    SetKeyboardState(&keyboard).unwrap();
                }
                let before = fixture.c.a.writes.get();
                assert!(down(&fixture.c, key.0));
                let expected = format!(
                    "{}{}",
                    fixture.word(),
                    if modifier.is_some() { upper } else { lower }
                );
                oracle::document(
                    &fixture.c,
                    &expected,
                    7 + expected.encode_utf16().count() as i32,
                );
                assert_eq!(fixture.c.a.writes.get(), before + 1);
                assert_eq!(
                    fixture.source_calls(),
                    if mode == 1 { (1, 1) } else { (0, 0) }
                );
            }
        }
    }
    let fixture = Fixture::new(1);
    fixture.boundary();
    fixture
        .c
        .profile_state
        .substitute
        .set(HKL(0x00020409usize as *mut _));
    let before = (
        text_reads(&fixture.c),
        fixture.c.a.starts.get(),
        fixture.c.a.writes.get(),
    );
    assert!(!down(&fixture.c, VK_OEM_7.0));
    assert_eq!(
        (
            text_reads(&fixture.c),
            fixture.c.a.starts.get(),
            fixture.c.a.writes.get()
        ),
        before
    );
    assert_eq!(fixture.source_calls(), (0, 0));
    oracle::document(&fixture.c, fixture.word(), 14);
    println!(
        "scenario source.handoff.PC-OEM-seven-letters.Shift.Caps.yo.target-punctuation.unsupported-dead-layout"
    );
}

#[test]
fn source_changed_complete_configuration_cancels_prepared_staged_and_inflight_intent() {
    for dictionary in [false, true] {
        for point in ["prepared", "staged", "during"] {
            let fixture = Fixture::new(1);
            fixture.boundary();
            let before = runtime(&fixture.c.service);
            let key = fixture.settings.key;
            let bytes = fixture.settings.bytes(true, "en.add", None);
            let path = fixture.settings.files.path("en.add");
            let publish = move || {
                if dictionary {
                    std::fs::write(&path, b"publicword\n").unwrap();
                } else {
                    unsafe {
                        RegSetValueExW(key, w!("Preferences"), None, REG_BINARY, Some(&bytes))
                    }
                    .ok()
                    .unwrap();
                }
            };
            if point == "during" {
                fixture.c.host.on("language.change", 0, move || {
                    publish();
                    std::thread::sleep(std::time::Duration::from_millis(1600));
                });
            } else {
                publish();
                std::thread::sleep(std::time::Duration::from_millis(1600));
                if point == "staged" {
                    let maintain: unsafe extern "system" fn(*mut c_void) -> HRESULT =
                        unsafe { fixture.c.api.export(b"KeyrillTestMaintain\0") };
                    assert_eq!(unsafe { maintain(fixture.c.handle) }, S_OK);
                    assert_eq!(source_status(&fixture.c), 4);
                }
            }
            assert_eq!(down(&fixture.c, b'G' as u16), point == "staged");
            assert_eq!(
                fixture.source_calls(),
                if point == "during" { (1, 0) } else { (0, 0) }
            );
            oracle::document(
                &fixture.c,
                if point == "staged" {
                    "список g"
                } else {
                    "список "
                },
                if point == "staged" { 15 } else { 14 },
            );
            if point != "staged" {
                until(&fixture.c, |status| status.revision > before.revision);
            }
            assert!(runtime(&fixture.c.service).revision > before.revision);
            assert_eq!(runtime(&fixture.c.service).effective, 2);
            fixture.c.host.drained();
        }
    }
    println!(
        "scenario source.handoff.changed-settings-nonce.changed-dictionary-owner.cancel-before-during"
    );
}
