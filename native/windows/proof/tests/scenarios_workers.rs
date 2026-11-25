const E_PENDING: windows_core::HRESULT = windows_core::HRESULT(0x8000000au32 as i32);
use super::{
    support::{
        client::Client,
        dll::{Event as InputEvent, snapshot},
        oracle,
    },
    worker_support::*,
};
use std::os::windows::ffi::OsStrExt;
use std::{
    ffi::c_void,
    ptr::null,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use windows::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{IO::*, Threading::*},
    UI::{TextServices::*, WindowsAndMessaging::*},
};
use windows_core::{BOOL, HRESULT, Interface, PCWSTR};

#[test]
fn runtime_worker_absent_sources_and_global_hook_owner() {
    let api = WorkerApi::load();
    for inherited in [false, true] {
        let events = Events::new();
        if inherited {
            assert!(unsafe { (api.set)(&events.hooks()) }.as_bool());
        }
        let worker = api.worker();
        assert!(worker.start(
            &[Source::absent(), Source::absent()],
            if inherited { 61 } else { 62 },
            false,
            if inherited {
                Hooks::default()
            } else {
                events.hooks()
            }
        ));
        let packet = events.next(&worker);
        assert_eq!(packet.model, 1);
        for source in packet.sources {
            assert_eq!((source.status, source.error), (0, 2));
            assert!(source.dictionary.is_null());
        }
        if inherited {
            assert!(unsafe { (api.set)(null()) }.as_bool());
        }
        events.stop(&worker);
        println!(
            "scenario runtime.{}.absent-null-both",
            if inherited {
                "global-hooks"
            } else {
                "absent-source"
            }
        );
    }
}

#[test]
fn runtime_worker_real_files_settle_parse_retain_and_absence() {
    let api = WorkerApi::load();
    let parse: unsafe extern "system" fn(*const u16, u32) -> BOOL =
        unsafe { api.module.export(b"KeyrillTestWorkerParsePath\0") };
    for path in [
        r"\\server\share\file",
        r"C:file",
        r"C:\a\..\b",
        r"C:\NUL.add",
        r"C:\a:stream",
        r"C:\a.\b",
        "C:\\a\\",
        r"C:\a//b",
        r"C:\\a",
    ] {
        let text: Vec<_> = path.encode_utf16().collect();
        assert!(!unsafe { parse(text.as_ptr(), text.len() as u32) }.as_bool());
        println!("scenario runtime.path.{path}");
    }
    let files = Files::new();
    files.write("en.add", b"hello\n");
    files.write("ru.add", b"");
    let events = Events::new();
    let worker = api.worker();
    assert!(worker.start(
        &[files.source("en.add", false), files.source("ru.add", false)],
        7,
        false,
        events.hooks()
    ));
    let first = events.next(&worker);
    assert_eq!(first.activation, 7);
    assert_eq!(first.sources[0].status, 1);
    assert!(first.sources[0].dictionary.is_null());
    let ready = events.next(&worker);
    assert_eq!(ready.sources.map(|s| s.status), [2, 2]);
    files.write("en.add", b"\xff\nhello\n");
    drop(events.next(&worker));
    let ready = events.next(&worker);
    assert_eq!(ready.sources[0].status, 2);
    assert!(!ready.sources[0].dictionary.is_null());
    let engine_new: unsafe extern "C" fn(u32, *mut *mut c_void) -> u32 =
        unsafe { api.module.export(b"kr_engine_new\0") };
    let adopt: unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void) -> u32 =
        unsafe { api.module.export(b"kr_adopt_dictionaries\0") };
    let skipped: unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> u32 =
        unsafe { api.module.export(b"kr_dictionary_skipped_lines\0") };
    let free: unsafe extern "C" fn(*mut c_void) = unsafe { api.module.export(b"kr_engine_free\0") };
    let mut engine = std::ptr::null_mut();
    assert_eq!(unsafe { engine_new(3, &mut engine) }, 0);
    assert!(!engine.is_null());
    assert_eq!(
        unsafe { adopt(engine, ready.sources[0].dictionary, null()) },
        0
    );
    let mut count = 0;
    assert_eq!(unsafe { skipped(engine, 1, &mut count) }, 0);
    assert_eq!(count, 1);
    unsafe { free(engine) };
    let last_good = ready.sources[0].dictionary;
    for bytes in [
        b"bad\0data".to_vec(),
        b"/encoding=latin1\nword\n".to_vec(),
        vec![b'a'; 1024 * 1024 + 1],
    ] {
        files.write("en.add", bytes);
        let bad = events.next(&worker);
        assert_eq!(bad.sources[0].status, 3);
        assert_eq!(bad.sources[0].dictionary, last_good);
    }
    std::fs::remove_file(files.path("en.add")).unwrap();
    let deleted = events.next(&worker);
    assert_eq!(deleted.sources[0].status, 3);
    assert_eq!(deleted.sources[0].dictionary, last_good);
    events.stop(&worker);
    let events = Events::new();
    assert!(worker.start(
        &[
            files.source("missing\\default.add", true),
            files.source("explicit.add", false)
        ],
        8,
        false,
        events.hooks()
    ));
    let absent = events.next(&worker);
    assert_eq!(absent.sources.map(|s| s.status), [2, 0]);
    assert!(!absent.sources[0].dictionary.is_null());
    assert!(absent.sources[1].dictionary.is_null());
    events.stop(&worker);
    println!(
        "scenario runtime.file.staged-two-equal.invalid-utf8.nul.encoding.oversize.deleted.default-parent.explicit-absence"
    );
}

#[test]
fn runtime_worker_replacement_and_parent_handles() {
    let api = WorkerApi::load();
    let files = Arc::new(Files::new());
    files.write("en.add", b"before\n");
    files.write("new.add", b"after\n");
    files.write("ru.add", b"");
    let events = Events::new();
    let exchanged = Arc::new(AtomicBool::new(false));
    let (owned, flag) = (Arc::clone(&files), Arc::clone(&exchanged));
    *events.action.lock().unwrap() = Some(Arc::new(move |at, _, _| {
        if at == BEFORE_READ && !flag.swap(true, Ordering::AcqRel) {
            unsafe {
                ReplaceFileW(
                    PCWSTR(wide(owned.path("en.add")).as_ptr()),
                    PCWSTR(wide(owned.path("new.add")).as_ptr()),
                    None,
                    REPLACE_FILE_FLAGS(0),
                    None,
                    None,
                )
                .unwrap();
            }
        }
    }));
    let worker = api.worker();
    assert!(worker.start(
        &[files.source("en.add", false), files.source("ru.add", false)],
        9,
        false,
        events.hooks()
    ));
    assert_eq!(events.next(&worker).sources[0].status, 0);
    assert!(exchanged.load(Ordering::Acquire));
    assert_eq!(events.next(&worker).sources[0].status, 1);
    assert_eq!(events.next(&worker).sources[0].status, 2);
    events.stop(&worker);
    let events = Events::new();
    let attempted = Arc::new(AtomicBool::new(false));
    let (owned, flag) = (Arc::clone(&files), Arc::clone(&attempted));
    let parent_index = wide(files.path("en.add"))
        .iter()
        .filter(|&&u| u == b'\\' as u16)
        .count()
        - 1;
    *events.action.lock().unwrap() = Some(Arc::new(move |at, _, index| {
        if at == PARENT_OPENED && index == parent_index && !flag.swap(true, Ordering::AcqRel) {
            unsafe {
                assert!(
                    MoveFileExW(
                        PCWSTR(wide(&owned.0).as_ptr()),
                        PCWSTR(wide(owned.0.with_extension("renamed")).as_ptr()),
                        MOVE_FILE_FLAGS(0)
                    )
                    .is_err()
                );
            }
        }
    }));
    assert!(worker.start(
        &[files.source("en.add", false), files.source("ru.add", false)],
        10,
        false,
        events.hooks()
    ));
    drop(events.next(&worker));
    assert!(attempted.load(Ordering::Acquire));
    events.stop(&worker);
    println!("scenario runtime.file.replacement-identity.parent-rename-denied");
}

#[test]
fn runtime_worker_cancellation_and_detached_mailboxes() {
    let api = WorkerApi::load();
    let files = Files::new();
    files.write("en.add", b"hello\n");
    let sources = [files.source("en.add", false), files.source("en.add", false)];
    let worker = api.worker();
    for boundary in [BEFORE_PARSE, BEFORE_PUBLISH] {
        let events = Events::new();
        events.pause.store(boundary, Ordering::Release);
        assert!(worker.start(&sources, 52, false, events.hooks()));
        events.published.wait();
        assert!(worker.poll().is_none());
        worker.stop();
        assert!(!events.finished.signaled());
        assert!(worker.poll().is_none());
        events.resume.signal();
        events.finished.wait();
        assert!(worker.poll().is_none());
        events.check();
        println!("scenario runtime.cancel.boundary-{boundary}");
    }
    let old = Events::new();
    old.pause.store(BEFORE_PUBLISH, Ordering::Release);
    assert!(worker.start(&sources, 40, false, old.hooks()));
    old.published.wait();
    let new = Events::new();
    assert!(worker.start(&sources, 41, false, new.hooks()));
    assert_eq!(new.next(&worker).activation, 41);
    old.resume.signal();
    old.finished.wait();
    old.check();
    let ready = new.next(&worker);
    assert_eq!(ready.activation, 41);
    assert!(ready.revision > 1);
    new.stop(&worker);
    println!("scenario runtime.native.old-40-new-41-mailbox");
}

#[test]
fn runtime_service_model_vocabulary_receipts_and_configuration() {
    let files = Files::new();
    files.write("en.add", b"");
    files.write("ru.add", b"");
    let _policy = Environment::set("KEYRILL_WINDOWS_POLICY", "model");
    let _en = Environment::set("KEYRILL_WINDOWS_EN_DICTIONARY", files.path("en.add"));
    let _ru = Environment::set("KEYRILL_WINDOWS_RU_DICTIONARY", files.path("ru.add"));
    for mode in [1, 2] {
        let c = Client::new();
        ready(&c);
        let input = if mode == 1 { "cgbcjr" } else { "цщкдв" };
        for unit in input.encode_utf16().chain(Some(32)) {
            let mut event = InputEvent::text(unit);
            event.mode = mode;
            assert!(c.event(event));
        }
        oracle::document(
            &c,
            if mode == 1 { "список " } else { "world " },
            if mode == 1 { 14 } else { 13 },
        );
        let before = runtime(&c.service).revision;
        files.write("en.add", b"publicword\n");
        let maintain: unsafe extern "system" fn(*mut c_void) -> HRESULT =
            unsafe { c.api.export(b"KeyrillTestMaintain\0") };
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(unsafe { maintain(c.handle) }, S_OK);
        assert_eq!(runtime(&c.service).revision, before);
        let mut reverse = InputEvent::command(5);
        reverse.mode = mode;
        assert!(c.event(reverse));
        oracle::document(
            &c,
            if mode == 1 { "cgbcjr " } else { "цщкдв " },
            if mode == 1 { 14 } else { 13 },
        );
        let mut finish = InputEvent::command(6);
        finish.mode = mode;
        assert!(!c.event(finish));
        maintain_until(&c, |s| s.revision > before + 1 && s.sources[0] == 2);
        println!("scenario runtime.service.mode-{mode}.receipt-across-publication");
    }
    for (en, ru) in [
        (b"cgbcjr\n".as_slice(), b"".as_slice()),
        (b"".as_slice(), "список/!\n".as_bytes()),
    ] {
        files.write("en.add", en);
        files.write("ru.add", ru);
        let c = Client::new();
        ready(&c);
        c.type_text("cgbcjr ");
        oracle::document(&c, "cgbcjr ", 14);
    }
    files.write("en.add", b"");
    files.write("ru.add", b"");
    {
        let c = Client::new();
        ready(&c);
        let before = runtime(&c.service).revision;
        files.write("en.add", b"bad\0data");
        maintain_until(&c, |s| {
            s.revision > before && s.sources[0] == 3 && s.source_errors[0] == 6
        });
        c.type_text("cgbcjr ");
        oracle::document(&c, "список ", 14);
    }
    {
        let _missing = Environment::set("KEYRILL_WINDOWS_EN_DICTIONARY", files.path("absent.add"));
        let c = Client::new();
        maintain_until(&c, |s| s.effective == 2);
        assert_eq!(runtime(&c.service).sources[0], 0);
        c.type_text("cgbcjr ");
        oracle::document(&c, "cgbcjr ", 14);
    }
    {
        let _off = Environment::set("KEYRILL_WINDOWS_POLICY", "off");
        let c = Client::new();
        assert_eq!(runtime(&c.service).effective, 0);
        c.type_text("cgbcjr");
        assert!(c.event(InputEvent::command(4)));
        oracle::document(&c, "список", 13);
        assert_eq!(c.deactivate(), S_OK);
        let _invalid = Environment::set("KEYRILL_WINDOWS_POLICY", "invalid");
        assert_eq!(c.reactivate(), S_OK);
        let state = runtime(&c.service);
        assert_eq!((state.requested, state.configuration_error), (0, 1));
    }
    println!(
        "scenario runtime.service.protected-en.protected-ru.retained-parse.explicit-missing.off-manual.invalid-whole-config"
    );
}
fn maintain_until(c: &Client, mut predicate: impl FnMut(RuntimeSnapshot) -> bool) {
    let maintain: unsafe extern "system" fn(*mut c_void) -> HRESULT =
        unsafe { c.api.export(b"KeyrillTestMaintain\0") };
    wait(|| {
        assert_eq!(unsafe { maintain(c.handle) }, S_OK);
        predicate(runtime(&c.service))
    });
}
fn ready(c: &Client) {
    maintain_until(c, |s| s.effective == 2 && s.sources == [2, 2]);
    assert_eq!(runtime(&c.service).model_ready, 1);
}

#[test]
fn runtime_binary_reader_rejects_invalid_envelopes_atomically() {
    let c = Client::new();
    let reader = c.service.cast::<RuntimeReader>().unwrap();
    for (size, version) in [(47, 1), (49, 1), (48, 0), (48, 2)] {
        let mut output = RuntimeSnapshot {
            activation: 42,
            ..Default::default()
        };
        assert_eq!(
            unsafe { reader.read(size, version, &mut output) },
            E_INVALIDARG
        );
        assert_eq!(output, RuntimeSnapshot::default());
    }
    assert_eq!(
        unsafe { reader.read(48, 1, std::ptr::null_mut()) },
        E_POINTER
    );
    assert_eq!(size_of::<RuntimeSnapshot>(), 48);
    assert_eq!(std::mem::offset_of!(RuntimeSnapshot, revision), 16);
    println!("scenario runtime.codec.binary-size-version-no-partial-output");
}

#[test]
fn runtime_text_codec_preserves_maximum_revision_and_rejects_partial_rows() {
    use keyrill_windows_proof::{diagnostics::format_runtime, proof_inspect::parse_runtime};
    let value = [7, 2, 2, 1, u32::MAX, u32::MAX, 0, 0, 2, 0, 3, 6];
    let text = format_runtime(&value);
    assert_eq!(parse_runtime(&text), Some(value));
    for bad in [
        format!("{text} trailing"),
        format!("{text}; extra=1"),
        "Keyrill runtime v1; activation=-1".into(),
        "Keyrill runtime v2; activation=7".into(),
    ] {
        assert_eq!(parse_runtime(&bad), None);
    }
    println!("scenario runtime.codec.max-u64.trailing.extra.negative.version");
}

struct WindowReentry {
    service: usize,
    manager: usize,
    destroying: bool,
    reactivate: bool,
    called: std::cell::Cell<bool>,
    deactivated: std::cell::Cell<HRESULT>,
    activated: std::cell::Cell<HRESULT>,
    failed: std::cell::Cell<bool>,
}
thread_local! {static WINDOW_REENTRY:std::cell::RefCell<Option<std::rc::Rc<WindowReentry>>>=const {std::cell::RefCell::new(None)};}
unsafe extern "system" fn window_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    let action = WINDOW_REENTRY.with(|slot| slot.borrow().clone());
    if let Some(action) = action {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if code
                != if action.destroying {
                    HCBT_DESTROYWND as i32
                } else {
                    HCBT_CREATEWND as i32
                }
                || action.called.get()
            {
                return;
            }
            let mut name = [0u16; 64];
            let length = unsafe { GetClassNameW(HWND(w.0 as *mut c_void), &mut name) };
            if name[..length as usize]
                != wide("KeyrillSnapshotMaintenance")
                    [..wide("KeyrillSnapshotMaintenance").len() - 1]
            {
                return;
            }
            action.called.set(true);
            let pointer = action.service as *mut c_void;
            let vtable = unsafe { &**pointer.cast::<*const ITfTextInputProcessorEx_Vtbl>() };
            action
                .deactivated
                .set(unsafe { (vtable.base__.Deactivate)(pointer) });
            if action.reactivate {
                action.activated.set(unsafe {
                    (vtable.ActivateEx)(pointer, action.manager as *mut c_void, 42, 0)
                });
            }
        }));
        if result.is_err() {
            action.failed.set(true);
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}
fn with_window_reentry(
    c: &Client,
    destroying: bool,
    reactivate: bool,
    call: impl FnOnce() -> HRESULT,
) -> (HRESULT, std::rc::Rc<WindowReentry>) {
    let action = std::rc::Rc::new(WindowReentry {
        service: c.service.as_raw() as usize,
        manager: c.thread.as_raw() as usize,
        destroying,
        reactivate,
        called: std::cell::Cell::new(false),
        deactivated: std::cell::Cell::new(E_PENDING),
        activated: std::cell::Cell::new(E_PENDING),
        failed: std::cell::Cell::new(false),
    });
    WINDOW_REENTRY.with(|slot| {
        assert!(slot.replace(Some(std::rc::Rc::clone(&action))).is_none());
    });
    let hook = unsafe {
        SetWindowsHookExW(WH_CBT, Some(window_hook), None, GetCurrentThreadId()).unwrap()
    };
    let result = call();
    unsafe {
        UnhookWindowsHookEx(hook).unwrap();
    }
    WINDOW_REENTRY.with(|slot| {
        slot.take();
    });
    assert!(action.called.get());
    assert!(!action.failed.get());
    assert_eq!(action.deactivated.get(), S_OK);
    (result, action)
}
#[test]
fn runtime_hidden_window_creation_destruction_reentry_and_timer_readiness() {
    let files = Files::new();
    files.write("en.add", b"");
    files.write("ru.add", b"");
    let _policy = Environment::set("KEYRILL_WINDOWS_POLICY", "model");
    let _en = Environment::set("KEYRILL_WINDOWS_EN_DICTIONARY", files.path("en.add"));
    let _ru = Environment::set("KEYRILL_WINDOWS_RU_DICTIONARY", files.path("ru.add"));
    let c = Client::new();
    let module = Module::test();
    let counts: unsafe extern "system" fn(*mut i32, *mut i32) -> HRESULT =
        unsafe { module.export(b"KeyrillTestCounts\0") };
    let drained = || {
        let (mut objects, mut workers) = (0, 0);
        assert_eq!(unsafe { counts(&mut objects, &mut workers) }, S_OK);
        workers == 0
    };
    let class_gone = || {
        let mut class = WNDCLASSW::default();
        unsafe {
            GetClassInfoW(
                Some(HINSTANCE(module.0.0)),
                PCWSTR(wide("KeyrillSnapshotMaintenance").as_ptr()),
                &mut class,
            )
            .is_err()
        }
    };
    assert_eq!(c.deactivate(), S_OK);
    wait(drained);
    let (result, _) = with_window_reentry(&c, false, false, || c.reactivate());
    assert!(result.is_err());
    assert_eq!(snapshot(&c.service).active, 0);
    assert!(drained());
    assert!(class_gone());
    let (result, action) = with_window_reentry(&c, false, true, || c.reactivate());
    assert!(result.is_err());
    assert_eq!(action.activated.get(), E_INVALIDARG);
    assert_eq!(snapshot(&c.service).active, 0);
    assert_eq!(c.reactivate(), S_OK);
    wait(|| {
        let state = runtime(&c.service);
        state.model_ready == 1 && state.effective == 2 && state.sources == [2, 2]
    });
    let (result, action) = with_window_reentry(&c, true, true, || c.deactivate());
    assert_eq!(result, S_OK);
    assert_eq!(action.activated.get(), E_INVALIDARG);
    assert_eq!(snapshot(&c.service).active, 0);
    assert_eq!(c.reactivate(), S_OK);
    wait(|| {
        let state = runtime(&c.service);
        state.model_ready == 1 && state.effective == 2 && state.sources == [2, 2]
    });
    assert_eq!(c.deactivate(), S_OK);
    wait(drained);
    assert!(class_gone());
    oracle::document(&c, "", 7);
    oracle::no_effects(&c);
    println!(
        "scenario runtime.window.create-deactivate.create-reactivate.destroy-reactivate.fresh-timer.class-retired"
    );
}

#[test]
fn runtime_nested_message_pumping_defers_snapshot_adoption() {
    let api = WorkerApi::load();
    let files = Files::new();
    files.write("en.add", b"");
    files.write("ru.add", b"");
    let _policy = Environment::set("KEYRILL_WINDOWS_POLICY", "model");
    let _en = Environment::set("KEYRILL_WINDOWS_EN_DICTIONARY", files.path("en.add"));
    let _ru = Environment::set("KEYRILL_WINDOWS_RU_DICTIONARY", files.path("ru.add"));
    let events = Events::new();
    events.pause.store(u32::MAX, Ordering::Release);
    let publications = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&publications);
    *events.action.lock().unwrap() = Some(Arc::new(move |at, _, _| {
        if at == PUBLISHED {
            counter.fetch_add(1, Ordering::AcqRel);
        }
    }));
    assert!(unsafe { (api.set)(&events.hooks()) }.as_bool());
    let c = Client::new();
    assert!(unsafe { (api.set)(null()) }.as_bool());
    ready(&c);
    let before = runtime(&c.service);
    let observed = publications.load(Ordering::Acquire);
    files.write("en.add", b"another\n");
    let maintain: unsafe extern "system" fn(*mut c_void) -> HRESULT =
        unsafe { c.api.export(b"KeyrillTestMaintain\0") };
    let reader = c.service.cast::<RuntimeReader>().unwrap();
    let handle = c.handle as usize;
    let called = std::rc::Rc::new(std::cell::Cell::new(false));
    let did_call = std::rc::Rc::clone(&called);
    c.host.on("GetStatus", 0, move || {
        did_call.set(true);
        wait(|| {
            assert_eq!(unsafe { maintain(handle as *mut c_void) }, S_OK);
            let mut value = RuntimeSnapshot::default();
            assert_eq!(unsafe { reader.read(48, 1, &mut value) }, E_PENDING);
            publications.load(Ordering::Acquire) > observed + 1
        });
    });
    assert!(c.key(b'c' as u16, true));
    assert!(called.get());
    assert_eq!(runtime(&c.service).revision, before.revision);
    assert!(!c.event(InputEvent::command(6)));
    maintain_until(&c, |s| s.revision > before.revision);
    oracle::document(&c, "c", 8);
    assert_eq!(c.deactivate(), S_OK);
    events.finished.wait();
    events.check();
    println!("scenario runtime.service.nested-pump-pending.deferred-adoption");
}

#[test]
fn runtime_native_open_association_event_and_completion_axes() {
    let api = WorkerApi::load();
    let files = Files::new();
    files.write("en.add", b"hello\n");
    let sources = [files.source("en.add", true), files.source("en.add", true)];
    for failure in [Failure::Root, Failure::Verification] {
        for remap in [false, true] {
            let schedule = Native::new(Io::Real, failure, 1, remap);
            let worker = api.worker();
            assert!(worker.start(&sources, 31, false, schedule.hooks()));
            let packet = schedule.events.next(&worker);
            assert_eq!(packet.sources[0].status, 0);
            assert!(packet.sources[0].dictionary.is_null());
            assert_eq!(
                packet.sources[0].error,
                if failure == Failure::Root { 2 } else { 4 }
            );
            schedule.stop(&worker);
            println!("scenario runtime.native.{failure:?}.remap-{remap}");
        }
    }
    {
        let schedule = Native::new(Io::Real, Failure::Component, 1, true);
        let worker = api.worker();
        assert!(worker.start(&sources, 31, false, schedule.hooks()));
        let packet = schedule.events.next(&worker);
        assert_eq!((packet.sources[0].status, packet.sources[0].error), (0, 4));
        assert!(packet.sources[0].dictionary.is_null());
        assert_eq!(schedule.reads.load(Ordering::Acquire), 0);
        schedule.stop(&worker);
    }
    {
        let schedule = Native::new(Io::Real, Failure::Event, 2, false);
        let worker = api.worker();
        assert!(worker.start(&sources, 31, false, schedule.hooks()));
        let packet = schedule.events.next(&worker);
        assert_eq!(packet.sources[0].error, 5);
        assert!(packet.sources[0].dictionary.is_null());
        assert_eq!(schedule.reads.load(Ordering::Acquire), 0);
        schedule.stop(&worker);
    }
    for io in [Io::Immediate, Io::Pending] {
        let schedule = Native::new(io, Failure::None, 1, false);
        let worker = api.worker();
        assert!(worker.start(&sources, 32, false, schedule.hooks()));
        drop(schedule.events.next(&worker));
        let ready = schedule.events.next(&worker);
        assert_eq!(ready.sources[0].status, 2);
        assert!(!ready.sources[0].dictionary.is_null());
        schedule.stop(&worker);
        let completions = schedule.completions.load(Ordering::Acquire);
        assert!(completions >= 8);
        assert_eq!(completions, schedule.reads.load(Ordering::Acquire));
        println!("scenario runtime.native.completion-{io:?}");
    }
    println!("scenario runtime.native.component-remap.io-event-failure");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Truncate,
    Grow,
    RestoreTimes,
}
#[test]
fn runtime_real_file_changes_settle_exact_bytes_and_metadata() {
    let api = WorkerApi::load();
    for change in [Change::Truncate, Change::Grow, Change::RestoreTimes] {
        let files = Arc::new(Files::new());
        files.write("en.add", b"hello\n");
        files.write("ru.add", b"");
        let events = Events::new();
        let armed = Arc::new(AtomicBool::new(false));
        let changed = Arc::new(AtomicBool::new(false));
        let (owned, arm, done) = (Arc::clone(&files), Arc::clone(&armed), Arc::clone(&changed));
        *events.action.lock().unwrap() = Some(Arc::new(move |at, file, _| {
            if at != BEFORE_READ || !arm.swap(false, Ordering::AcqRel) {
                return;
            }
            let mut original = FILE_BASIC_INFO::default();
            unsafe {
                GetFileInformationByHandleEx(
                    file,
                    FileBasicInfo,
                    (&mut original as *mut FILE_BASIC_INFO).cast(),
                    size_of::<FILE_BASIC_INFO>() as u32,
                )
                .unwrap();
            }
            owned.write(
                "en.add",
                match change {
                    Change::Truncate => b"hi\n".as_slice(),
                    Change::Grow => b"longerword\n".as_slice(),
                    Change::RestoreTimes => b"other\n".as_slice(),
                },
            );
            if change == Change::RestoreTimes {
                let attributes = unsafe {
                    CreateFileW(
                        PCWSTR(wide(owned.path("en.add")).as_ptr()),
                        FILE_WRITE_ATTRIBUTES.0,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        None,
                        OPEN_EXISTING,
                        FILE_FLAG_OPEN_REPARSE_POINT,
                        None,
                    )
                    .unwrap()
                };
                unsafe {
                    SetFileInformationByHandle(
                        attributes,
                        FileBasicInfo,
                        (&original as *const FILE_BASIC_INFO).cast(),
                        size_of::<FILE_BASIC_INFO>() as u32,
                    )
                    .unwrap();
                    CloseHandle(attributes).unwrap();
                }
                let mut restored = FILE_BASIC_INFO::default();
                unsafe {
                    GetFileInformationByHandleEx(
                        file,
                        FileBasicInfo,
                        (&mut restored as *mut FILE_BASIC_INFO).cast(),
                        size_of::<FILE_BASIC_INFO>() as u32,
                    )
                    .unwrap();
                }
                assert_eq!(
                    (
                        restored.CreationTime,
                        restored.LastWriteTime,
                        restored.ChangeTime
                    ),
                    (
                        original.CreationTime,
                        original.LastWriteTime,
                        original.ChangeTime
                    )
                );
            }
            done.store(true, Ordering::Release);
        }));
        let worker = api.worker();
        assert!(worker.start(
            &[files.source("en.add", false), files.source("ru.add", false)],
            51,
            false,
            events.hooks()
        ));
        drop(events.next(&worker));
        let before = events.next(&worker);
        assert_eq!(before.sources[0].status, 2);
        let last_good = before.sources[0].dictionary;
        armed.store(true, Ordering::Release);
        let candidate = events.next(&worker);
        assert!(changed.load(Ordering::Acquire));
        assert_eq!(candidate.sources[0].status, 3);
        assert_eq!(candidate.sources[0].dictionary, last_good);
        assert_eq!(
            candidate.sources[0].error,
            if change == Change::RestoreTimes { 0 } else { 4 }
        );
        if change != Change::RestoreTimes {
            let first = events.next(&worker);
            assert_eq!((first.sources[0].status, first.sources[0].error), (3, 0));
            assert_eq!(first.sources[0].dictionary, last_good);
        }
        let stable = events.next(&worker);
        assert_eq!(stable.sources[0].status, 2);
        assert!(!stable.sources[0].dictionary.is_null());
        assert_ne!(stable.sources[0].dictionary, last_good);
        events.stop(&worker);
        println!("scenario runtime.stability.{change:?}");
    }
}

#[test]
fn runtime_parent_reparse_race_and_preexisting_junction_read_no_payload() {
    let api = WorkerApi::load();
    let files = Arc::new(Files::new());
    let target = Files::new();
    target.write("en.add", b"sentinel\n");
    std::fs::create_dir(files.path("parent")).unwrap();
    let directory = wide(files.path("parent"));
    let attributes = unsafe {
        CreateFileW(
            PCWSTR(directory.as_ptr()),
            FILE_WRITE_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
        .unwrap()
    };
    let substitute: Vec<u16> = format!("\\??\\{}", target.0.display())
        .encode_utf16()
        .collect();
    let printable: Vec<u16> = target.0.as_os_str().encode_wide().collect();
    let substitute_bytes = (substitute.len() * 2) as u16;
    let printable_bytes = (printable.len() * 2) as u16;
    let length = 8 + substitute_bytes + printable_bytes + 4;
    let mut packet = Vec::new();
    packet.extend_from_slice(&0xa0000003u32.to_le_bytes());
    for word in [
        length,
        0,
        0,
        substitute_bytes,
        substitute_bytes + 2,
        printable_bytes,
    ] {
        packet.extend_from_slice(&word.to_le_bytes());
    }
    for unit in substitute
        .into_iter()
        .chain(Some(0))
        .chain(printable)
        .chain(Some(0))
    {
        packet.extend_from_slice(&unit.to_le_bytes());
    }
    assert_eq!(packet.len(), 8 + length as usize);
    let events = Events::new();
    let mutated = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let (flag, read_count) = (Arc::clone(&mutated), Arc::clone(&reads));
    let handle = attributes.0 as usize;
    let parent_index = wide(files.path("parent\\en.add"))
        .iter()
        .filter(|&&u| u == 92)
        .count()
        - 1;
    *events.action.lock().unwrap() = Some(Arc::new(move |at, _, index| {
        if at == BEFORE_READ {
            read_count.fetch_add(1, Ordering::AcqRel);
        }
        if at == PARENT_OPENED && index == parent_index && !flag.swap(true, Ordering::AcqRel) {
            let mut returned = 0;
            unsafe {
                DeviceIoControl(
                    HANDLE(handle as *mut c_void),
                    0x900a4,
                    Some(packet.as_ptr().cast()),
                    packet.len() as u32,
                    None,
                    0,
                    Some(&mut returned),
                    None,
                )
                .unwrap();
            }
        }
    }));
    let worker = api.worker();
    assert!(worker.start(
        &[
            files.source("parent\\en.add", true),
            files.source("absent.add", false)
        ],
        11,
        false,
        events.hooks()
    ));
    for observation in 0..2 {
        let ready = events.next(&worker);
        assert!(mutated.load(Ordering::Acquire));
        assert_eq!(reads.load(Ordering::Acquire), 0);
        assert_eq!((ready.sources[0].status, ready.sources[0].error), (0, 2));
        assert!(ready.sources[0].dictionary.is_null());
        println!("scenario runtime.reparse.observation-{observation}");
    }
    events.stop(&worker);
    unsafe {
        CloseHandle(attributes).unwrap();
        RemoveDirectoryW(PCWSTR(directory.as_ptr())).unwrap();
    }
}

#[test]
fn runtime_actual_dll_worker_startup_drain_and_unload_axes() {
    let executable = std::env::var_os("KEYRILL_WORKER_SMOKE")
        .expect("runner supplies real worker child executable");
    assert!(std::path::Path::new(&executable).is_absolute());
    for case in [
        "boundary-0",
        "boundary-2",
        "boundary-7",
        "boundary-8",
        "startup-event",
        "startup-pin",
        "startup-submit",
        "startup-long",
        "drain",
        "production",
    ] {
        let output = std::process::Command::new(&executable)
            .env("KEYRILL_WORKER_CASE", case)
            .output()
            .expect("launch isolated DLL worker schedule");
        assert!(
            output.status.success(),
            "worker {case} failed\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(&format!("scenario runtime.dll.{case}")));
        print!("{text}");
    }
}
