#![allow(non_snake_case)]
use super::*;
use crate::journal::{Backend, Change, Journal};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use windows::Win32::Foundation::E_FAIL;
use windows_core::{BOOL, BSTR, GUID, PCWSTR, implement};

#[path = "tsf_qualification_cleanup.rs"]
mod qualification_cleanup;

#[cfg(feature = "maintainer-native-qualification")]
#[path = "tsf_native_qualification.rs"]
mod qualification;

#[cfg(feature = "maintainer-native-qualification")]
#[test]
#[ignore = "maintainer-only elevated native TSF registration qualification"]
fn disposable_native_profile_repair_preserves_protected_state() {
    qualification::run().unwrap();
}

#[cfg(feature = "maintainer-native-qualification")]
#[test]
#[ignore = "explicit read-only native qualification observer preflight"]
fn read_only_native_profile_qualification_preflight() {
    qualification::read_only_preflight().unwrap();
}

#[test]
fn profile_string_storage_terminates_descriptions_and_icon_paths() {
    for (value, expected) in [
        ("Keyrill English", "Keyrill English\0"),
        ("Keyrill Русский", "Keyrill Русский\0"),
        ("", "\0"),
        ("Я🔑", "Я🔑\0"),
        (r"C:\Keyrill\Русский🔑.ico", "C:\\Keyrill\\Русский🔑.ico\0"),
    ] {
        assert_eq!(
            profile_buffer(value),
            expected.encode_utf16().collect::<Vec<_>>(),
            "unterminated TSF profile storage for {value:?}"
        );
    }
}

#[test]
fn profile_native_character_counts_preserve_exact_utf16_text() {
    let state = Rc::new(State {
        value: RefCell::new(Value::Null),
        layout: RefCell::new(Value::Null),
        writes: Cell::new(0),
        layout_writes: Cell::new(0),
        failure: Cell::new(Failure::None),
        layout_failure: Cell::new(Failure::None),
        service: None,
    });
    let provider: ITfInputProcessorProfiles = Provider(state.clone()).into();
    let mut writes = 0;
    for (index, profile) in PROFILES.iter().enumerate() {
        for description in [profile.description, "", "Я🔑"] {
            for icon in ["public-english.ico", "", r"C:\Keyrill\Русский🔑.ico"] {
                let desired = json!({"description":description,"icon":icon,"index":0});
                profile_write_with(&provider, index, &desired).unwrap();
                writes += 1;
                assert_eq!(*state.value.borrow(), desired);
                assert_eq!(state.writes.get(), writes);
            }
        }
    }
}

#[test]
fn machine_service_accepts_exact_native_enable_metadata() {
    let path = format!(
        "Software\\KeyrillTests\\ServiceMetadata-{}",
        std::process::id()
    );
    assert!(!registry::exists(HKEY_CURRENT_USER, KEY_WOW64_64KEY, &path).unwrap());
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        &path,
        "Enable",
        &registry::string("1"),
    )
    .unwrap();
    let observed = service_metadata_at(HKEY_CURRENT_USER, KEY_WOW64_64KEY, &path);
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        &path,
        "Enable",
        &Value::Null,
    )
    .unwrap();
    let wide = registry::wide(&path);
    // SAFETY: this uniquely named disposable key has no values or descendants.
    unsafe {
        RegDeleteKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide.as_ptr()),
            KEY_WOW64_64KEY.0,
            None,
        )
    }
    .ok()
    .unwrap();
    assert_eq!(observed.unwrap(), registry::string("1"));
}

#[test]
fn standard_layout_requires_the_complete_native_handle_identity() {
    let expected = HKL(0x04090409usize as *mut _);
    assert_eq!(standard_layout(&[expected], 0x0409), Some(expected));
    assert_eq!(standard_layout(&[expected], 0x0419), None);
    #[cfg(target_pointer_width = "64")]
    {
        let private = HKL(0xffff_ffff_0409_0409usize as *mut _);
        assert_eq!(standard_layout(&[private], 0x0409), None);
        assert_eq!(
            standard_layout(&[private, expected], 0x0409),
            Some(expected)
        );
    }
}

#[derive(Clone, Copy)]
enum Failure {
    None,
    Before,
    After,
    Partial,
}
struct State {
    value: RefCell<Value>,
    layout: RefCell<Value>,
    writes: Cell<usize>,
    layout_writes: Cell<usize>,
    failure: Cell<Failure>,
    layout_failure: Cell<Failure>,
    service: Option<Rc<ServiceState>>,
}
#[implement(ITfInputProcessorProfiles)]
struct Provider(Rc<State>);
impl ITfInputProcessorProfiles_Impl for Provider_Impl {
    fn AddLanguageProfile(
        &self,
        class: *const GUID,
        language: u16,
        profile: *const GUID,
        description: &PCWSTR,
        description_len: u32,
        icon: &PCWSTR,
        icon_len: u32,
        index: u32,
    ) -> windows_core::Result<()> {
        // SAFETY: this provider is invoked only by the typed SDK caller with live GUID/buffer inputs.
        let (class, profile, description, icon) = unsafe {
            (
                *class,
                *profile,
                std::slice::from_raw_parts(description.0, description_len as usize),
                std::slice::from_raw_parts(icon.0, icon_len as usize),
            )
        };
        assert_eq!(class, SERVICE_ID);
        let expected = PROFILES.iter().find(|p| p.language == language).unwrap();
        assert_eq!(profile, expected.id);
        assert_eq!(index, 0);
        if matches!(self.0.failure.get(), Failure::Before) {
            return Err(E_FAIL.into());
        }
        self.0.writes.set(self.0.writes.get() + 1);
        *self.0.value.borrow_mut() = if matches!(self.0.failure.get(), Failure::Partial) {
            json!({"description":"partial native effect"})
        } else {
            json!({"description":String::from_utf16(description).unwrap(),"icon":String::from_utf16(icon).unwrap(),"index":0})
        };
        if matches!(self.0.failure.get(), Failure::After | Failure::Partial) {
            Err(E_FAIL.into())
        } else {
            Ok(())
        }
    }
    fn RemoveLanguageProfile(
        &self,
        _: *const GUID,
        _: u16,
        _: *const GUID,
    ) -> windows_core::Result<()> {
        *self.0.value.borrow_mut() = Value::Null;
        self.0.writes.set(self.0.writes.get() + 1);
        Ok(())
    }
    fn Register(&self, class: *const GUID) -> windows_core::Result<()> {
        // SAFETY: the typed SDK caller retains the GUID for this call.
        assert_eq!(unsafe { *class }, SERVICE_ID);
        self.0
            .service
            .as_ref()
            .expect("unexpected Register")
            .apply(true)
    }
    fn Unregister(&self, class: *const GUID) -> windows_core::Result<()> {
        // SAFETY: the typed SDK caller retains the GUID for this call.
        assert_eq!(unsafe { *class }, SERVICE_ID);
        self.0
            .service
            .as_ref()
            .expect("unexpected Unregister")
            .apply(false)
    }
    fn EnumInputProcessorInfo(&self) -> windows_core::Result<IEnumGUID> {
        panic!("unexpected enumeration")
    }
    fn GetDefaultLanguageProfile(
        &self,
        _: u16,
        _: *const GUID,
        _: *mut GUID,
        _: *mut GUID,
    ) -> windows_core::Result<()> {
        panic!("unexpected default read")
    }
    fn SetDefaultLanguageProfile(
        &self,
        _: u16,
        _: *const GUID,
        _: *const GUID,
    ) -> windows_core::Result<()> {
        panic!("source selection forbidden")
    }
    fn ActivateLanguageProfile(
        &self,
        _: *const GUID,
        _: u16,
        _: *const GUID,
    ) -> windows_core::Result<()> {
        panic!("source activation forbidden")
    }
    fn GetActiveLanguageProfile(
        &self,
        _: *const GUID,
        _: *mut u16,
        _: *mut GUID,
    ) -> windows_core::Result<()> {
        panic!("unexpected active read")
    }
    fn GetLanguageProfileDescription(
        &self,
        _: *const GUID,
        _: u16,
        _: *const GUID,
    ) -> windows_core::Result<BSTR> {
        panic!("unexpected description query")
    }
    fn GetCurrentLanguage(&self) -> windows_core::Result<u16> {
        panic!("unexpected language read")
    }
    fn ChangeCurrentLanguage(&self, _: u16) -> windows_core::Result<()> {
        panic!("source change forbidden")
    }
    fn GetLanguageList(&self, _: *mut *mut u16, _: *mut u32) -> windows_core::Result<()> {
        panic!("unexpected language list")
    }
    fn EnumLanguageProfiles(&self, _: u16) -> windows_core::Result<IEnumTfLanguageProfiles> {
        panic!("unexpected profiles enumeration")
    }
    fn EnableLanguageProfile(
        &self,
        _: *const GUID,
        _: u16,
        _: *const GUID,
        _: BOOL,
    ) -> windows_core::Result<()> {
        panic!("source enablement forbidden")
    }
    fn IsEnabledLanguageProfile(
        &self,
        _: *const GUID,
        _: u16,
        _: *const GUID,
    ) -> windows_core::Result<BOOL> {
        panic!("unexpected enablement read")
    }
    fn EnableLanguageProfileByDefault(
        &self,
        _: *const GUID,
        _: u16,
        _: *const GUID,
        _: BOOL,
    ) -> windows_core::Result<()> {
        panic!("default enablement forbidden")
    }
    fn SubstituteKeyboardLayout(
        &self,
        class: *const GUID,
        language: u16,
        profile: *const GUID,
        layout: HKL,
    ) -> windows_core::Result<()> {
        // SAFETY: the typed caller retains its fixed service and profile GUIDs for this call.
        unsafe {
            assert_eq!(*class, SERVICE_ID);
            assert_eq!(*profile, PROFILES[0].id);
        }
        assert_eq!(language, PROFILES[0].language);
        assert_eq!(layout.0 as usize, 0x04090409);
        let failure = self.0.layout_failure.get();
        if matches!(failure, Failure::Before) {
            return Err(E_FAIL.into());
        }
        self.0.layout_writes.set(self.0.layout_writes.get() + 1);
        *self.0.layout.borrow_mut() = if matches!(failure, Failure::Partial) {
            json!("partial native layout")
        } else {
            json!("0x04090409")
        };
        if matches!(failure, Failure::After | Failure::Partial) {
            Err(E_FAIL.into())
        } else {
            Ok(())
        }
    }
}
struct Fixture {
    provider: ITfInputProcessorProfiles,
    state: Rc<State>,
    receipt: Option<Journal>,
    interrupt_after_profile: bool,
}
impl Backend for Fixture {
    fn read(&mut self, resource: &str) -> Result<Value> {
        match resource {
            "profile/en" => Ok(self.state.value.borrow().clone()),
            "layout/en" => Ok(self.state.layout.borrow().clone()),
            _ => panic!("unexpected resource"),
        }
    }
    fn write(&mut self, resource: &str, value: &Value) -> Result<()> {
        match resource {
            "profile/en" => profile_write_with(&self.provider, 0, value),
            "layout/en" => {
                layout_write_with(&self.provider, 0, value, HKL(0x04090409usize as *mut _))
            }
            _ => panic!("unexpected resource"),
        }
    }
    fn persist(&mut self, journal: &Journal) -> Result<()> {
        self.receipt = Some(journal.clone());
        if self.interrupt_after_profile && journal.completed == 1 {
            return Err("interrupted after profile checkpoint".into());
        }
        Ok(())
    }
}
#[test]
fn injected_native_profile_hresult_is_not_an_installation_acknowledgement() {
    for failure in [
        Failure::None,
        Failure::Before,
        Failure::After,
        Failure::Partial,
    ] {
        let state = Rc::new(State {
            value: RefCell::new(Value::Null),
            layout: RefCell::new(Value::Null),
            writes: Cell::new(0),
            layout_writes: Cell::new(0),
            failure: Cell::new(failure),
            layout_failure: Cell::new(Failure::None),
            service: None,
        });
        let provider: ITfInputProcessorProfiles = Provider(state.clone()).into();
        let mut backend = Fixture {
            provider,
            state: state.clone(),
            receipt: None,
            interrupt_after_profile: false,
        };
        let desired =
            json!({"description":PROFILES[0].description,"icon":"public-english.ico","index":0});
        let mut journal = Journal {
            desired: "a".repeat(64),
            previous: None,
            completed: 0,
            changes: vec![Change {
                resource: "profile/en".into(),
                before: Value::Null,
                after: desired.clone(),
            }],
        };
        let outcome = journal.reconcile(&mut backend);
        assert_eq!(
            outcome.is_ok(),
            matches!(failure, Failure::None | Failure::After)
        );
        state.failure.set(Failure::None);
        let mut recovered = backend.receipt.clone().unwrap();
        if matches!(failure, Failure::Partial) {
            assert!(recovered.reconcile(&mut backend).is_err());
            assert_eq!(state.writes.get(), 1);
            assert_eq!(recovered.completed, 0);
        } else {
            recovered.reconcile(&mut backend).unwrap();
            assert_eq!(*state.value.borrow(), desired);
            assert_eq!(state.writes.get(), 1);
            profile_write_with(&backend.provider, 0, &Value::Null).unwrap();
            assert!(state.value.borrow().is_null());
        }
    }
}

#[test]
fn profile_checkpoint_and_substitute_layout_failures_recover_without_replaying_profile() {
    for failure in [
        Failure::None,
        Failure::Before,
        Failure::After,
        Failure::Partial,
    ] {
        let state = Rc::new(State {
            value: RefCell::new(Value::Null),
            layout: RefCell::new(Value::Null),
            writes: Cell::new(0),
            layout_writes: Cell::new(0),
            failure: Cell::new(Failure::None),
            layout_failure: Cell::new(failure),
            service: None,
        });
        let provider: ITfInputProcessorProfiles = Provider(state.clone()).into();
        let mut backend = Fixture {
            provider,
            state: state.clone(),
            receipt: None,
            interrupt_after_profile: true,
        };
        let mut journal = Journal {
            desired: "a".repeat(64),
            previous: None,
            completed: 0,
            changes: vec![
                Change {
                    resource: "profile/en".into(),
                    before: Value::Null,
                    after: json!({"description":PROFILES[0].description,"icon":"public-english.ico","index":0}),
                },
                Change {
                    resource: "layout/en".into(),
                    before: Value::Null,
                    after: json!("0x04090409"),
                },
            ],
        };
        assert!(journal.reconcile(&mut backend).is_err());
        assert_eq!(state.writes.get(), 1);
        assert_eq!(state.layout_writes.get(), 0);
        backend.interrupt_after_profile = false;
        let mut recovered = backend.receipt.clone().unwrap();
        assert_eq!(recovered.completed, 1);
        assert_eq!(
            recovered.reconcile(&mut backend).is_ok(),
            matches!(failure, Failure::None | Failure::After)
        );
        state.layout_failure.set(Failure::None);
        let mut retried = backend.receipt.clone().unwrap();
        if matches!(failure, Failure::Partial) {
            assert!(retried.reconcile(&mut backend).is_err());
            assert_eq!(*state.layout.borrow(), json!("partial native layout"));
        } else {
            retried.reconcile(&mut backend).unwrap();
            assert_eq!(*state.layout.borrow(), json!("0x04090409"));
        }
        assert_eq!(state.writes.get(), 1);
        assert_eq!(state.layout_writes.get(), 1);
    }
}

struct ServiceRoots([String; 2]);
impl ServiceRoots {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let paths = [64, 32].map(|view| {
            let path = format!(
                "Software\\KeyrillTests\\Service-{}-{id}-{view}",
                std::process::id()
            );
            assert!(!registry::exists(HKEY_CURRENT_USER, KEY_WOW64_64KEY, &path).unwrap());
            path
        });
        Self(paths)
    }
    fn read(&self) -> Result<Value> {
        service_state_with(|view| {
            let path = &self.0[usize::from(view == KEY_WOW64_32KEY)];
            Ok((
                registry::exists(HKEY_CURRENT_USER, view, path)?,
                service_metadata_at(HKEY_CURRENT_USER, view, path)?,
            ))
        })
    }
    fn set(&self, present: bool, metadata: &Value) {
        for path in &self.0 {
            Self::remove(path);
            if present {
                registry::write(
                    HKEY_CURRENT_USER,
                    KEY_WOW64_64KEY,
                    path,
                    "Enable",
                    &registry::string("1"),
                )
                .unwrap();
                registry::write(HKEY_CURRENT_USER, KEY_WOW64_64KEY, path, "Enable", metadata)
                    .unwrap();
            }
        }
    }
    fn remove(path: &str) {
        for name in registry::names(HKEY_CURRENT_USER, KEY_WOW64_64KEY, path).unwrap() {
            registry::write(
                HKEY_CURRENT_USER,
                KEY_WOW64_64KEY,
                path,
                &name,
                &Value::Null,
            )
            .unwrap();
        }
        if registry::exists(HKEY_CURRENT_USER, KEY_WOW64_64KEY, path).unwrap() {
            let wide = registry::wide(path);
            // SAFETY: only this fixture's unique childless HKCU key is removed.
            unsafe {
                RegDeleteKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(wide.as_ptr()),
                    KEY_WOW64_64KEY.0,
                    None,
                )
            }
            .ok()
            .unwrap();
        }
    }
}
impl Drop for ServiceRoots {
    fn drop(&mut self) {
        for path in &self.0 {
            Self::remove(path);
        }
    }
}

#[test]
fn service_readback_rejects_unrecognized_raw_metadata_and_view_drift() {
    for metadata in [
        json!({"type":1,"data":[49,0]}),
        json!({"type":1,"data":[49,0,0]}),
        json!({"type":1,"data":[]}),
    ] {
        assert!(
            validate_service_metadata("malformed raw fixture", &["Enable".into()], &metadata)
                .is_err()
        );
    }
    let roots = ServiceRoots::new();
    assert_eq!(roots.read().unwrap(), false);
    for metadata in [Value::Null, registry::string("1")] {
        roots.set(true, &metadata);
        assert_eq!(roots.read().unwrap(), true);
    }
    for metadata in [
        registry::dword(1),
        registry::string("0"),
        registry::string(""),
        json!({"type":1,"data":[49,0,0,0,0,0]}),
        json!({"type":1,"data":[49,0,0,0,65,0,0,0]}),
        json!({"type":1,"data":[0,216,0,0]}),
    ] {
        roots.set(true, &metadata);
        for path in &roots.0 {
            assert_eq!(
                registry::read(HKEY_CURRENT_USER, KEY_WOW64_64KEY, path, "Enable").unwrap(),
                metadata
            );
        }
        let observed = roots.read();
        assert!(
            observed.is_err(),
            "accepted malformed fixture {metadata:?}: {observed:?}"
        );
        let error = observed.unwrap_err().to_string();
        assert!(error.contains("Enable has invalid type or data"), "{error}");
    }
    roots.set(true, &Value::Null);
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        &roots.0[0],
        "Unknown",
        &registry::dword(1),
    )
    .unwrap();
    assert!(
        roots
            .read()
            .unwrap_err()
            .to_string()
            .contains("unexpected value name")
    );
    roots.set(true, &Value::Null);
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        &roots.0[1],
        "Enable",
        &registry::string("1"),
    )
    .unwrap();
    assert!(
        roots
            .read()
            .unwrap_err()
            .to_string()
            .contains("differs between registry views")
    );
    ServiceRoots::remove(&roots.0[0]);
    assert!(roots.read().is_err());
    roots.set(true, &Value::Null);
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        &roots.0[0],
        "eNaBlE",
        &registry::string("1"),
    )
    .unwrap();
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        &roots.0[1],
        "eNaBlE",
        &registry::string("1"),
    )
    .unwrap();
    assert_eq!(roots.read().unwrap(), true);
}

#[test]
fn service_view_query_failures_propagate_before_boolean_projection() {
    for failing_view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        let error = service_state_with(|view| {
            if view == failing_view {
                Err("injected service registry query failure".into())
            } else {
                Ok((true, Value::Null))
            }
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "injected service registry query failure");
    }
    assert!(service_state_with(|_| Ok((false, registry::string("1")))).is_err());
}

struct ServiceState {
    roots: ServiceRoots,
    metadata: Value,
    failure: Cell<Failure>,
    writes: Cell<usize>,
}
impl ServiceState {
    fn apply(&self, present: bool) -> windows_core::Result<()> {
        if matches!(self.failure.get(), Failure::Before) {
            return Err(E_FAIL.into());
        }
        self.writes.set(self.writes.get() + 1);
        let metadata = if matches!(self.failure.get(), Failure::Partial) {
            registry::dword(1)
        } else {
            self.metadata.clone()
        };
        self.roots.set(
            present || matches!(self.failure.get(), Failure::Partial),
            &metadata,
        );
        if matches!(self.failure.get(), Failure::After | Failure::Partial) {
            Err(E_FAIL.into())
        } else {
            Ok(())
        }
    }
}
struct ServiceFixture {
    provider: ITfInputProcessorProfiles,
    state: Rc<ServiceState>,
    saved: Option<Journal>,
    calls: usize,
    fail_at: usize,
}
impl ServiceFixture {
    fn new(metadata: Value, failure: Failure) -> Self {
        let service = Rc::new(ServiceState {
            roots: ServiceRoots::new(),
            metadata,
            failure: Cell::new(failure),
            writes: Cell::new(0),
        });
        let state = Rc::new(State {
            value: RefCell::new(Value::Null),
            layout: RefCell::new(Value::Null),
            writes: Cell::new(0),
            layout_writes: Cell::new(0),
            failure: Cell::new(Failure::None),
            layout_failure: Cell::new(Failure::None),
            service: Some(service.clone()),
        });
        Self {
            provider: Provider(state).into(),
            state: service,
            saved: None,
            calls: 0,
            fail_at: usize::MAX,
        }
    }
    fn tick(&mut self) -> Result<()> {
        self.calls += 1;
        if self.calls == self.fail_at {
            return Err("injected service interruption".into());
        }
        Ok(())
    }
}
impl Backend for ServiceFixture {
    fn read(&mut self, resource: &str) -> Result<Value> {
        assert_eq!(resource, "service");
        self.tick()?;
        self.state.roots.read()
    }
    fn write(&mut self, resource: &str, value: &Value) -> Result<()> {
        assert_eq!(resource, "service");
        self.tick()?;
        service_write_with(&self.provider, value.as_bool().unwrap())?;
        self.tick()
    }
    fn persist(&mut self, journal: &Journal) -> Result<()> {
        self.tick()?;
        self.saved = Some(journal.clone());
        self.tick()
    }
}
fn service_journal(before: bool, after: bool) -> Journal {
    Journal {
        desired: "a".repeat(64),
        previous: None,
        completed: 0,
        changes: vec![Change {
            resource: "service".into(),
            before: json!(before),
            after: json!(after),
        }],
    }
}

#[test]
fn service_native_effects_recover_from_every_checkpoint_without_replay() {
    for metadata in [Value::Null, registry::string("1")] {
        for before in [false, true] {
            for fail_at in 1..=10 {
                let mut backend = ServiceFixture::new(metadata.clone(), Failure::None);
                backend.state.roots.set(before, &metadata);
                backend.fail_at = fail_at;
                let original = service_journal(before, !before);
                let mut journal = original.clone();
                let _ = journal.reconcile(&mut backend);
                backend.fail_at = usize::MAX;
                let saved = backend.saved.clone().unwrap_or(original);
                let mut recovered =
                    Journal::parse(&serde_json::to_vec(&saved.value()).unwrap()).unwrap();
                recovered.reconcile(&mut backend).unwrap();
                assert_eq!(backend.state.roots.read().unwrap(), !before);
                assert_eq!(backend.state.writes.get(), 1, "interruption {fail_at}");
                recovered.reconcile(&mut backend).unwrap();
                assert_eq!(backend.state.writes.get(), 1);
            }
        }
    }
}

#[test]
fn service_hresult_acknowledgement_requires_fresh_valid_metadata() {
    for metadata in [Value::Null, registry::string("1")] {
        for before in [false, true] {
            for failure in [Failure::Before, Failure::After, Failure::Partial] {
                let mut backend = ServiceFixture::new(metadata.clone(), failure);
                backend.state.roots.set(before, &metadata);
                let mut journal = service_journal(before, !before);
                let outcome = journal.reconcile(&mut backend);
                assert_eq!(outcome.is_ok(), matches!(failure, Failure::After));
                if matches!(failure, Failure::Partial) {
                    assert_eq!(
                        outcome.unwrap_err().to_string(),
                        "installation unknown effect: readback unavailable; receipt retained"
                    );
                    assert_eq!(backend.saved.as_ref().unwrap().completed, 0);
                }
                backend.state.failure.set(Failure::None);
                let mut recovered = backend.saved.clone().unwrap();
                assert_eq!(
                    recovered.reconcile(&mut backend).is_err(),
                    matches!(failure, Failure::Partial)
                );
                assert_eq!(backend.state.writes.get(), 1);
            }
        }
    }
}

#[test]
fn existing_service_boolean_receipt_skips_register_and_refuses_later_metadata_drift() {
    for metadata in [Value::Null, registry::string("1")] {
        let mut backend = ServiceFixture::new(metadata.clone(), Failure::None);
        backend.state.roots.set(true, &metadata);
        let original = service_journal(true, true);
        let mut old_receipt =
            Journal::parse(&serde_json::to_vec(&original.value()).unwrap()).unwrap();
        old_receipt.reconcile(&mut backend).unwrap();
        assert_eq!(backend.state.writes.get(), 0);
        let equivalent = if metadata.is_null() {
            registry::string("1")
        } else {
            Value::Null
        };
        backend.state.roots.set(true, &equivalent);
        let mut recovered = backend.saved.clone().unwrap();
        recovered.reconcile(&mut backend).unwrap();
        assert_eq!(backend.state.writes.get(), 0);
        backend.state.roots.set(true, &registry::dword(1));
        let mut recovered = backend.saved.clone().unwrap();
        assert!(recovered.reconcile(&mut backend).is_err());
        assert_eq!(backend.state.writes.get(), 0);
        backend.state.roots.set(true, &metadata);
        registry::write(
            HKEY_CURRENT_USER,
            KEY_WOW64_64KEY,
            &backend.state.roots.0[0],
            "Unknown",
            &registry::dword(1),
        )
        .unwrap();
        assert!(recovered.reconcile(&mut backend).is_err());
        assert_eq!(backend.state.writes.get(), 0);
    }
}
