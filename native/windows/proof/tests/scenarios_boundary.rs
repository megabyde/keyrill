use crate::support::{
    client, com_slots,
    dll::*,
    document::Document,
    providers::{Kind, Provider},
    schedule::HostCapsule,
};
use std::{ffi::c_void, rc::Rc};
use windows::Win32::{Foundation::*, System::Com::IAgileObject, UI::TextServices::*};
use windows_core::{BSTR, GUID, HRESULT, IUnknown, Interface};

#[test]
fn boundary_failed_constructor_query_releases_context_and_service_owners() {
    let api = Api::load();
    let counts: unsafe extern "system" fn(*mut i32, *mut i32) -> HRESULT =
        unsafe { api.export(b"KeyrillTestCounts\0") };
    let (mut before, mut workers) = (0, 0);
    unsafe {
        assert_eq!(counts(&mut before, &mut workers), S_OK);
    }
    {
        let c = client::Client::new();
        let references = unsafe { com_slots::owner(c.context_a.as_raw()).references.get() };
        let document = Rc::clone(&c.a);
        c.host.on("context.QI", 0, move || {
            document.results.borrow_mut().insert("QI", E_FAIL);
        });
        assert!(!c.key(b'g' as u16, false));
        assert_eq!(
            unsafe { com_slots::owner(c.context_a.as_raw()).references.get() },
            references
        );
        crate::support::oracle::no_effects(&c);
    }
    let mut after = 0;
    unsafe {
        assert_eq!(counts(&mut after, &mut workers), S_OK);
    }
    assert_eq!(after, before);
}

#[test]
fn boundary_teardown_reentry_and_hresult_failure_detach_each_sink_once() {
    for (index, boundary) in [
        (2, "UnadviseKeyEventSink"),
        (0, "UnadviseThreadSink"),
        (1, "UnadviseProfileSink"),
    ] {
        for failure in [false, true] {
            let c = client::Client::new();
            c.manager.unsink_results[index].set(if failure { E_FAIL } else { S_OK });
            let service = c.service.clone();
            c.host.on(boundary, 0, move || unsafe {
                assert_eq!((service.vtable().base__.Deactivate)(service.as_raw()), S_OK);
            });
            assert_eq!(c.deactivate(), if failure { E_FAIL } else { S_OK });
            assert_eq!(c.manager.unadvises.each_ref().map(|n| n.get()), [1; 3]);
            assert!(!c.key(b'g' as u16, false));
            assert_eq!(c.deactivate(), S_OK);
            assert_eq!(c.manager.unadvises.each_ref().map(|n| n.get()), [1; 3]);
            crate::support::oracle::no_effects(&c);
        }
    }
}
struct Instance {
    api: Api,
    host: Rc<HostCapsule>,
    handle: *mut c_void,
    service: Option<ITfTextInputProcessorEx>,
}
impl Instance {
    fn new() -> Self {
        let api = Api::load();
        let host = Rc::new(HostCapsule::default());
        let deps = Dependencies {
            size: size_of::<Dependencies>() as u32,
            version: VERSION,
            context: Rc::as_ptr(&host).cast_mut().cast(),
            retain: Some(client::retain),
            release: Some(client::release),
            profiles: std::ptr::null_mut(),
            layouts: None,
            capture: None,
        };
        let mut handle = std::ptr::null_mut();
        let mut service = std::ptr::null_mut();
        unsafe {
            assert_eq!(
                (api.create)(VERSION, &deps, &mut handle, &mut service),
                S_OK
            );
            Self {
                api,
                host,
                handle,
                service: Some(ITfTextInputProcessorEx::from_raw(service)),
            }
        }
    }
    fn document(&self) -> (Rc<Document>, ITfContext) {
        let d = Document::new(Rc::clone(&self.host), 101);
        let context = unsafe {
            ITfContext::from_raw(Provider::create(
                Rc::clone(&self.host),
                Kind::Document(Rc::clone(&d)),
                ITfContext::IID,
            ))
        };
        d.context.set(context.as_raw());
        (d, context)
    }
    fn probe(&self, context: &ITfContext, operation: u32) -> Probe {
        let mut value = Probe::default();
        unsafe {
            assert_eq!(
                (self.api.probe)(
                    self.handle,
                    context.as_raw(),
                    operation,
                    size_of::<Probe>() as u32,
                    &mut value
                ),
                S_OK
            );
        }
        assert_eq!(value.size, size_of::<Probe>() as u32);
        assert_eq!(value.version, VERSION);
        assert_eq!(value.reserved, 0);
        self.host.check();
        value
    }
    fn detach(&mut self) {
        self.service.take();
        if !self.handle.is_null() {
            unsafe {
                assert_eq!((self.api.release)(self.handle), S_OK);
            }
            self.handle = std::ptr::null_mut();
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.detach();
        if !std::thread::panicking() {
            self.host.drained();
        }
    }
}
#[test]
fn boundary_query_exact_hresult_and_reference_balance() {
    let i = Instance::new();
    let (d, c) = i.document();
    let adds = i.host.adds.get();
    let releases = i.host.releases.get();
    let result = i.probe(&c, 0);
    assert_eq!(result.outer, S_OK);
    assert_eq!(result.identity, c.as_raw() as usize);
    assert_eq!(i.host.adds.get(), adds + 1);
    assert_eq!(i.host.releases.get(), releases + 1);
    d.nulls.borrow_mut().push("QI");
    assert_eq!(i.probe(&c, 0).outer, E_UNEXPECTED);
    for status in [S_FALSE, HRESULT(2), E_FAIL] {
        d.results.borrow_mut().insert("QI", status);
        assert_eq!(i.probe(&c, 0).outer, status);
    }
    d.nulls.borrow_mut().clear();
    d.results.borrow_mut().clear();
}
#[test]
fn boundary_requests_complete_once_and_expire() {
    let i = Instance::new();
    let (d, c) = i.document();
    d.retain_callback.set(true);
    d.repeat.set(true);
    let result = i.probe(&c, 1);
    assert_eq!(
        (
            result.outer,
            result.callback,
            result.entered,
            result.completed,
            result.effect
        ),
        (S_OK, S_OK, 1, 1, 2)
    );
    let raw = d.retained.borrow().as_ref().unwrap().as_raw();
    unsafe {
        let session = ITfEditSession::from_raw(com_slots::retain(raw));
        assert_eq!(
            (session.vtable().DoEditSession)(session.as_raw(), 123),
            E_FAIL
        );
    }
    d.invoke.set(false);
    let result = i.probe(&c, 1);
    assert_eq!((result.entered, result.completed, result.effect), (0, 0, 0));
    let raw = d.retained.borrow().as_ref().unwrap().as_raw();
    unsafe {
        let session = ITfEditSession::from_raw(com_slots::retain(raw));
        assert_eq!(
            (session.vtable().DoEditSession)(session.as_raw(), 123),
            E_FAIL
        );
    }
    for status in [S_FALSE, HRESULT(2), E_FAIL] {
        d.results.borrow_mut().insert("RequestEditSession", status);
        assert_eq!(i.probe(&c, 1).outer, status);
    }
    d.drain_retained();
}
#[test]
fn boundary_query_before_failure_has_no_authority() {
    let i = Instance::new();
    let (d, c) = i.document();
    d.results.borrow_mut().insert("QI", E_ACCESSDENIED);
    let result = i.probe(&c, 0);
    assert_eq!(result.outer, E_ACCESSDENIED);
    assert_eq!(result.identity, 0);
    assert_eq!(d.reads.get(), 0);
    assert_eq!(d.writes.get(), 0);
}
#[test]
fn boundary_query_after_success_allows_lifecycle_reentry() {
    let i = Instance::new();
    let (_, c) = i.document();
    let service = i.service.as_ref().unwrap().as_raw() as usize;
    i.host.on("QI.after", 0, move || unsafe {
        let raw = service as *mut c_void;
        let table = *(raw.cast::<*const ITfTextInputProcessorEx_Vtbl>());
        assert_eq!(((*table).base__.Deactivate)(raw), S_OK);
    });
    assert_eq!(i.probe(&c, 0).outer, S_OK);
}
#[test]
fn boundary_request_before_hresult_preserves_unattempted() {
    let i = Instance::new();
    let (d, c) = i.document();
    d.invoke.set(false);
    d.results
        .borrow_mut()
        .insert("RequestEditSession", E_ACCESSDENIED);
    let result = i.probe(&c, 1);
    assert_eq!(result.outer, E_ACCESSDENIED);
    assert_eq!((result.entered, result.completed, result.effect), (0, 0, 0));
    d.drain_retained();
}
#[test]
fn boundary_request_after_hresult_preserves_completed_effect() {
    let i = Instance::new();
    let (_, c) = i.document();
    let d = unsafe { com_slots::owner(c.as_raw()).document() };
    d.results.borrow_mut().insert("RequestEditSession", E_FAIL);
    let result = i.probe(&c, 1);
    assert_eq!(result.outer, E_FAIL);
    assert_eq!((result.entered, result.completed, result.effect), (1, 1, 2));
}
fn reference_reentry(boundary: &'static str, operation: u32) {
    let i = Instance::new();
    let (_, c) = i.document();
    let service = i.service.as_ref().unwrap().as_raw() as usize;
    i.host.on(boundary, 0, move || unsafe {
        let raw = service as *mut c_void;
        let table = *(raw.cast::<*const ITfTextInputProcessorEx_Vtbl>());
        assert_eq!(((*table).base__.Deactivate)(raw), S_OK);
    });
    let before = unsafe { com_slots::owner(c.as_raw()).references.get() };
    let result = i.probe(&c, operation);
    assert_eq!(result.outer, S_OK);
    assert_eq!(
        unsafe { com_slots::owner(c.as_raw()).references.get() },
        before
    );
}
#[test]
fn boundary_add_before_reentry_is_balanced() {
    reference_reentry("AddRef.before", 1);
}
#[test]
fn boundary_add_after_reentry_is_balanced() {
    reference_reentry("AddRef.after", 1);
}
#[test]
fn boundary_release_before_reentry_is_balanced() {
    reference_reentry("Release.before", 0);
}
#[test]
fn boundary_release_after_reentry_is_balanced() {
    reference_reentry("Release.after", 0);
}
#[test]
fn boundary_final_release_callback_keeps_host_context_alive() {
    let i = Instance::new();
    let (_, c) = i.document();
    let service = i.service.as_ref().unwrap().as_raw() as usize;
    let finals = i.host.final_releases.get();
    i.host.on("Release.final", 0, move || unsafe {
        let raw = service as *mut c_void;
        let table = *(raw.cast::<*const ITfTextInputProcessorEx_Vtbl>());
        assert_eq!(((*table).base__.Deactivate)(raw), S_OK);
    });
    drop(c);
    assert_eq!(i.host.final_releases.get(), finals + 1);
}
#[test]
fn boundary_dependency_capsule_outlives_external_service_owners() {
    let mut i = Instance::new();
    let host = Rc::clone(&i.host);
    let (d, c) = i.document();
    d.retain_callback.set(true);
    assert_eq!(host.dependency_retains.get(), 1);
    let raw_service = i.service.as_ref().unwrap().as_raw();
    let identity = i
        .service
        .as_ref()
        .unwrap()
        .cast::<IUnknown>()
        .unwrap()
        .as_raw() as usize;
    assert_eq!(i.probe(&c, 1).entered, 1);
    i.detach();
    assert_eq!(host.dependency_releases.get(), 0);
    unsafe {
        let service = ITfTextInputProcessorEx::from_raw(com_slots::retain(raw_service));
        assert_eq!(
            service.cast::<IUnknown>().unwrap().as_raw() as usize,
            identity
        );
        let raw = d.retained.borrow().as_ref().unwrap().as_raw();
        let session = ITfEditSession::from_raw(com_slots::retain(raw));
        assert_eq!(
            (session.vtable().DoEditSession)(session.as_raw(), 123),
            E_FAIL
        );
    }
    d.drain_retained();
    assert_eq!(host.dependency_releases.get(), 1);
}
#[test]
fn boundary_rust_panics_preserve_effect_and_poison() {
    for at in [1, 2, 3] {
        let i = Instance::new();
        let (_, c) = i.document();
        unsafe {
            assert_eq!((i.api.panic)(i.handle, at), S_OK);
        }
        if at != 3 {
            let result = i.probe(&c, 1);
            assert_eq!(result.callback, E_FAIL);
            assert_eq!(result.effect, at - 1);
            let mut stopped = 0;
            unsafe {
                assert_eq!((i.api.stopped)(i.handle, &mut stopped), S_OK);
                let service = i.service.as_ref().unwrap();
                assert_eq!((service.vtable().base__.Deactivate)(service.as_raw()), S_OK);
                assert_eq!(
                    (service.vtable().base__.Activate)(service.as_raw(), std::ptr::null_mut(), 0),
                    E_FAIL
                );
            }
            assert_eq!(stopped, 1);
        }
    }
}
#[test]
fn boundary_service_identity_and_output_contracts() {
    let i = Instance::new();
    let service = i.service.as_ref().unwrap();
    let keys = service.cast::<ITfKeyEventSink>().unwrap();
    let provider = service.cast::<ITfFunctionProvider>().unwrap();
    let a = service.cast::<IUnknown>().unwrap();
    assert_eq!(a.as_raw(), keys.cast::<IUnknown>().unwrap().as_raw());
    assert_eq!(a.as_raw(), provider.cast::<IUnknown>().unwrap().as_raw());
    unsafe {
        let mut agile = std::ptr::dangling_mut();
        assert_eq!(
            (keys.vtable().base__.QueryInterface)(keys.as_raw(), &IAgileObject::IID, &mut agile),
            E_NOINTERFACE
        );
        assert!(agile.is_null());
        assert_eq!(
            (provider.vtable().GetType)(provider.as_raw(), std::ptr::null_mut()),
            E_POINTER
        );
        assert_eq!(
            (provider.vtable().GetDescription)(provider.as_raw(), std::ptr::null_mut()),
            E_POINTER
        );
        let mut kind = GUID::zeroed();
        assert_eq!(
            (provider.vtable().GetType)(provider.as_raw(), &mut kind),
            S_OK
        );
        assert_eq!(kind.data1, 0x1a746212);
        let mut description = std::ptr::null_mut();
        assert_eq!(
            (provider.vtable().GetDescription)(provider.as_raw(), &mut description),
            S_OK
        );
        let description = BSTR::from_raw(description.cast());
        assert_eq!(description.to_string(), "Keyrill proof service diagnostics");
        let mut output = std::ptr::dangling_mut();
        assert_eq!(
            (provider.vtable().GetFunction)(
                provider.as_raw(),
                &GUID::zeroed(),
                &IUnknown::IID,
                &mut output
            ),
            E_NOINTERFACE
        );
        assert!(output.is_null());
        assert_eq!(
            (provider.vtable().GetFunction)(
                provider.as_raw(),
                &GUID::zeroed(),
                &IUnknown::IID,
                std::ptr::null_mut()
            ),
            E_POINTER
        );
    }
}
#[test]
fn boundary_wrong_thread_outputs_and_handle_ownership() {
    let i = Instance::new();
    let keys = i
        .service
        .as_ref()
        .unwrap()
        .cast::<ITfKeyEventSink>()
        .unwrap();
    let keys_address = keys.as_raw() as usize;
    let handle = i.handle as usize;
    let release = i.api.release;
    let stopped = i.api.stopped;
    std::thread::spawn(move || unsafe {
        let raw = keys_address as *mut c_void;
        let table = &**raw.cast::<*const ITfKeyEventSink_Vtbl>();
        for (method, expected) in [
            (table.OnTestKeyDown, RPC_E_WRONG_THREAD),
            (table.OnKeyDown, RPC_E_WRONG_THREAD),
            (table.OnTestKeyUp, S_OK),
            (table.OnKeyUp, S_OK),
        ] {
            assert_eq!(
                method(
                    raw,
                    std::ptr::null_mut(),
                    WPARAM(0),
                    LPARAM(0),
                    std::ptr::null_mut()
                ),
                E_POINTER
            );
            let mut eaten = true.into();
            assert_eq!(
                method(raw, std::ptr::null_mut(), WPARAM(0), LPARAM(0), &mut eaten),
                expected
            );
            assert!(!eaten.as_bool());
        }
        let mut output = 1;
        assert_eq!(release(handle as *mut c_void), RPC_E_WRONG_THREAD);
        assert_eq!(
            stopped(handle as *mut c_void, &mut output),
            RPC_E_WRONG_THREAD
        );
        assert_eq!(output, 0);
    })
    .join()
    .unwrap();
}

#[test]
fn boundary_invalid_dependency_table_clears_outputs_before_retention() {
    let api = Api::load();
    let host = Rc::new(HostCapsule::default());
    let mut deps = Dependencies {
        size: 8,
        version: VERSION,
        context: Rc::as_ptr(&host).cast_mut().cast(),
        retain: Some(client::retain),
        release: Some(client::release),
        profiles: std::ptr::null_mut(),
        layouts: None,
        capture: None,
    };
    for variation in 0..4 {
        deps.size = if variation == 0 {
            8
        } else {
            size_of::<Dependencies>() as u32
        };
        deps.version = if variation == 1 { VERSION - 1 } else { VERSION };
        deps.release = if variation == 2 {
            None
        } else {
            Some(client::release)
        };
        let mut handle = std::ptr::dangling_mut();
        let mut service = std::ptr::dangling_mut();
        unsafe {
            assert_eq!(
                (api.create)(
                    if variation == 3 { VERSION - 1 } else { VERSION },
                    &deps,
                    &mut handle,
                    &mut service
                ),
                E_INVALIDARG
            );
        }
        assert!(handle.is_null() && service.is_null());
        assert_eq!(host.dependency_retains.get(), 0);
    }
}
#[test]
fn boundary_inactive_events_do_not_inspect_host() {
    let i = Instance::new();
    let (d, c) = i.document();
    let before = i.host.trace.borrow().len();
    let mut eaten = 1;
    unsafe {
        assert_eq!(
            (i.api.event)(i.handle, c.as_raw(), &Event::text(b'g' as u16), &mut eaten),
            S_OK
        );
        let maintain: unsafe extern "system" fn(*mut c_void) -> HRESULT =
            i.api.export(b"KeyrillTestMaintain\0");
        assert_eq!(maintain(i.handle), S_OK);
    }
    assert_eq!(eaten, 0);
    assert_eq!(i.host.trace.borrow().len(), before);
    assert_eq!(d.request.get(), 0);
    assert_eq!(d.reads.get(), 0);
}
#[test]
fn boundary_pure_native_keyboard_and_binding_exports() {
    use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
    let api = Api::load();
    unsafe {
        let translate: unsafe extern "system" fn(
            WPARAM,
            LPARAM,
            HKL,
            *const u8,
            *mut Event,
            *mut i32,
        ) -> HRESULT = api.export(b"KeyrillTestTranslate\0");
        let exact: unsafe extern "system" fn(*const u8, *mut i32) -> HRESULT =
            api.export(b"KeyrillTestExactCommandModifiers\0");
        let binding: unsafe extern "system" fn(u32, *mut GUID, *mut TF_PRESERVEDKEY) -> HRESULT =
            api.export(b"KeyrillTestBinding\0");
        let mut state = [0u8; 256];
        let mut event = Event::default();
        let mut modifier = 1;
        let en = HKL(0x04090409usize as *mut _);
        assert_eq!(
            translate(
                WPARAM(b'G' as usize),
                LPARAM(0x220001),
                en,
                state.as_ptr(),
                &mut event,
                &mut modifier
            ),
            S_OK
        );
        assert_eq!(
            (
                event.role,
                event.mode,
                event.length,
                event.text[0],
                modifier
            ),
            (1, 1, 1, b'g' as u16, 0)
        );
        assert_eq!(
            translate(
                WPARAM(b'G' as usize),
                LPARAM(1),
                HKL::default(),
                state.as_ptr(),
                &mut event,
                &mut modifier
            ),
            S_OK
        );
        assert_eq!((event.role, event.mode), (6, 0));
        assert_eq!(
            translate(
                WPARAM(0xa4),
                LPARAM(1),
                en,
                state.as_ptr(),
                &mut event,
                &mut modifier
            ),
            S_OK
        );
        assert_eq!(modifier, 1);
        state[0xa2] = 0x80;
        state[0xa4] = 0x80;
        let mut command = 0;
        assert_eq!(exact(state.as_ptr(), &mut command), S_OK);
        assert_eq!(command, 1);
        assert_eq!(
            translate(
                WPARAM(b'K' as usize),
                LPARAM(1),
                en,
                state.as_ptr(),
                &mut event,
                &mut modifier
            ),
            S_OK
        );
        assert_eq!(event.role, 4);
        let mut id = GUID::zeroed();
        let mut key = TF_PRESERVEDKEY::default();
        assert_eq!(binding(0, &mut id, &mut key), S_OK);
        assert_eq!(id.data1, 0xd7abce42);
        assert_eq!(key.uVKey, b'K' as u32);
        assert_eq!(key.uModifiers, TF_MOD_LCONTROL | TF_MOD_LALT);
    }
}
#[test]
fn boundary_known_notepad_versions_are_exact() {
    let api = Api::load();
    let known: unsafe extern "system" fn(*const HostIdentity, *mut i32) -> HRESULT =
        unsafe { api.export(b"KeyrillTestKnownNotepad\0") };
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        let host = HostIdentity {
            kind: 2,
            version,
            ..Default::default()
        };
        let mut output = -1;
        assert_eq!(unsafe { known(&host, &mut output) }, S_OK);
        assert_eq!(output, 1, "exact approved version {version:?}");
    }
}

#[test]
fn boundary_unknown_notepad_versions_and_kinds_are_rejected() {
    let api = Api::load();
    let known: unsafe extern "system" fn(*const HostIdentity, *mut i32) -> HRESULT =
        unsafe { api.export(b"KeyrillTestKnownNotepad\0") };
    let mut rejected = vec![
        [0; 4],
        [11, 2609, 10, 0],
        [11, 2609, 12, 0],
        [11, 2609, 11, 1],
    ];
    for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
        for component in 0..4 {
            let mut neighbor = version;
            neighbor[component] += 1;
            rejected.push(neighbor);
            if version[component] > 0 {
                neighbor[component] = version[component] - 1;
                rejected.push(neighbor);
            }
        }
        for kind in [0, 1, 3, u32::MAX] {
            let host = HostIdentity {
                kind,
                version,
                ..Default::default()
            };
            let mut output = -1;
            assert_eq!(unsafe { known(&host, &mut output) }, S_OK);
            assert_eq!(output, 0, "kind {kind}, version {version:?}");
        }
    }
    for version in rejected {
        let host = HostIdentity {
            kind: 2,
            version,
            ..Default::default()
        };
        let mut output = -1;
        assert_eq!(unsafe { known(&host, &mut output) }, S_OK);
        assert_eq!(output, 0, "unapproved version {version:?}");
    }
    let mut output = -1;
    assert_eq!(unsafe { known(std::ptr::null(), &mut output) }, E_POINTER);
    assert_eq!(output, 0);
    assert_eq!(
        unsafe { known(&HostIdentity::default(), std::ptr::null_mut()) },
        E_POINTER
    );
    assert_eq!(
        unsafe { known(std::ptr::null(), std::ptr::null_mut()) },
        E_POINTER
    );
}

#[test]
fn boundary_pure_native_field_match_and_secure_refusal() {
    let api = Api::load();
    unsafe {
        let matches: unsafe extern "system" fn(
            *const FieldFacts,
            *mut FieldIdentity,
            *mut i32,
        ) -> HRESULT = api.export(b"KeyrillTestMatch\0");
        let mut facts = FieldFacts {
            host: HostIdentity {
                process: 17,
                thread: 23,
                started: 1,
                kind: 2,
                version: [11, 2607, 14, 0],
            },
            process: 17,
            thread: 23,
            view: HWND(std::ptr::dangling_mut()),
            focus: HWND(std::ptr::dangling_mut()),
            root: HWND(3usize as *mut _),
            foreground: HWND(3usize as *mut _),
            ..Default::default()
        };
        let node = |handle, class: &str| {
            let mut value = FieldWindow {
                handle,
                process: 17,
                thread: 23,
                enabled: 1,
                complete: 1,
                ..Default::default()
            };
            for (to, from) in value.class_name.iter_mut().zip(class.encode_utf16()) {
                *to = from;
            }
            value
        };
        facts.field = node(facts.view, "RichEditD2DPT");
        facts.field.style = 0x40000004;
        facts.parent = node(HWND(2usize as *mut _), "NotepadTextBox");
        facts.grandparent = node(facts.root, "Notepad");
        let sentinel = FieldIdentity {
            kind: u32::MAX,
            field: HWND(5usize as *mut _),
            parent: HWND(6usize as *mut _),
            root: HWND(7usize as *mut _),
            process: u32::MAX,
            thread: u32::MAX,
            root_thread: u32::MAX,
        };
        let expected = FieldIdentity {
            kind: 1,
            field: facts.view,
            parent: facts.parent.handle,
            root: facts.root,
            process: 17,
            thread: 23,
            root_thread: 23,
        };
        for version in [[11, 2607, 14, 0], [11, 2609, 11, 0]] {
            facts.host.version = version;
            let mut identity = sentinel;
            let mut matched = -1;
            assert_eq!(matches(&facts, &mut identity, &mut matched), S_OK);
            assert_eq!(matched, 1, "version {version:?}");
            assert_eq!(identity, expected);
            for case in 0..7 {
                let mut invalid = facts;
                match case {
                    0 => invalid.field.style |= 0x20,
                    1 => invalid.field.style |= 0x800,
                    2 => invalid.focus = HWND::default(),
                    3 => invalid.parent.class_name[0] = b'!' as u16,
                    4 => invalid.grandparent.enabled = 0,
                    5 => invalid.host.version[2] += 1,
                    6 => invalid.host.kind = 1,
                    _ => unreachable!(),
                }
                identity = sentinel;
                matched = -1;
                assert_eq!(matches(&invalid, &mut identity, &mut matched), S_OK);
                assert_eq!(matched, 0, "version {version:?}, case {case}");
                assert_eq!(identity, FieldIdentity::default());
            }
        }
        let mut identity = sentinel;
        let mut matched = -1;
        assert_eq!(
            matches(std::ptr::null(), &mut identity, &mut matched),
            E_POINTER
        );
        assert_eq!(identity, FieldIdentity::default());
        assert_eq!(matched, 0);
        identity = sentinel;
        matched = -1;
        assert_eq!(
            matches(&facts, std::ptr::null_mut(), &mut matched),
            E_POINTER
        );
        assert_eq!(matched, -1);
        assert_eq!(
            matches(&facts, &mut identity, std::ptr::null_mut()),
            E_POINTER
        );
        assert_eq!(identity, sentinel);
    }
}
