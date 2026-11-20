use super::*;
use windows::Win32::Foundation::{E_INVALIDARG, HWND, RPC_E_CALL_REJECTED, RPC_E_WRONG_THREAD};
use windows_core::BSTR;

fn factory_configure() -> ITfFnConfigure {
    let mut raw = std::ptr::null_mut();
    // SAFETY: exact SDK GUID inputs and writable interface output survive both native entries.
    unsafe {
        assert_eq!(
            DllGetClassObject(&SERVICE_ID, &IClassFactory::IID, &mut raw),
            S_OK
        );
        let factory = IClassFactory::from_raw(raw);
        raw = std::ptr::null_mut();
        assert_eq!(
            (factory.vtable().CreateInstance)(
                factory.as_raw(),
                std::ptr::null_mut(),
                &ITfFnConfigure::IID,
                &mut raw
            ),
            S_OK
        );
        ITfFnConfigure::from_raw(raw)
    }
}

#[test]
fn configure_sdk_factory_inheritance_and_provider_preserve_identity_and_reference_ownership() {
    let configure = factory_configure();
    let function = configure.cast::<ITfFunction>().unwrap();
    let provider = configure.cast::<ITfFunctionProvider>().unwrap();
    let identity = configure.cast::<IUnknown>().unwrap();
    assert_eq!(function.cast::<IUnknown>().unwrap(), identity);
    let mut output = std::ptr::dangling_mut();
    // SAFETY: all SDK interface pointers, GUIDs and outputs are retained for each call.
    unsafe {
        assert_eq!(
            (configure.vtable().base__.GetDisplayName)(configure.as_raw(), std::ptr::null_mut()),
            E_POINTER
        );
        assert_eq!(
            (function.vtable().GetDisplayName)(function.as_raw(), &mut output),
            S_OK
        );
        assert_eq!(
            BSTR::from_raw(output.cast()),
            BSTR::from("Keyrill Settings")
        );
        output = std::ptr::null_mut();
        assert_eq!(
            (provider.vtable().GetFunction)(
                provider.as_raw(),
                &GUID::zeroed(),
                &ITfFnConfigure::IID,
                &mut output
            ),
            S_OK
        );
        let returned = ITfFnConfigure::from_raw(output);
        drop(configure);
        drop(function);
        drop(provider);
        assert_eq!(returned.cast::<IUnknown>().unwrap(), identity);
        assert_eq!(
            returned.GetDisplayName().unwrap(),
            BSTR::from("Keyrill Settings")
        );
    }
}

#[test]
fn configure_sdk_rejects_bad_profiles_before_unavailable_package_and_initializes_outputs() {
    let configure = factory_configure();
    let provider = configure.cast::<ITfFunctionProvider>().unwrap();
    let profile = &crate::profiles::PROFILES[0];
    // SAFETY: every non-null GUID is a live input and every non-null output is writable.
    unsafe {
        assert_eq!(
            (configure.vtable().Show)(
                configure.as_raw(),
                HWND::default(),
                profile.language,
                std::ptr::null()
            ),
            E_POINTER
        );
        assert_eq!(
            (configure.vtable().Show)(
                configure.as_raw(),
                HWND::default(),
                profile.language ^ 1,
                &profile.id
            ),
            E_INVALIDARG
        );
        assert_eq!(
            (configure.vtable().Show)(
                configure.as_raw(),
                HWND::default(),
                profile.language,
                &GUID::zeroed()
            ),
            E_INVALIDARG
        );
        assert_eq!(
            (configure.vtable().Show)(
                configure.as_raw(),
                HWND(std::ptr::dangling_mut()),
                profile.language,
                &profile.id
            ),
            E_INVALIDARG
        );
        assert_eq!(
            (configure.vtable().Show)(
                configure.as_raw(),
                HWND::default(),
                profile.language,
                &profile.id
            ),
            HRESULT::from_win32(windows::Win32::Foundation::ERROR_NOT_SUPPORTED.0)
        );
        let mut output = std::ptr::dangling_mut();
        assert_eq!(
            (provider.vtable().GetFunction)(
                provider.as_raw(),
                &SERVICE_ID,
                &ITfFnConfigure::IID,
                &mut output
            ),
            E_NOINTERFACE
        );
        assert!(output.is_null());
        assert_eq!(
            (provider.vtable().GetFunction)(
                provider.as_raw(),
                &GUID::zeroed(),
                &ITfFnConfigure::IID,
                std::ptr::null_mut()
            ),
            E_POINTER
        );
    }
}

#[test]
fn configure_sdk_checks_thread_reentry_and_panic_fence() {
    let object = windows_core::ComObject::new(ServiceCom {
        service: Service::new(),
        configuring: Cell::new(true),
    });
    let configure = object.to_interface::<RawConfigure>();
    let profile = &crate::profiles::PROFILES[0];
    // SAFETY: retained SDK-compatible interface and profile survive the call.
    assert_eq!(
        unsafe {
            (configure.vtable().Show)(
                configure.as_raw(),
                HWND::default(),
                profile.language,
                &profile.id,
            )
        },
        RPC_E_CALL_REJECTED
    );
    let address = configure.as_raw() as usize;
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                // SAFETY: the owner retains this object until the scoped wrong-thread probe joins.
                unsafe {
                    let raw = address as *mut c_void;
                    let table = &**raw.cast::<*const ITfFnConfigure_Vtbl>();
                    let mut output = std::ptr::dangling_mut();
                    assert_eq!(
                        (table.base__.GetDisplayName)(raw, &mut output),
                        RPC_E_WRONG_THREAD
                    );
                    assert!(output.is_null());
                    assert_eq!(
                        (table.Show)(raw, HWND::default(), profile.language, &profile.id),
                        RPC_E_WRONG_THREAD
                    );
                }
            })
            .join()
            .unwrap();
    });
    assert_eq!(
        fence(|| panic!("controlled Configure panic fence")),
        E_UNEXPECTED
    );
}
