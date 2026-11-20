use crate::diagnostics::{self, RuntimeSnapshot, Snapshot, SnapshotV2};
use crate::service::{LOCKS, OBJECTS, ObjectLease, Service, Sinks, WORKERS};
use std::cell::Cell;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_NOINTERFACE, E_POINTER, E_UNEXPECTED,
    LPARAM, S_FALSE, S_OK, WPARAM,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfDocumentMgr,
    ITfInputProcessorProfileActivationSink, ITfInputProcessorProfileActivationSink_Impl,
    ITfThreadMgrEventSink, ITfThreadMgrEventSink_Impl, TF_INPUTPROCESSORPROFILE,
    TF_IPP_FLAG_ACTIVE, TF_IPSINK_FLAG_ACTIVE,
};
use windows::Win32::UI::TextServices::{
    ITfFnConfigure, ITfFnConfigure_Vtbl, ITfFunction, ITfFunction_Vtbl, ITfFunctionProvider,
    ITfFunctionProvider_Vtbl, ITfKeyEventSink, ITfKeyEventSink_Vtbl, ITfTextInputProcessor_Impl,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl, ITfThreadMgr,
};
use windows_core::{
    BOOL, GUID, HRESULT, IUnknown, IUnknown_Vtbl, IUnknownImpl, Interface, Ref, Result, implement,
    interface,
};

pub(crate) use crate::profiles::SERVICE_ID;

// Raw mirrors retain the SDK's output-pointer checks and exact HRESULT semantics.
#[interface("aa80e7f5-2021-11d2-93e0-0060b067b86e")]
unsafe trait RawKeyEventSink: IUnknown {
    fn OnSetFocus(&self, foreground: BOOL) -> HRESULT;
    fn OnTestKeyDown(
        &self,
        context: *mut c_void,
        key: WPARAM,
        flags: LPARAM,
        eaten: *mut BOOL,
    ) -> HRESULT;
    fn OnTestKeyUp(
        &self,
        context: *mut c_void,
        key: WPARAM,
        flags: LPARAM,
        eaten: *mut BOOL,
    ) -> HRESULT;
    fn OnKeyDown(
        &self,
        context: *mut c_void,
        key: WPARAM,
        flags: LPARAM,
        eaten: *mut BOOL,
    ) -> HRESULT;
    fn OnKeyUp(
        &self,
        context: *mut c_void,
        key: WPARAM,
        flags: LPARAM,
        eaten: *mut BOOL,
    ) -> HRESULT;
    fn OnPreservedKey(&self, context: *mut c_void, id: *const GUID, eaten: *mut BOOL) -> HRESULT;
}

#[interface("101d6610-0990-11d3-8df0-00105a2799b5")]
unsafe trait RawFunctionProvider: IUnknown {
    fn GetType(&self, output: *mut GUID) -> HRESULT;
    fn GetDescription(&self, output: *mut *mut c_void) -> HRESULT;
    fn GetFunction(
        &self,
        function: *const GUID,
        id: *const GUID,
        output: *mut *mut c_void,
    ) -> HRESULT;
}

#[interface("db593490-098f-11d3-8df0-00105a2799b5")]
unsafe trait RawFunction: IUnknown {
    fn GetDisplayName(&self, output: *mut *mut c_void) -> HRESULT;
}

#[interface("88f567c6-1757-49f8-a1b2-89234c1eeff9")]
unsafe trait RawConfigure: RawFunction {
    fn Show(
        &self,
        parent: windows::Win32::Foundation::HWND,
        language: u16,
        profile: *const GUID,
    ) -> HRESULT;
}

#[interface("f75f909b-293a-4a7b-a172-f5a061880352")]
unsafe trait RawSnapshotReader: IUnknown {
    fn Copy(&self, size: u32, version: u32, output: *mut Snapshot) -> HRESULT;
}
#[interface("4f89429e-a955-4885-bb33-c78ad1982481")]
unsafe trait RawSnapshotReaderV2: IUnknown {
    fn Copy(&self, size: u32, version: u32, output: *mut SnapshotV2) -> HRESULT;
}
#[interface("cda9ed25-1dbc-4fb6-b2a5-bc132945e082")]
unsafe trait RawRuntimeReader: IUnknown {
    fn Copy(&self, size: u32, version: u32, output: *mut RuntimeSnapshot) -> HRESULT;
}

const _: () = {
    assert!(RawKeyEventSink::IID.to_u128() == ITfKeyEventSink::IID.to_u128());
    assert!(RawFunctionProvider::IID.to_u128() == ITfFunctionProvider::IID.to_u128());
    assert!(size_of::<RawKeyEventSink_Vtbl>() == size_of::<ITfKeyEventSink_Vtbl>());
    assert!(align_of::<RawKeyEventSink_Vtbl>() == align_of::<ITfKeyEventSink_Vtbl>());
    assert!(size_of::<RawFunctionProvider_Vtbl>() == size_of::<ITfFunctionProvider_Vtbl>());
    assert!(align_of::<RawFunctionProvider_Vtbl>() == align_of::<ITfFunctionProvider_Vtbl>());
    assert!(
        std::mem::offset_of!(RawKeyEventSink_Vtbl, base__)
            == std::mem::offset_of!(ITfKeyEventSink_Vtbl, base__)
    );
    assert!(
        std::mem::offset_of!(RawFunctionProvider_Vtbl, base__)
            == std::mem::offset_of!(ITfFunctionProvider_Vtbl, base__)
    );
};

macro_rules! assert_method {
    ($raw:ty, $sdk:ty, $field:ident) => {
        const _: () =
            assert!(std::mem::offset_of!($raw, $field) == std::mem::offset_of!($sdk, $field));
        const _: fn(&$raw, &$sdk) = |raw, sdk| {
            let mut signature = raw.$field;
            let _ = signature;
            signature = sdk.$field;
            let _ = signature;
        };
    };
}
assert_method!(RawKeyEventSink_Vtbl, ITfKeyEventSink_Vtbl, OnSetFocus);
assert_method!(RawKeyEventSink_Vtbl, ITfKeyEventSink_Vtbl, OnTestKeyDown);
assert_method!(RawKeyEventSink_Vtbl, ITfKeyEventSink_Vtbl, OnTestKeyUp);
assert_method!(RawKeyEventSink_Vtbl, ITfKeyEventSink_Vtbl, OnKeyDown);
assert_method!(RawKeyEventSink_Vtbl, ITfKeyEventSink_Vtbl, OnKeyUp);
assert_method!(RawKeyEventSink_Vtbl, ITfKeyEventSink_Vtbl, OnPreservedKey);
assert_method!(RawFunctionProvider_Vtbl, ITfFunctionProvider_Vtbl, GetType);
assert_method!(
    RawFunctionProvider_Vtbl,
    ITfFunctionProvider_Vtbl,
    GetDescription
);
assert_method!(
    RawFunctionProvider_Vtbl,
    ITfFunctionProvider_Vtbl,
    GetFunction
);

#[implement(
    ITfTextInputProcessorEx,
    RawKeyEventSink,
    RawFunctionProvider,
    RawConfigure,
    RawSnapshotReader,
    RawSnapshotReaderV2,
    RawRuntimeReader,
    ITfThreadMgrEventSink,
    ITfInputProcessorProfileActivationSink,
    ITfCompositionSink,
    Agile = false
)]
struct ServiceCom {
    service: Rc<Service>,
    configuring: Cell<bool>,
}

pub(crate) fn create(service: Rc<Service>) -> ITfTextInputProcessorEx {
    create_owned(service).processor()
}

struct OwnInterfaces {
    processor: ITfTextInputProcessorEx,
    composition: *mut c_void,
}

#[derive(Clone)]
pub(crate) struct OwnServiceLease(Rc<OwnInterfaces>);

pub(crate) fn create_owned(service: Rc<Service>) -> OwnServiceLease {
    let object = windows_core::ComObject::new(ServiceCom {
        service,
        configuring: Cell::new(false),
    });
    let processor = object.to_interface::<ITfTextInputProcessorEx>();
    let composition = object.as_interface::<ITfCompositionSink>().as_raw();
    OwnServiceLease(Rc::new(OwnInterfaces {
        processor,
        composition,
    }))
}

impl OwnServiceLease {
    pub(crate) fn processor(&self) -> ITfTextInputProcessorEx {
        self.0.processor.clone()
    }
    pub(crate) fn composition_sink(&self) -> *mut c_void {
        self.0.composition
    }
}

impl ServiceCom_Impl {
    fn release_key(
        &self,
        context: *mut c_void,
        key: WPARAM,
        flags: LPARAM,
        eaten: *mut BOOL,
        delivery: diagnostics::ReleaseDelivery,
    ) -> HRESULT {
        if eaten.is_null() {
            return E_POINTER;
        }
        // SAFETY: each SDK key-release method supplies one writable BOOL output.
        unsafe { eaten.write(BOOL(0)) };
        if !self.service.owner_thread() {
            return S_OK;
        }
        // SAFETY: the SDK retains context/output throughout this synchronous callback.
        unsafe {
            self.service
                .release_alt(self.owner_lease(), context, key.0, flags.0, eaten, delivery)
        }
    }

    fn owner_lease(&self) -> OwnServiceLease {
        OwnServiceLease(Rc::new(OwnInterfaces {
            processor: self.as_interface::<ITfTextInputProcessorEx>().to_owned(),
            composition: self.as_interface::<ITfCompositionSink>().as_raw(),
        }))
    }
}

impl ITfTextInputProcessor_Impl for ServiceCom_Impl {
    fn Activate(&self, manager: Ref<'_, ITfThreadMgr>, client: u32) -> Result<()> {
        self.activate(manager, client, 0).ok()
    }

    fn Deactivate(&self) -> Result<()> {
        self.service.deactivate().ok()
    }
}

impl ITfTextInputProcessorEx_Impl for ServiceCom_Impl {
    fn ActivateEx(&self, manager: Ref<'_, ITfThreadMgr>, client: u32, flags: u32) -> Result<()> {
        self.activate(manager, client, flags).ok()
    }
}

impl ServiceCom_Impl {
    fn activate(&self, manager: Ref<'_, ITfThreadMgr>, client: u32, flags: u32) -> HRESULT {
        if !self.service.owner_thread() {
            return windows::Win32::Foundation::RPC_E_WRONG_THREAD;
        }
        let _keep = self.to_interface::<ITfTextInputProcessorEx>();
        let sinks = Sinks {
            thread: self.as_interface::<ITfThreadMgrEventSink>().as_raw(),
            profile: self
                .as_interface::<ITfInputProcessorProfileActivationSink>()
                .as_raw(),
            keys: self.as_interface::<RawKeyEventSink>().as_raw(),
            provider: self.as_interface::<RawFunctionProvider>().as_raw(),
        };
        self.service.activate(
            manager
                .as_ref()
                .map_or(std::ptr::null_mut(), Interface::as_raw),
            client,
            flags,
            sinks,
        )
    }
}

impl ITfThreadMgrEventSink_Impl for ServiceCom_Impl {
    fn OnInitDocumentMgr(&self, _: Ref<'_, ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }
    fn OnUninitDocumentMgr(&self, _: Ref<'_, ITfDocumentMgr>) -> Result<()> {
        self.service
            .checked(|| {
                self.service
                    .invalidate_for_lifecycle(diagnostics::LifecycleCause::DocumentUninit);
                S_OK
            })
            .ok()
    }
    fn OnSetFocus(&self, _: Ref<'_, ITfDocumentMgr>, _: Ref<'_, ITfDocumentMgr>) -> Result<()> {
        self.service
            .checked(|| {
                self.service
                    .invalidate_for_lifecycle(diagnostics::LifecycleCause::DocumentFocus);
                S_OK
            })
            .ok()
    }
    fn OnPushContext(
        &self,
        _: Ref<'_, windows::Win32::UI::TextServices::ITfContext>,
    ) -> Result<()> {
        self.service
            .checked(|| {
                self.service
                    .invalidate_for_lifecycle(diagnostics::LifecycleCause::ContextPush);
                S_OK
            })
            .ok()
    }
    fn OnPopContext(&self, _: Ref<'_, windows::Win32::UI::TextServices::ITfContext>) -> Result<()> {
        self.service
            .checked(|| {
                self.service
                    .invalidate_for_lifecycle(diagnostics::LifecycleCause::ContextPop);
                S_OK
            })
            .ok()
    }
}

impl ITfInputProcessorProfileActivationSink_Impl for ServiceCom_Impl {
    fn OnActivated(
        &self,
        kind: u32,
        language: u16,
        service: *const GUID,
        category: *const GUID,
        profile: *const GUID,
        layout: HKL,
        flags: u32,
    ) -> Result<()> {
        if service.is_null() || profile.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: the SDK callback supplies readable GUID references for its duration.
        unsafe {
            self.service.profile_changed(TF_INPUTPROCESSORPROFILE {
                dwProfileType: kind,
                langid: language,
                clsid: *service,
                catid: if category.is_null() {
                    GUID::zeroed()
                } else {
                    *category
                },
                guidProfile: *profile,
                hkl: layout,
                dwFlags: if flags & TF_IPSINK_FLAG_ACTIVE != 0 {
                    TF_IPP_FLAG_ACTIVE
                } else {
                    0
                },
                ..Default::default()
            })
        }
        .ok()
    }
}

impl ITfCompositionSink_Impl for ServiceCom_Impl {
    fn OnCompositionTerminated(&self, _: u32, composition: Ref<'_, ITfComposition>) -> Result<()> {
        self.service
            .terminated(
                composition
                    .as_ref()
                    .map_or(std::ptr::null_mut(), Interface::as_raw),
            )
            .ok()
    }
}

impl RawKeyEventSink_Impl for ServiceCom_Impl {
    unsafe fn OnSetFocus(&self, foreground: BOOL) -> HRESULT {
        self.service.checked(|| {
            self.service.revoke_release();
            if !foreground.as_bool() {
                self.service
                    .invalidate_for_lifecycle(diagnostics::LifecycleCause::ForegroundLost);
            }
            S_OK
        })
    }

    unsafe fn OnTestKeyDown(
        &self,
        _: *mut c_void,
        _: WPARAM,
        _: LPARAM,
        eaten: *mut BOOL,
    ) -> HRESULT {
        if eaten.is_null() {
            return E_POINTER;
        }
        // SAFETY: the SDK caller supplies one writable BOOL output.
        unsafe { eaten.write(BOOL(0)) };
        self.service.checked(|| {
            // SAFETY: the output remains writable after the owner-thread admission check.
            unsafe { eaten.write(BOOL(self.service.interested() as i32)) };
            S_OK
        })
    }

    unsafe fn OnTestKeyUp(
        &self,
        context: *mut c_void,
        key: WPARAM,
        flags: LPARAM,
        eaten: *mut BOOL,
    ) -> HRESULT {
        self.release_key(
            context,
            key,
            flags,
            eaten,
            diagnostics::ReleaseDelivery::Test,
        )
    }

    unsafe fn OnKeyDown(
        &self,
        context: *mut c_void,
        key: WPARAM,
        flags: LPARAM,
        eaten: *mut BOOL,
    ) -> HRESULT {
        if eaten.is_null() {
            return E_POINTER;
        }
        // SAFETY: the SDK supplies one writable output, defaulted before fallible work.
        unsafe { eaten.write(BOOL(0)) };
        if !self.service.owner_thread() {
            return windows::Win32::Foundation::RPC_E_WRONG_THREAD;
        }
        // SAFETY: the SDK keeps context/output live through this synchronous entry.
        unsafe {
            self.service
                .key_down(self.owner_lease(), context, key, flags, eaten)
        }
    }

    unsafe fn OnKeyUp(
        &self,
        context: *mut c_void,
        key: WPARAM,
        flags: LPARAM,
        eaten: *mut BOOL,
    ) -> HRESULT {
        self.release_key(
            context,
            key,
            flags,
            eaten,
            diagnostics::ReleaseDelivery::Actual,
        )
    }

    unsafe fn OnPreservedKey(
        &self,
        context: *mut c_void,
        id: *const GUID,
        eaten: *mut BOOL,
    ) -> HRESULT {
        if eaten.is_null() {
            return E_POINTER;
        }
        // SAFETY: the SDK caller supplies a writable BOOL, initialized before fallible work.
        unsafe { eaten.write(BOOL(0)) };
        if !self.service.owner_thread() {
            return windows::Win32::Foundation::RPC_E_WRONG_THREAD;
        }
        if id.is_null() {
            return E_POINTER;
        }
        // SAFETY: the SDK retains context and both readable GUID/writable BOOL slots through entry.
        unsafe {
            self.service
                .preserved(self.owner_lease(), context, id.read(), eaten)
        }
    }
}

impl RawFunctionProvider_Impl for ServiceCom_Impl {
    unsafe fn GetType(&self, output: *mut GUID) -> HRESULT {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: the COM caller supplies one writable GUID, initialized before fallible work.
        unsafe { output.write(SERVICE_ID) };
        fence(|| S_OK)
    }

    unsafe fn GetDescription(&self, output: *mut *mut c_void) -> HRESULT {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: the COM caller supplies a writable pointer slot.
        unsafe { output.write(std::ptr::null_mut()) };
        fence(|| {
            let literal = windows_core::w!("Keyrill proof service diagnostics");
            // SAFETY: w! provides a static, zero-terminated UTF-16 string.
            let description =
                unsafe { windows::Win32::Foundation::SysAllocStringLen(Some(literal.as_wide())) };
            if description.is_empty() {
                return windows::Win32::Foundation::E_OUTOFMEMORY;
            }
            // SAFETY: ownership of the allocated BSTR is transferred to the caller.
            unsafe { output.write(description.into_raw().cast_mut().cast()) };
            S_OK
        })
    }

    unsafe fn GetFunction(
        &self,
        function: *const GUID,
        id: *const GUID,
        output: *mut *mut c_void,
    ) -> HRESULT {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: the COM caller supplies a writable pointer slot.
        unsafe { output.write(std::ptr::null_mut()) };
        if function.is_null() || id.is_null() {
            return E_POINTER;
        }
        fence(|| {
            // SAFETY: the SDK caller supplies two readable GUID references.
            let (function, id) = unsafe { (*function, *id) };
            if function != GUID::zeroed() {
                return E_NOINTERFACE;
            }
            let interface = if id == ITfFnConfigure::IID || id == ITfFunction::IID {
                self.as_interface::<RawConfigure>().to_owned().into_raw()
            } else if id == diagnostics::SNAPSHOT_IID {
                self.as_interface::<RawSnapshotReader>()
                    .to_owned()
                    .into_raw()
            } else if id == diagnostics::SNAPSHOT_V2_IID {
                self.as_interface::<RawSnapshotReaderV2>()
                    .to_owned()
                    .into_raw()
            } else if id == diagnostics::RUNTIME_IID {
                self.as_interface::<RawRuntimeReader>()
                    .to_owned()
                    .into_raw()
            } else {
                return E_NOINTERFACE;
            };
            // SAFETY: transfer one reference to our own generated interface through the checked output.
            unsafe { output.write(interface) };
            S_OK
        })
    }
}

const _: () = {
    assert!(RawFunction::IID.to_u128() == ITfFunction::IID.to_u128());
    assert!(RawConfigure::IID.to_u128() == ITfFnConfigure::IID.to_u128());
    assert!(size_of::<RawFunction_Vtbl>() == size_of::<ITfFunction_Vtbl>());
    assert!(align_of::<RawFunction_Vtbl>() == align_of::<ITfFunction_Vtbl>());
    assert!(size_of::<RawConfigure_Vtbl>() == size_of::<ITfFnConfigure_Vtbl>());
    assert!(align_of::<RawConfigure_Vtbl>() == align_of::<ITfFnConfigure_Vtbl>());
    assert!(
        std::mem::offset_of!(RawFunction_Vtbl, base__)
            == std::mem::offset_of!(ITfFunction_Vtbl, base__)
    );
    assert!(
        std::mem::offset_of!(RawConfigure_Vtbl, base__)
            == std::mem::offset_of!(ITfFnConfigure_Vtbl, base__)
    );
};
assert_method!(RawFunction_Vtbl, ITfFunction_Vtbl, GetDisplayName);
assert_method!(RawConfigure_Vtbl, ITfFnConfigure_Vtbl, Show);

impl RawFunction_Impl for ServiceCom_Impl {
    unsafe fn GetDisplayName(&self, output: *mut *mut c_void) -> HRESULT {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: the caller provides one writable BSTR output slot.
        unsafe { output.write(std::ptr::null_mut()) };
        if !self.service.owner_thread() {
            return windows::Win32::Foundation::RPC_E_WRONG_THREAD;
        }
        fence(|| {
            let literal = windows_core::w!("Keyrill Settings");
            // SAFETY: the static literal supplies the exact UTF-16 allocation span.
            let name =
                unsafe { windows::Win32::Foundation::SysAllocStringLen(Some(literal.as_wide())) };
            if name.is_empty() {
                return windows::Win32::Foundation::E_OUTOFMEMORY;
            }
            // SAFETY: transfer the allocated BSTR to the checked SDK output.
            unsafe { output.write(name.into_raw().cast_mut().cast()) };
            S_OK
        })
    }
}

impl RawConfigure_Impl for ServiceCom_Impl {
    unsafe fn Show(
        &self,
        parent: windows::Win32::Foundation::HWND,
        language: u16,
        profile: *const GUID,
    ) -> HRESULT {
        if !self.service.owner_thread() {
            return windows::Win32::Foundation::RPC_E_WRONG_THREAD;
        }
        if profile.is_null() {
            return E_POINTER;
        }
        // SAFETY: the SDK caller lends one readable profile GUID for this call.
        let profile = unsafe { profile.read() };
        if !crate::profiles::PROFILES
            .iter()
            .any(|p| p.language == language && p.id == profile)
        {
            return windows::Win32::Foundation::E_INVALIDARG;
        }
        if self.configuring.replace(true) {
            return windows::Win32::Foundation::RPC_E_CALL_REJECTED;
        }
        struct Configuring<'a>(&'a Cell<bool>);
        impl Drop for Configuring<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _gate = Configuring(&self.configuring);
        let _keep = self.to_interface::<ITfTextInputProcessorEx>();
        fence(|| match crate::configure::show(parent) {
            Ok(()) => S_OK,
            Err(error) => error.code(),
        })
    }
}

fn copy_snapshot<T>(
    service: &Service,
    size: u32,
    version: u32,
    expected: u32,
    output: *mut T,
    read: impl FnOnce() -> std::result::Result<T, HRESULT>,
) -> HRESULT {
    if output.is_null() {
        return E_POINTER;
    }
    // SAFETY: Copy's caller supplies at least size writable bytes. Only the bounded prefix is cleared.
    unsafe { std::ptr::write_bytes(output.cast::<u8>(), 0, (size as usize).min(size_of::<T>())) };
    if !service.owner_thread() {
        return windows::Win32::Foundation::RPC_E_WRONG_THREAD;
    }
    if size as usize != size_of::<T>() || version != expected {
        return windows::Win32::Foundation::E_INVALIDARG;
    }
    service.entry(|| match read() {
        Ok(value) => {
            // SAFETY: exact size/version validate one complete output object.
            unsafe { output.write(value) };
            S_OK
        }
        Err(error) => error,
    })
}

impl RawSnapshotReader_Impl for ServiceCom_Impl {
    unsafe fn Copy(&self, size: u32, version: u32, output: *mut Snapshot) -> HRESULT {
        copy_snapshot(
            &self.service,
            size,
            version,
            diagnostics::VERSION,
            output,
            || self.service.copy_snapshot(),
        )
    }
}
impl RawSnapshotReaderV2_Impl for ServiceCom_Impl {
    unsafe fn Copy(&self, size: u32, version: u32, output: *mut SnapshotV2) -> HRESULT {
        copy_snapshot(
            &self.service,
            size,
            version,
            diagnostics::VERSION_V2,
            output,
            || self.service.copy_snapshot_v2(),
        )
    }
}
impl RawRuntimeReader_Impl for ServiceCom_Impl {
    unsafe fn Copy(&self, size: u32, version: u32, output: *mut RuntimeSnapshot) -> HRESULT {
        copy_snapshot(
            &self.service,
            size,
            version,
            diagnostics::RUNTIME_VERSION,
            output,
            || self.service.copy_runtime(),
        )
    }
}

#[implement(IClassFactory, Agile = false)]
struct Factory {
    _object: ObjectLease,
}

fn fence(call: impl FnOnce() -> HRESULT) -> HRESULT {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or(E_UNEXPECTED)
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<'_, IUnknown>,
        id: *const GUID,
        output: *mut *mut c_void,
    ) -> Result<()> {
        fence(|| {
            if output.is_null() {
                return E_POINTER;
            }
            // SAFETY: COM supplies a writable output slot before any object creation.
            unsafe { output.write(std::ptr::null_mut()) };
            if id.is_null() {
                return E_POINTER;
            }
            if !outer.is_null() {
                return CLASS_E_NOAGGREGATION;
            }
            let service = create(Service::new());
            // SAFETY: this is our generated object, with a valid IID and writable output slot.
            unsafe { service.query(id, output) }
        })
        .ok()
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        fence(|| {
            if lock.as_bool() {
                LOCKS.fetch_add(1, Ordering::AcqRel);
            } else {
                let _ = LOCKS.try_update(Ordering::AcqRel, Ordering::Acquire, |locks| {
                    locks.checked_sub(1)
                });
            }
            S_OK
        })
        .ok()
    }
}

/// Return a class factory for the Windows TSF service.
///
/// # Safety
/// GUID pointers must identify readable GUIDs and `output` must identify a writable pointer slot.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    id: *const GUID,
    output: *mut *mut c_void,
) -> HRESULT {
    fence(|| {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies a writable pointer slot.
        unsafe { output.write(std::ptr::null_mut()) };
        if class.is_null() || id.is_null() {
            return E_POINTER;
        }
        // SAFETY: both GUID pointers are live readable values under the export contract.
        if unsafe { *class } != SERVICE_ID {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory {
            _object: ObjectLease::new(),
        }
        .into();
        // SAFETY: the factory is our generated object; id/output were checked above.
        unsafe { factory.query(id, output) }
    })
}

/// Report whether COM objects, workers and server locks have drained.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTS.load(Ordering::Acquire) == 0
        && LOCKS.load(Ordering::Acquire) == 0
        && WORKERS.load(Ordering::Acquire) == 0
    {
        S_OK
    } else {
        S_FALSE
    }
}

#[cfg(test)]
#[path = "configure_com_tests.rs"]
mod configure_tests;
