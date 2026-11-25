//! Real isolated HKCU values, the production worker, and advised DLL keyboard entry.
#[path = "scenarios_source_handoff.rs"]
mod source_handoff;
use crate::support::{client::Client, oracle};
use crate::worker_support::{Environment, Files, RuntimeSnapshot, runtime, wait, wide};
use std::ffi::c_void;
use windows::Win32::{
    Foundation::*,
    System::{Com::CoCreateGuid, Registry::*},
    UI::Input::KeyboardAndMouse::*,
};
use windows_core::{BOOL, HRESULT, Interface, PCWSTR, w};

struct Settings {
    key: HKEY,
    path: Vec<u16>,
    _namespace: Environment,
    _policy: Environment,
    _profile: Environment,
    files: Files,
}
impl Settings {
    fn new() -> Self {
        let nonce = unsafe { CoCreateGuid() }.unwrap().to_u128();
        let namespace = format!("{nonce:032x}");
        let path = wide(format!("Software\\KeyrillTests\\Settings-{namespace}"));
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY,
                None,
                &mut key,
                None,
            )
        }
        .ok()
        .unwrap();
        let files = Files::new();
        files.write("en.add", b"");
        files.write("ru.add", b"");
        Self {
            key,
            path,
            _namespace: Environment::set("KEYRILL_WINDOWS_TEST_SETTINGS_NAMESPACE", namespace),
            _policy: Environment::absent("KEYRILL_WINDOWS_POLICY"),
            _profile: Environment::set("USERPROFILE", &files.0),
            files,
        }
    }
    fn bytes(&self, automatic: bool, english: &str, application: Option<u8>) -> Vec<u8> {
        fn text(bytes: &mut Vec<u8>, text: &str) {
            let units: Vec<_> = text.encode_utf16().collect();
            bytes.extend((units.len() as u16).to_le_bytes());
            for unit in units {
                bytes.extend(unit.to_le_bytes());
            }
        }
        let mut bytes = b"KRST\x01\0\0\0\0\0\0\0".to_vec();
        bytes.extend(unsafe { CoCreateGuid() }.unwrap().to_u128().to_le_bytes());
        bytes.extend([u8::from(automatic), 1]);
        for path in [self.files.path(english), self.files.path("ru.add")] {
            bytes.push(1);
            text(&mut bytes, path.to_str().unwrap());
        }
        bytes.extend(u16::from(application.is_some()).to_le_bytes());
        if let Some(policy) = application {
            bytes.push(0);
            let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
            let executable = executable
                .to_str()
                .unwrap()
                .strip_prefix(r"\\?\")
                .unwrap()
                .to_uppercase();
            text(&mut bytes, &executable);
            bytes.push(policy);
        }
        let length = bytes.len() as u32;
        bytes[8..12].copy_from_slice(&length.to_le_bytes());
        bytes
    }
    fn write(&self, bytes: &[u8]) {
        unsafe { RegSetValueExW(self.key, w!("Preferences"), None, REG_BINARY, Some(bytes)) }
            .ok()
            .unwrap();
    }
    fn save(&self, automatic: bool, english: &str, application: Option<u8>) {
        self.write(&self.bytes(automatic, english, application));
    }
    fn remove(&self) {
        unsafe { RegDeleteValueW(self.key, w!("Preferences")) }
            .ok()
            .unwrap();
    }
}
impl Drop for Settings {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.key).ok().unwrap();
            RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(self.path.as_ptr()))
                .ok()
                .unwrap();
        }
    }
}
struct Keyboard([u8; 256]);
impl Keyboard {
    fn new() -> Self {
        let mut state = [0; 256];
        unsafe {
            assert!(crate::support::keyboard_state(&mut state));
            SetKeyboardState(&[0; 256]).unwrap();
        }
        Self(state)
    }
    fn command(&self, c: &Client, key: u16) {
        let mut state = [0; 256];
        for modifier in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_LMENU] {
            state[modifier.0 as usize] = 0x80;
        }
        unsafe {
            SetKeyboardState(&state).unwrap();
        }
        assert!(down(c, key));
        unsafe {
            SetKeyboardState(&[0; 256]).unwrap();
        }
    }
}
impl Drop for Keyboard {
    fn drop(&mut self) {
        unsafe {
            SetKeyboardState(&self.0).unwrap();
        }
    }
}
fn down(c: &Client, key: u16) -> bool {
    let advised = c
        .manager
        .key_sink
        .borrow()
        .clone()
        .expect("actual advised sink");
    let scan = unsafe { MapVirtualKeyW(key as u32, MAPVK_VK_TO_VSC) } as isize;
    let mut eaten = BOOL(1);
    unsafe {
        assert_eq!(
            (advised.sink.vtable().OnKeyDown)(
                advised.sink.as_raw(),
                c.context_a.as_raw(),
                WPARAM(key as usize),
                LPARAM(1 | scan << 16),
                &mut eaten
            ),
            S_OK
        );
    }
    c.host.check();
    eaten.as_bool()
}
fn type_text(c: &Client, text: &str) {
    for byte in text.bytes() {
        assert!(
            down(c, byte.to_ascii_uppercase() as u16),
            "key {byte}, runtime {:?}, snapshot {:?}",
            runtime(&c.service),
            crate::support::dll::snapshot(&c.service)
        );
    }
}
fn until(c: &Client, mut predicate: impl FnMut(RuntimeSnapshot) -> bool) {
    let maintain: unsafe extern "system" fn(*mut c_void) -> HRESULT =
        unsafe { c.api.export(b"KeyrillTestMaintain\0") };
    wait(|| {
        assert_eq!(unsafe { maintain(c.handle) }, S_OK);
        predicate(runtime(&c.service))
    });
}
fn text_reads(c: &Client) -> usize {
    c.host
        .trace
        .borrow()
        .iter()
        .filter(|boundary| boundary.starts_with("GetText"))
        .count()
}

#[test]
fn generic_field_policy_and_command_routes_never_grant_authority() {
    use windows::Win32::UI::TextServices::*;
    use windows_core::GUID;
    let settings = Settings::new();
    let _keyboard = Keyboard::new();
    for policy in [0, 1, 2] {
        settings.save(true, "en.add", Some(policy));
        let c = Client::ordinary("Chrome_WidgetWin_1");
        until(&c, |status| status.revision > 0 && status.model_ready == 1);
        for scope in [IS_DEFAULT, IS_TEXT] {
            c.a.scope.set(scope);
            unsafe { SetKeyboardState(&[0; 256]).unwrap() };
            let sink = c.manager.key_sink.borrow().clone().unwrap().sink.clone();
            let mut eaten = BOOL(0);
            unsafe {
                assert_eq!(
                    (sink.vtable().OnTestKeyDown)(
                        sink.as_raw(),
                        c.context_a.as_raw(),
                        WPARAM(71),
                        LPARAM(1),
                        &mut eaten
                    ),
                    S_OK
                );
            }
            assert!(
                !eaten.as_bool(),
                "unsupported fields must receive native typing under every policy"
            );
            assert!(!down(&c, b'G' as u16));
            let mut chord = [0; 256];
            for key in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_LMENU] {
                chord[key.0 as usize] = 0x80;
            }
            unsafe { SetKeyboardState(&chord).unwrap() };
            for (key, command) in [
                (
                    b'K',
                    GUID::from_u128(0xd7abce42_c8a9_4a43_990a_4a8d6c011ebb),
                ),
                (
                    b'Z',
                    GUID::from_u128(0x16531b70_516e_4dfb_9f18_cb760de590b4),
                ),
            ] {
                assert!(!down(&c, key as u16));
                unsafe {
                    assert_eq!(
                        (sink.vtable().OnPreservedKey)(
                            sink.as_raw(),
                            c.context_a.as_raw(),
                            &command,
                            &mut eaten
                        ),
                        S_OK
                    );
                }
                assert!(!eaten.as_bool(), "denied preserved command passes");
            }
            unsafe {
                SetKeyboardState(&[0; 256]).unwrap();
                assert_eq!(
                    (sink.vtable().OnKeyUp)(
                        sink.as_raw(),
                        c.context_a.as_raw(),
                        WPARAM(VK_MENU.0 as usize),
                        LPARAM(0xc0000001u32 as isize),
                        &mut eaten
                    ),
                    S_OK
                );
            }
            assert!(!eaten.as_bool());
            oracle::document(&c, "", 7);
            oracle::no_effects(&c);
            assert_eq!(text_reads(&c), 0);
            assert!(!c.host.trace.borrow().contains(&"range.ACP"));
            assert!(!c.host.trace.borrow().contains(&"GetSelection"));
            assert_eq!(c.profile_state.language_calls.get(), 0);
            assert_eq!(c.profile_state.activation_calls.get(), 0);
        }
    }
}

#[test]
fn persisted_defaults_manual_automatic_and_off_use_advised_entry() {
    let settings = Settings::new();
    let _unused_developer_dictionary = Environment::set(
        "KEYRILL_WINDOWS_EN_DICTIONARY",
        "invalid without explicit model override",
    );
    let keyboard = Keyboard::new();
    let c = Client::new();
    until(&c, |status| status.revision > 0 && status.model_ready == 1);
    assert_eq!(runtime(&c.service).effective, 0);
    type_text(&c, "ghbdtn");
    oracle::document(&c, "ghbdtn", 13);
    keyboard.command(&c, b'K' as u16);
    oracle::document(&c, "привет", 13);
    keyboard.command(&c, b'Z' as u16);
    oracle::document(&c, "ghbdtn", 13);
    assert!(down(&c, VK_SPACE.0));
    let previous = runtime(&c.service).revision;
    settings.save(true, "en.add", None);
    until(&c, |status| status.requested == 2);
    assert_eq!(runtime(&c.service).effective, 0);
    type_text(&c, "cgbcjr ");
    assert_eq!(runtime(&c.service).effective, 2);
    assert!(
        runtime(&c.service).revision > previous,
        "natural finish adopts before replanning the next word"
    );
    oracle::document(&c, "ghbdtn список ", 21);
    let previous = runtime(&c.service).revision;
    settings.save(true, "missing.add", Some(2));
    until(&c, |status| {
        status.revision > previous && status.effective == 0
    });
    assert_eq!(runtime(&c.service).sources[0], 0);
    assert_eq!(runtime(&c.service).source_errors[0], 3);
    let before = (text_reads(&c), c.a.starts.get(), c.a.writes.get());
    assert!(!down(&c, b'G' as u16));
    assert_eq!((text_reads(&c), c.a.starts.get(), c.a.writes.get()), before);
    oracle::document(&c, "ghbdtn список ", 21);
    settings.remove();
    let previous = runtime(&c.service).revision;
    until(&c, |status| {
        status.revision > previous && status.configuration_error == 0
    });
    assert_eq!(runtime(&c.service).effective, 0);
    type_text(&c, "ghbdtn");
    keyboard.command(&c, b'K' as u16);
    oracle::document(&c, "ghbdtn список привет", 27);
    println!(
        "scenario configuration.persisted.absence-manual.automatic.off-zero-read.reset-defaults"
    );
}

#[test]
fn persisted_invalid_and_rebound_dictionary_keep_complete_revision() {
    let settings = Settings::new();
    let _keyboard = Keyboard::new();
    settings.save(false, "en.add", None);
    let c = Client::new();
    until(&c, |status| status.revision > 0);
    type_text(&c, "gh ");
    let before = runtime(&c.service).revision;
    settings.save(true, "new.add", None);
    until(&c, |status| status.error == 3);
    assert_eq!(runtime(&c.service).revision, before);
    assert_eq!(
        runtime(&c.service).effective,
        0,
        "unready binding cannot apply new automatic policy"
    );
    assert!(c.a.active.borrow().is_some());
    settings.files.write("new.add", b"cgbcjr\n");
    until(&c, |status| status.requested == 2 && status.error == 0);
    assert_eq!(runtime(&c.service).effective, 0);
    assert!(!down(&c, VK_TAB.0));
    until(&c, |status| status.effective == 2 && status.error == 0);
    assert!(c.a.active.borrow().is_none());
    type_text(&c, "cgbcjr ");
    oracle::document(&c, "gh cgbcjr ", 17);
    assert!(!down(&c, VK_TAB.0));
    let before = runtime(&c.service).revision;
    let mut invalid = settings.bytes(false, "en.add", Some(2));
    invalid[4] = 2;
    settings.write(&invalid);
    until(&c, |status| {
        status.revision > before && status.configuration_error == 1
    });
    assert_eq!(
        runtime(&c.service).effective,
        2,
        "invalid complete candidate retains old policy"
    );
    type_text(&c, "cgbcjr ");
    oracle::document(&c, "gh cgbcjr cgbcjr ", 24);
    println!(
        "scenario configuration.persisted.invalid-last-good.rebound-dictionary.whole-owned-retirement"
    );
}

#[test]
fn persisted_preparation_latest_save_wins_and_revoked_worker_cannot_publish() {
    use crate::worker_support::{BEFORE_PUBLISH, BEFORE_READ, Events, WorkerApi};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    for boundary in [BEFORE_READ, BEFORE_PUBLISH] {
        let settings = Settings::new();
        settings.save(true, "en.add", None);
        let api = WorkerApi::load();
        let events = Events::new();
        events.pause.store(u32::MAX, Ordering::Release);
        let changed = Arc::new(AtomicBool::new(false));
        let did_change = Arc::clone(&changed);
        let key = settings.key.0 as usize;
        let replacement = settings.bytes(false, "ru.add", Some(2));
        *events.action.lock().unwrap() = Some(Arc::new(move |at, _, _| {
            if at == boundary && !did_change.swap(true, Ordering::AcqRel) {
                unsafe {
                    RegSetValueExW(
                        HKEY(key as *mut _),
                        w!("Preferences"),
                        None,
                        REG_BINARY,
                        Some(&replacement),
                    )
                }
                .ok()
                .unwrap();
            }
        }));
        assert!(unsafe { (api.set)(&events.hooks()) }.as_bool());
        let c = Client::new();
        assert!(unsafe { (api.set)(std::ptr::null()) }.as_bool());
        until(&c, |status| {
            assert_eq!(
                status.effective, 0,
                "superseded automatic candidate never becomes effective"
            );
            status.revision > 0
        });
        assert!(changed.load(Ordering::Acquire));
        let _keyboard = Keyboard::new();
        assert!(!down(&c, b'G' as u16));
        assert_eq!(text_reads(&c), 0);
        oracle::no_effects(&c);
        assert_eq!(c.deactivate(), S_OK);
        events.finished.wait();
        events.check();
    }
    let settings = Settings::new();
    settings.save(true, "en.add", None);
    let api = WorkerApi::load();
    let events = Events::new();
    events.pause.store(BEFORE_PUBLISH, Ordering::Release);
    assert!(unsafe { (api.set)(&events.hooks()) }.as_bool());
    let c = Client::new();
    assert!(unsafe { (api.set)(std::ptr::null()) }.as_bool());
    events.published.wait();
    assert_eq!(runtime(&c.service).revision, 0);
    assert_eq!(c.deactivate(), S_OK);
    events.resume.signal();
    events.finished.wait();
    events.check();
    assert_eq!(runtime(&c.service).revision, 0);
    assert_eq!(c.reactivate(), S_OK);
    until(&c, |status| status.effective == 2);
    println!(
        "scenario configuration.persisted.latest-before-read.latest-before-publication.cancelled-epoch"
    );
}

#[test]
fn persisted_off_waits_for_verified_owned_finish_and_reentrant_deactivate_wins() {
    let settings = Settings::new();
    let _keyboard = Keyboard::new();
    settings.save(true, "en.add", None);
    let c = Client::new();
    until(&c, |status| status.effective == 2);
    type_text(&c, "gh");
    let before = runtime(&c.service).revision;
    c.a.invoke.set(false);
    c.a.results.borrow_mut().insert(
        "session",
        windows::Win32::UI::TextServices::TF_E_SYNCHRONOUS,
    );
    settings.save(true, "en.add", Some(2));
    until(&c, |status| status.requested == 0);
    assert_eq!(
        (runtime(&c.service).revision, runtime(&c.service).effective),
        (before, 2)
    );
    oracle::document(&c, "gh", 9);
    assert!(c.a.active.borrow().is_some());
    c.a.invoke.set(true);
    c.a.results.borrow_mut().remove("session");
    assert!(
        !down(&c, b'G' as u16),
        "new input only finishes under the old policy and passes once"
    );
    until(&c, |status| {
        status.revision > before && status.effective == 0
    });
    oracle::document(&c, "gh", 9);
    assert!(c.a.active.borrow().is_none());
    let before_reads = text_reads(&c);
    assert!(!down(&c, b'G' as u16));
    assert_eq!(text_reads(&c), before_reads);
    assert_eq!(c.deactivate(), S_OK);
    settings.save(true, "en.add", None);
    c.a.native_text(&[32]);
    assert_eq!(c.reactivate(), S_OK);
    until(&c, |status| status.effective == 2);
    type_text(&c, "gh");
    let service = c.service.as_raw() as usize;
    c.host.on("GetStatus", 0, move || unsafe {
        let pointer = service as *mut c_void;
        let table = *(pointer
            as *const *const windows::Win32::UI::TextServices::ITfTextInputProcessorEx_Vtbl);
        assert_eq!(((*table).base__.Deactivate)(pointer), S_OK);
    });
    settings.save(false, "en.add", Some(2));
    until(&c, |_| {
        crate::support::dll::snapshot(&c.service).active == 0
    });
    c.host.drained();
    oracle::document(&c, "gh gh", 12);
    println!(
        "scenario configuration.persisted.denied-retirement.pending-blocks-input.reentrant-deactivation"
    );
}

#[test]
fn restrictive_retirement_replaces_unchanged_text_and_unknown_end_blocks_retirement() {
    for unknown in [false, true] {
        let settings = Settings::new();
        let _keyboard = Keyboard::new();
        settings.save(true, "en.add", None);
        let c = Client::new();
        until(&c, |status| status.effective == 2);
        type_text(&c, "gh");
        let before = (c.a.writes.get(), c.a.ends.get());
        if unknown {
            c.a.results.borrow_mut().insert("EndComposition", E_FAIL);
            assert!(!c.event(crate::support::dll::Event::command(6)));
            assert_eq!((c.a.writes.get(), c.a.ends.get()), (before.0, before.1 + 1));
            c.a.results.borrow_mut().remove("EndComposition");
        }
        settings.save(true, "en.add", Some(2));
        until(&c, |status| {
            status.requested == 0 && (unknown || status.effective == 0)
        });
        assert_eq!(
            (c.a.writes.get(), c.a.ends.get()),
            (before.0 + u32::from(!unknown), before.1 + 1)
        );
        assert!(c.a.active.borrow().is_none());
        oracle::document(&c, "gh", 9);
        c.host.drained();
    }
}

#[test]
fn explicit_policy_validates_only_its_dictionary_bindings() {
    let _policy = Environment::set("KEYRILL_WINDOWS_POLICY", "fixture");
    let _keyboard = Keyboard::new();
    let c = Client::new();
    assert_eq!(runtime(&c.service).effective, 1);
    assert_eq!(c.deactivate(), S_OK);
    let _off = Environment::set("KEYRILL_WINDOWS_POLICY", "off");
    let _bad = Environment::set(
        "KEYRILL_WINDOWS_EN_DICTIONARY",
        "not an absolute local path",
    );
    assert_eq!(c.reactivate(), S_OK);
    assert_eq!(
        runtime(&c.service).effective,
        0,
        "explicit off must supersede prior fixture without consuming irrelevant dictionary paths"
    );
    assert_eq!(runtime(&c.service).configuration_error, 0);
    for (policy, effective, error) in [
        ("fixture", 1, 0),
        ("model", 1, 1),
        ("unknown", 1, 1),
        ("off", 0, 0),
    ] {
        assert_eq!(c.deactivate(), S_OK);
        let _policy = Environment::set("KEYRILL_WINDOWS_POLICY", policy);
        assert_eq!(c.reactivate(), S_OK);
        let status = runtime(&c.service);
        assert_eq!(
            (status.effective, status.configuration_error),
            (effective, error)
        );
    }
    assert_eq!(c.deactivate(), S_OK);
    let files = Files::new();
    let _english = Environment::set("KEYRILL_WINDOWS_EN_DICTIONARY", files.path("en.add"));
    let _russian = Environment::set("KEYRILL_WINDOWS_RU_DICTIONARY", "invalid russian binding");
    let _model = Environment::set("KEYRILL_WINDOWS_POLICY", "model");
    assert_eq!(c.reactivate(), S_OK);
    let status = runtime(&c.service);
    assert_eq!((status.effective, status.configuration_error), (0, 1));
}

#[test]
fn pending_off_survives_exact_host_termination() {
    pending_off_survives_invalidation("termination");
}

#[test]
fn pending_off_survives_focus_invalidation() {
    pending_off_survives_invalidation("focus");
}

#[test]
fn pending_off_survives_source_invalidation() {
    pending_off_survives_invalidation("source");
}

#[test]
fn pending_off_survives_unknown_retirement() {
    pending_off_survives_invalidation("unknown");
}

#[test]
fn pending_off_is_cancelled_by_new_activation() {
    use crate::worker_support::{BEFORE_PUBLISH, Events, WorkerApi};
    use std::sync::atomic::Ordering;
    let settings = Settings::new();
    let _keyboard = Keyboard::new();
    settings.save(true, "en.add", None);
    let api = WorkerApi::load();
    let old = Events::new();
    old.pause.store(u32::MAX, Ordering::Release);
    assert!(unsafe { (api.set)(&old.hooks()) }.as_bool());
    let c = Client::new();
    assert!(unsafe { (api.set)(std::ptr::null()) }.as_bool());
    until(&c, |status| status.effective == 2);
    type_text(&c, "gh");
    c.a.invoke.set(false);
    c.a.results.borrow_mut().insert(
        "session",
        windows::Win32::UI::TextServices::TF_E_SYNCHRONOUS,
    );
    settings.save(true, "en.add", Some(2));
    until(&c, |status| status.requested == 0);
    old.pause.store(BEFORE_PUBLISH, Ordering::Release);
    old.published.wait();
    let old_activation = runtime(&c.service).activation;
    assert_eq!(c.deactivate(), S_OK);
    c.a.active.borrow_mut().take();
    c.a.invoke.set(true);
    c.a.results.borrow_mut().remove("session");
    settings.save(true, "en.add", None);
    let new = Events::new();
    new.pause.store(BEFORE_PUBLISH, Ordering::Release);
    assert!(unsafe { (api.set)(&new.hooks()) }.as_bool());
    assert_eq!(c.reactivate(), S_OK);
    assert!(unsafe { (api.set)(std::ptr::null()) }.as_bool());
    new.published.wait();
    old.resume.signal();
    old.finished.wait();
    old.check();
    until(&c, |status| {
        assert_eq!(
            status.revision, 0,
            "prior activation must not adopt its pending revision"
        );
        status.activation > old_activation
    });
    let before = (text_reads(&c), c.a.starts.get(), c.a.writes.get());
    assert!(!down(&c, b'G' as u16));
    assert_eq!((text_reads(&c), c.a.starts.get(), c.a.writes.get()), before);
    new.pause.store(u32::MAX, Ordering::Release);
    new.resume.signal();
    until(&c, |status| status.effective == 2);
    assert_eq!(crate::support::dll::snapshot(&c.service).stopped, 0);
    c.a.selection.set(crate::support::document::Span {
        start: 7,
        length: 0,
    });
    assert!(down(&c, b'G' as u16));
    assert_eq!(c.deactivate(), S_OK);
    new.finished.wait();
    new.check();
}

fn pending_off_survives_invalidation(cause: &str) {
    use crate::worker_support::{BEFORE_PUBLISH, Events, WorkerApi};
    use std::sync::atomic::Ordering;
    use windows::Win32::UI::TextServices::*;
    let settings = Settings::new();
    let _keyboard = Keyboard::new();
    settings.save(true, "en.add", None);
    let api = WorkerApi::load();
    let events = Events::new();
    events.pause.store(u32::MAX, Ordering::Release);
    assert!(unsafe { (api.set)(&events.hooks()) }.as_bool());
    let c = Client::new();
    assert!(unsafe { (api.set)(std::ptr::null()) }.as_bool());
    until(&c, |status| status.effective == 2);
    type_text(&c, "gh");
    if cause == "unknown" {
        c.a.results.borrow_mut().insert("SetText", E_FAIL);
    } else {
        c.a.invoke.set(false);
        c.a.results.borrow_mut().insert("session", TF_E_SYNCHRONOUS);
    }
    settings.save(true, "en.add", Some(2));
    until(&c, |status| status.requested == 0);
    events.pause.store(BEFORE_PUBLISH, Ordering::Release);
    events.published.wait();
    c.a.invoke.set(true);
    c.a.results.borrow_mut().remove("session");
    match cause {
        "focus" => {
            let sink = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
            let manager = c.thread.cast::<ITfDocumentMgr>().unwrap();
            unsafe {
                assert_eq!(
                    (sink.vtable().OnSetFocus)(
                        sink.as_raw(),
                        manager.as_raw(),
                        std::ptr::null_mut()
                    ),
                    S_OK
                );
            }
        }
        "source" => {
            let sink = c
                .service
                .cast::<ITfInputProcessorProfileActivationSink>()
                .unwrap();
            unsafe {
                assert_eq!(
                    (sink.vtable().OnActivated)(
                        sink.as_raw(),
                        TF_PROFILETYPE_INPUTPROCESSOR,
                        0x409,
                        &crate::support::providers::SERVICE_ID,
                        &GUID_TFCAT_TIP_KEYBOARD,
                        &crate::support::providers::PROFILE_EN,
                        HKL::default(),
                        TF_IPSINK_FLAG_ACTIVE
                    ),
                    S_OK
                );
            }
        }
        "unknown" => {
            c.a.results.borrow_mut().remove("SetText");
            let writes = c.a.writes.get();
            assert!(!down(&c, b'G' as u16));
            assert_eq!(
                c.a.writes.get(),
                writes,
                "unknown retirement must not replay the write"
            );
        }
        "termination" => {}
        _ => unreachable!(),
    }
    let stopped = u32::from(cause != "termination");
    assert_eq!(crate::support::dll::snapshot(&c.service).stopped, stopped);
    let requests = c.a.request.get();
    for _ in 0..3 {
        until(&c, |status| status.requested == 0);
    }
    assert_eq!(
        c.a.request.get(),
        requests,
        "maintenance must not retry retirement for the same owned revision"
    );
    let pointer = c.a.active.borrow().as_ref().unwrap().pointer;
    let old =
        unsafe { windows_core::IUnknown::from_raw(crate::support::com_slots::retain(pointer)) };
    c.a.active.borrow_mut().take();
    let sink = c.service.cast::<ITfCompositionSink>().unwrap();
    unsafe {
        assert_eq!(
            (sink.vtable().OnCompositionTerminated)(sink.as_raw(), 123, old.as_raw()),
            S_OK
        );
    }
    c.a.selection.set(crate::support::document::Span {
        start: 7,
        length: 0,
    });
    let before = (text_reads(&c), c.a.starts.get(), c.a.writes.get());
    let eaten = down(&c, b'G' as u16);
    wait(|| runtime(&c.service).effective == 0);
    assert_eq!(crate::support::dll::snapshot(&c.service).stopped, stopped);
    assert!(!down(&c, b'G' as u16));
    let after = (text_reads(&c), c.a.starts.get(), c.a.writes.get());
    assert_eq!(c.deactivate(), S_OK);
    events.pause.store(u32::MAX, Ordering::Release);
    events.resume.signal();
    events.finished.wait();
    events.check();
    assert!(
        !eaten,
        "known pending Off must remain restrictive after exact host termination; before={before:?}, after={after:?}"
    );
    assert_eq!(
        before, after,
        "known pending Off must cause no new text inspection/start/write"
    );
    oracle::document(&c, "gh", 7);
    println!(
        "scenario configuration.pending-off.{cause}.paused-worker.zero-read-adoption {before:?} -> {after:?}"
    );
}
