use super::support::dll::Api;
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr::null_mut,
    rc::Rc,
};
use windows::Win32::{
    Foundation::*,
    UI::{Input::KeyboardAndMouse::HKL, TextServices::*},
};
use windows_core::{BOOL, BSTR, GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, PCWSTR};

const SERVICE: GUID = GUID::from_u128(0x1a746212_84db_4acf_88b3_caaa26352774);
const PROFILES: [GUID; 2] = [
    GUID::from_u128(0x0d52a994_9eb5_44ab_b18f_603dc5f69671),
    GUID::from_u128(0x4c1e583c_f8e5_4b7d_b66c_b7e7319aed51),
];
const LANGUAGES: [u16; 2] = [0x409, 0x419];
const DESCRIPTIONS: [&str; 2] = ["Keyrill English", "Keyrill Русский"];
struct Scenario {
    present: [bool; 2],
    enumeration: [HRESULT; 2],
    next: HRESULT,
    description: HRESULT,
    empty_description: bool,
    wrong_description: bool,
    enable: HRESULT,
    readback: HRESULT,
    readback_matches: bool,
    disappear: bool,
    enabled: Cell<bool>,
    probes: [Cell<u32>; 2],
    writes: Cell<u32>,
    profile_adds: Cell<u32>,
    failed: Cell<bool>,
    allocations: Cell<u32>,
    frees: Cell<u32>,
    adds: Cell<u32>,
    releases: Cell<u32>,
    reentry: RefCell<Option<Rc<dyn Fn()>>>,
}
impl Default for Scenario {
    fn default() -> Self {
        Self {
            present: [true, true],
            enumeration: [S_OK; 2],
            next: S_OK,
            description: S_OK,
            empty_description: false,
            wrong_description: false,
            enable: S_OK,
            readback: S_OK,
            readback_matches: true,
            disappear: false,
            enabled: Cell::new(false),
            probes: [Cell::new(0), Cell::new(0)],
            writes: Cell::new(0),
            profile_adds: Cell::new(0),
            failed: Cell::new(false),
            allocations: Cell::new(0),
            frees: Cell::new(0),
            adds: Cell::new(0),
            releases: Cell::new(0),
            reentry: RefCell::new(None),
        }
    }
}
enum Kind {
    Manager,
    Entries { index: usize, returned: Cell<bool> },
}
#[repr(C)]
struct Provider {
    vtable: *const c_void,
    refs: Cell<u32>,
    iid: GUID,
    state: Rc<Scenario>,
    kind: Kind,
}
impl Provider {
    fn create(state: Rc<Scenario>, kind: Kind) -> *mut c_void {
        state.allocations.set(state.allocations.get() + 1);
        let (vtable, iid) = match kind {
            Kind::Manager => (
                (&MANAGER as *const ITfInputProcessorProfiles_Vtbl).cast(),
                ITfInputProcessorProfiles::IID,
            ),
            Kind::Entries { .. } => (
                (&ENTRIES as *const IEnumTfLanguageProfiles_Vtbl).cast(),
                IEnumTfLanguageProfiles::IID,
            ),
        };
        Box::into_raw(Box::new(Self {
            vtable,
            refs: Cell::new(1),
            iid,
            state,
            kind,
        }))
        .cast()
    }
}
fn fence<T>(this: *mut c_void, failure: T, call: impl FnOnce(&Provider) -> T) -> T {
    let object = unsafe { &*this.cast::<Provider>() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(object))) {
        Ok(value) => value,
        Err(_) => {
            object.state.failed.set(true);
            failure
        }
    }
}
unsafe extern "system" fn query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    fence(this, E_UNEXPECTED, |object| {
        if out.is_null() {
            return E_POINTER;
        }
        unsafe { out.write(null_mut()) };
        if iid.is_null() {
            return E_POINTER;
        }
        if unsafe { *iid } == IUnknown::IID || unsafe { *iid } == object.iid {
            unsafe {
                add(this);
                out.write(this)
            };
            S_OK
        } else {
            E_NOINTERFACE
        }
    })
}
unsafe extern "system" fn add(this: *mut c_void) -> u32 {
    fence(this, 1, |object| {
        let count = object.refs.get() + 1;
        object.refs.set(count);
        object.state.adds.set(object.state.adds.get() + 1);
        count
    })
}
unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let state = unsafe { Rc::clone(&(*this.cast::<Provider>()).state) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let count = {
            let object = unsafe { &*this.cast::<Provider>() };
            let count = object.refs.get() - 1;
            object.refs.set(count);
            count
        };
        state.releases.set(state.releases.get() + 1);
        if count == 0 {
            state.frees.set(state.frees.get() + 1);
            unsafe { drop(Box::from_raw(this.cast::<Provider>())) };
        }
        count
    }));
    result.unwrap_or_else(|_| {
        state.failed.set(true);
        0
    })
}
const BASE: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: query,
    AddRef: add,
    Release: release,
};
unsafe extern "system" fn entries(
    this: *mut c_void,
    language: u16,
    out: *mut *mut c_void,
) -> HRESULT {
    fence(this, E_UNEXPECTED, |object| {
        if out.is_null() {
            return E_POINTER;
        }
        unsafe { out.write(null_mut()) };
        let index = LANGUAGES
            .iter()
            .position(|&value| value == language)
            .expect("known profile language");
        object.state.probes[index].set(object.state.probes[index].get() + 1);
        let result = object.state.enumeration[index];
        if result.is_err() {
            return result;
        }
        unsafe {
            out.write(Provider::create(
                Rc::clone(&object.state),
                Kind::Entries {
                    index,
                    returned: Cell::new(false),
                },
            ))
        };
        result
    })
}
unsafe extern "system" fn next(
    this: *mut c_void,
    count: u32,
    out: *mut TF_LANGUAGEPROFILE,
    fetched: *mut u32,
) -> HRESULT {
    fence(this, E_UNEXPECTED, |object| {
        assert_eq!(count, 1);
        assert!(!out.is_null() && !fetched.is_null());
        unsafe {
            out.write(TF_LANGUAGEPROFILE::default());
            fetched.write(0)
        };
        if object.state.next.is_err() {
            return object.state.next;
        }
        let Kind::Entries {
            index,
            ref returned,
        } = object.kind
        else {
            return E_UNEXPECTED;
        };
        if !object.state.present[index]
            || (object.state.disappear && object.state.writes.get() > 0)
            || returned.replace(true)
        {
            return S_FALSE;
        }
        unsafe {
            out.write(TF_LANGUAGEPROFILE {
                clsid: SERVICE,
                langid: LANGUAGES[index],
                guidProfile: PROFILES[index],
                ..Default::default()
            });
            fetched.write(1)
        };
        object.state.next
    })
}
unsafe extern "system" fn description(
    this: *mut c_void,
    service: *const GUID,
    language: u16,
    profile: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    fence(this, E_UNEXPECTED, |object| {
        assert!(!out.is_null());
        unsafe { out.write(null_mut()) };
        let index = LANGUAGES
            .iter()
            .position(|&value| value == language)
            .unwrap();
        assert_eq!(unsafe { *service }, SERVICE);
        assert_eq!(unsafe { *profile }, PROFILES[index]);
        if object.state.description.is_ok() && !object.state.empty_description {
            let value = BSTR::from(if object.state.wrong_description {
                "unrelated"
            } else {
                DESCRIPTIONS[index]
            });
            unsafe { out.write(value.into_raw().cast_mut().cast()) }
        }
        object.state.description
    })
}
unsafe extern "system" fn enable(
    this: *mut c_void,
    service: *const GUID,
    language: u16,
    profile: *const GUID,
    value: BOOL,
) -> HRESULT {
    let state = unsafe { Rc::clone(&(*this.cast::<Provider>()).state) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let index = LANGUAGES.iter().position(|&v| v == language).unwrap();
        assert_eq!(unsafe { *service }, SERVICE);
        assert_eq!(unsafe { *profile }, PROFILES[index]);
        state.writes.set(state.writes.get() + 1);
        state.enabled.set(value.as_bool());
        let action = state.reentry.borrow_mut().take();
        if let Some(action) = action {
            action()
        }
        state.enable
    }));
    result.unwrap_or_else(|_| {
        state.failed.set(true);
        E_UNEXPECTED
    })
}
unsafe extern "system" fn enabled(
    this: *mut c_void,
    _: *const GUID,
    _: u16,
    _: *const GUID,
    out: *mut BOOL,
) -> HRESULT {
    fence(this, E_UNEXPECTED, |object| {
        if out.is_null() {
            return E_POINTER;
        }
        unsafe {
            out.write(BOOL::from(
                object.state.enabled.get() == object.state.readback_matches,
            ))
        };
        object.state.readback
    })
}
macro_rules! stub {($name:ident($($arg:ident:$ty:ty),* $(,)?))=>{unsafe extern "system" fn $name($($arg:$ty),*)->HRESULT {$(let _=$arg;)* E_NOTIMPL}};}
stub!(guid_stub(this: *mut c_void, id: *const GUID));
stub!(profile_stub(this: *mut c_void, id: *const GUID, lang: u16, profile: *const GUID));
unsafe extern "system" fn add_profile(
    this: *mut c_void,
    id: *const GUID,
    lang: u16,
    profile: *const GUID,
    description: PCWSTR,
    length: u32,
    icon: PCWSTR,
    icon_length: u32,
    index: u32,
) -> HRESULT {
    fence(this, E_UNEXPECTED, |object| {
        let position = object.state.profile_adds.get() as usize;
        assert_eq!(unsafe { *id }, SERVICE);
        assert_eq!(lang, LANGUAGES[position]);
        assert_eq!(unsafe { *profile }, PROFILES[position]);
        let expected: Vec<u16> = DESCRIPTIONS[position].encode_utf16().collect();
        assert_eq!(length as usize, expected.len());
        // SAFETY: AddLanguageProfile supplies the declared readable counted buffers.
        assert_eq!(
            unsafe { std::slice::from_raw_parts(description.0, length as usize) },
            expected
        );
        let expected_icon: Vec<u16> = "KeyrillTsf.dll".encode_utf16().collect();
        assert_eq!(icon_length as usize, expected_icon.len());
        // SAFETY: the counted icon path is retained through this callback.
        assert_eq!(
            unsafe { std::slice::from_raw_parts(icon.0, icon_length as usize) },
            expected_icon
        );
        assert_eq!(index, 0);
        object.state.profile_adds.set(position as u32 + 1);
        S_OK
    })
}
stub!(default_stub(this: *mut c_void, lang: u16, id: *const GUID, profile: *const GUID));
stub!(change_stub(this: *mut c_void, lang: u16));
stub!(
    substitute_stub(
        this: *mut c_void,
        id: *const GUID,
        lang: u16,
        profile: *const GUID,
        layout: HKL,
    )
);
stub!(reset_stub(this: *mut c_void));
stub!(skip_stub(this: *mut c_void, count: u32));
unsafe extern "system" fn enum_stub(_: *mut c_void, out: *mut *mut c_void) -> HRESULT {
    if !out.is_null() {
        unsafe { out.write(null_mut()) }
    }
    E_NOTIMPL
}
unsafe extern "system" fn get_default_stub(
    _: *mut c_void,
    _: u16,
    _: *const GUID,
    id: *mut GUID,
    profile: *mut GUID,
) -> HRESULT {
    unsafe {
        if !id.is_null() {
            id.write(GUID::zeroed())
        }
        if !profile.is_null() {
            profile.write(GUID::zeroed())
        }
    }
    E_NOTIMPL
}
unsafe extern "system" fn active_stub(
    _: *mut c_void,
    _: *const GUID,
    lang: *mut u16,
    profile: *mut GUID,
) -> HRESULT {
    unsafe {
        if !lang.is_null() {
            lang.write(0)
        }
        if !profile.is_null() {
            profile.write(GUID::zeroed())
        }
    }
    E_NOTIMPL
}
unsafe extern "system" fn language_stub(_: *mut c_void, out: *mut u16) -> HRESULT {
    if !out.is_null() {
        unsafe { out.write(0) }
    }
    E_NOTIMPL
}
unsafe extern "system" fn list_stub(
    _: *mut c_void,
    out: *mut *mut u16,
    count: *mut u32,
) -> HRESULT {
    unsafe {
        if !out.is_null() {
            out.write(null_mut())
        }
        if !count.is_null() {
            count.write(0)
        }
    }
    E_NOTIMPL
}
static ENTRIES: IEnumTfLanguageProfiles_Vtbl = IEnumTfLanguageProfiles_Vtbl {
    base__: BASE,
    Clone: enum_stub,
    Next: next,
    Reset: reset_stub,
    Skip: skip_stub,
};
static MANAGER: ITfInputProcessorProfiles_Vtbl = ITfInputProcessorProfiles_Vtbl {
    base__: BASE,
    Register: guid_stub,
    Unregister: guid_stub,
    AddLanguageProfile: add_profile,
    RemoveLanguageProfile: profile_stub,
    EnumInputProcessorInfo: enum_stub,
    GetDefaultLanguageProfile: get_default_stub,
    SetDefaultLanguageProfile: default_stub,
    ActivateLanguageProfile: profile_stub,
    GetActiveLanguageProfile: active_stub,
    GetLanguageProfileDescription: description,
    GetCurrentLanguage: language_stub,
    ChangeCurrentLanguage: change_stub,
    GetLanguageList: list_stub,
    EnumLanguageProfiles: entries,
    EnableLanguageProfile: enable,
    IsEnabledLanguageProfile: enabled,
    EnableLanguageProfileByDefault: enable,
    SubstituteKeyboardLayout: substitute_stub,
};
type Registration = unsafe extern "system" fn(*mut c_void, u32) -> HRESULT;
fn run(api: &Api, scenario: Scenario, operations: &[(u32, HRESULT, u32)], preflight_refusal: bool) {
    let call: Registration = unsafe { api.export(b"KeyrillTestRegistration\0") };
    let state = Rc::new(scenario);
    let manager = unsafe {
        ITfInputProcessorProfiles::from_raw(Provider::create(Rc::clone(&state), Kind::Manager))
    };
    for &(operation, result, writes) in operations {
        assert_eq!(unsafe { call(manager.as_raw(), operation) }, result);
        assert_eq!(state.writes.get(), writes);
        if writes > 0 {
            assert_eq!(state.enabled.get(), operation == 1);
        }
        assert!(!state.failed.get());
    }
    if preflight_refusal {
        assert_eq!(state.probes.each_ref().map(Cell::get), [1, 1]);
    }
    drop(manager);
    assert_eq!(state.allocations.get(), state.frees.get());
    assert_eq!(
        state.releases.get(),
        state.adds.get() + state.allocations.get()
    );
}
#[test]
fn registrar_add_sends_exact_counted_profile_descriptions() {
    let api = Api::load();
    let call: Registration = unsafe { api.export(b"KeyrillTestRegistration\0") };
    let state = Rc::new(Scenario::default());
    let manager = unsafe {
        ITfInputProcessorProfiles::from_raw(Provider::create(Rc::clone(&state), Kind::Manager))
    };
    assert_eq!(unsafe { call(manager.as_raw(), 3) }, S_OK);
    assert_eq!(state.profile_adds.get(), 2);
    assert_eq!(state.writes.get(), 0);
    assert!(!state.failed.get());
    drop(manager);
    assert_eq!(state.allocations.get(), state.frees.get());
    assert_eq!(
        state.releases.get(),
        state.adds.get() + state.allocations.get()
    );
}

#[test]
fn registrar_presence_preflight_and_acknowledgement_axes() {
    let api = Api::load();
    for index in 0..2 {
        let mut state = Scenario::default();
        state.present[index] = false;
        run(&api, state, &[(1, ERROR_NOT_FOUND.to_hresult(), 0)], true);
        println!("scenario registration.missing-{index}");
    }
    for index in 0..2 {
        let mut state = Scenario::default();
        state.enumeration[index] = E_ACCESSDENIED;
        run(&api, state, &[(1, E_ACCESSDENIED, 0)], true);
        println!("scenario registration.enumeration-{index}");
    }
    for (name, state, result) in [
        (
            "next",
            Scenario {
                next: E_FAIL,
                ..Default::default()
            },
            E_FAIL,
        ),
        (
            "description",
            Scenario {
                description: E_FAIL,
                ..Default::default()
            },
            E_FAIL,
        ),
        (
            "empty-description",
            Scenario {
                empty_description: true,
                ..Default::default()
            },
            E_UNEXPECTED,
        ),
        (
            "wrong-description",
            Scenario {
                wrong_description: true,
                ..Default::default()
            },
            E_UNEXPECTED,
        ),
    ] {
        run(&api, state, &[(1, result, 0)], true);
        println!("scenario registration.{name}");
    }
    run(&api, Scenario::default(), &[(0, S_OK, 0)], false);
    run(
        &api,
        Scenario::default(),
        &[(1, S_OK, 2), (2, S_OK, 4)],
        false,
    );
    for (name, state, operation, result, writes) in [
        (
            "failed-write",
            Scenario {
                enable: E_ACCESSDENIED,
                ..Default::default()
            },
            1,
            E_ACCESSDENIED,
            1,
        ),
        (
            "mismatch",
            Scenario {
                readback_matches: false,
                ..Default::default()
            },
            1,
            E_FAIL,
            2,
        ),
        (
            "readback-failure",
            Scenario {
                readback: E_ACCESSDENIED,
                ..Default::default()
            },
            1,
            E_ACCESSDENIED,
            2,
        ),
        (
            "readback-s-false",
            Scenario {
                readback: S_FALSE,
                ..Default::default()
            },
            2,
            E_UNEXPECTED,
            2,
        ),
        (
            "write-s-false",
            Scenario {
                enable: S_FALSE,
                ..Default::default()
            },
            1,
            E_UNEXPECTED,
            1,
        ),
        (
            "disappeared",
            Scenario {
                disappear: true,
                ..Default::default()
            },
            1,
            ERROR_NOT_FOUND.to_hresult(),
            2,
        ),
    ] {
        run(&api, state, &[(operation, result, writes)], false);
        println!("scenario registration.{name}");
    }
    println!("scenario registration.check-read-only.enable-disable-readback");
}

#[test]
fn registrar_exact_outputs_and_after_effect_reentry() {
    let api = Api::load();
    for status in [S_FALSE, E_ACCESSDENIED] {
        run(
            &api,
            Scenario {
                enumeration: [status, S_OK],
                ..Default::default()
            },
            &[(
                1,
                if status == S_FALSE {
                    E_UNEXPECTED
                } else {
                    status
                },
                0,
            )],
            true,
        );
        run(
            &api,
            Scenario {
                description: status,
                ..Default::default()
            },
            &[(
                1,
                if status == S_FALSE {
                    E_UNEXPECTED
                } else {
                    status
                },
                0,
            )],
            true,
        );
        println!("scenario registration.exact-output-{:08x}", status.0 as u32);
    }
    run(
        &api,
        Scenario {
            next: S_FALSE,
            ..Default::default()
        },
        &[(1, E_UNEXPECTED, 0)],
        true,
    );
    let call: Registration = unsafe { api.export(b"KeyrillTestRegistration\0") };
    for status in [S_OK, E_ACCESSDENIED] {
        let state = Rc::new(Scenario {
            enable: status,
            ..Default::default()
        });
        let manager = unsafe {
            ITfInputProcessorProfiles::from_raw(Provider::create(Rc::clone(&state), Kind::Manager))
        };
        let pointer = manager.as_raw() as usize;
        let nested = Rc::new(Cell::new(false));
        let observed = Rc::clone(&nested);
        let weak = Rc::downgrade(&state);
        *state.reentry.borrow_mut() = Some(Rc::new(move || {
            let state = weak.upgrade().unwrap();
            assert!(state.enabled.get());
            assert_eq!(state.writes.get(), 1);
            assert_eq!(unsafe { call(pointer as *mut c_void, 0) }, S_OK);
            observed.set(true);
        }));
        assert_eq!(unsafe { call(manager.as_raw(), 1) }, status);
        assert!(nested.get());
        assert!(state.enabled.get());
        assert_eq!(state.writes.get(), if status == S_OK { 2 } else { 1 });
        assert!(!state.failed.get());
        drop(manager);
        assert_eq!(state.allocations.get(), state.frees.get());
        assert_eq!(
            state.releases.get(),
            state.adds.get() + state.allocations.get()
        );
        println!(
            "scenario registration.after-effect-reentry-{:08x}",
            status.0 as u32
        );
    }
}
