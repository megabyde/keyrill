//! Typed TSF calls preserving exact HRESULTs and initialized output storage.

use super::{Call, InterfaceError, Lease, returned_interface};
use crate::keys::Binding;
use std::ffi::c_void;
use windows::Win32::Foundation::{E_UNEXPECTED, S_OK};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::*;
use windows_core::{BOOL, GUID, HRESULT, Interface};

unsafe fn sdk_create_profiles(output: *mut *mut c_void) -> Call {
    // SAFETY: the typed caller retains all interfaces and buffers through this ABI call.
    windows_core::link!("ole32.dll" "system" fn CoCreateInstance(class: *const GUID, outer: *mut c_void, context: u32, id: *const GUID, output: *mut *mut c_void) -> HRESULT);
    Call {
        result: unsafe {
            CoCreateInstance(
                &CLSID_TF_InputProcessorProfiles,
                std::ptr::null_mut(),
                1,
                &ITfInputProcessorProfileMgr::IID,
                output,
            )
        },
    }
}

unsafe fn sdk_layouts(
    call: Option<unsafe extern "system" fn(i32, *mut isize) -> i32>,
    count: i32,
    layouts: *mut HKL,
    output: *mut i32,
) -> Call {
    // SAFETY: the typed caller retains all interfaces and buffers through this ABI call.
    windows_core::link!("user32.dll" "system" fn GetKeyboardLayoutList(count: i32, output: *mut HKL) -> i32);
    unsafe {
        *output = match call {
            Some(callback) => callback(count, layouts.cast()),
            None => GetKeyboardLayoutList(count, layouts),
        };
    }
    Call { result: S_OK }
}

pub(crate) fn advise_single(
    source: &Lease<ITfSourceSingle>,
    client: u32,
    provider: *mut c_void,
) -> Call {
    // SAFETY: source is retained and the caller retains its own generated provider interface.
    Call {
        result: unsafe {
            (super::vtable::<ITfSourceSingle>(pointer(source)).AdviseSingleSink)(
                pointer(source),
                client,
                &ITfFunctionProvider::IID,
                provider,
            )
        },
    }
}

pub(crate) fn unadvise_single(source: &Lease<ITfSourceSingle>, client: u32) {
    // SAFETY: source is retained through the one diagnostic-registration surrender.
    let _ = Call {
        result: unsafe {
            (super::vtable::<ITfSourceSingle>(pointer(source)).UnadviseSingleSink)(
                pointer(source),
                client,
                &ITfFunctionProvider::IID,
            )
        },
    };
}

fn pointer<T: Interface>(lease: &Lease<T>) -> *mut c_void {
    lease.owner.pointer.as_ptr()
}

pub(crate) fn acquired<T: Interface>(
    call: Call,
    output: *mut c_void,
) -> Result<Lease<T>, InterfaceError> {
    let output = returned_interface::<T>(call, output).value;
    if !call.exact() {
        return Err(InterfaceError::Native(call));
    }
    output.ok_or(InterfaceError::Missing)
}

pub(crate) fn advise(source: &Lease<ITfSource>, id: &GUID, sink: *mut c_void) -> (Call, u32) {
    let mut cookie = TF_INVALID_COOKIE;
    // SAFETY: the lease retains source, and the caller retains its own generated COM sink.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfSource>(pointer(source)).AdviseSink)(
                pointer(source),
                id,
                sink,
                &mut cookie,
            )
        },
    };
    (call, cookie)
}

pub(crate) fn unadvise(source: &Lease<ITfSource>, cookie: u32) -> Call {
    // SAFETY: source is retained while the captured registration is surrendered once.
    Call {
        result: unsafe {
            (super::vtable::<ITfSource>(pointer(source)).UnadviseSink)(pointer(source), cookie)
        },
    }
}

pub(crate) fn advise_keys(keys: &Lease<ITfKeystrokeMgr>, client: u32, sink: *mut c_void) -> Call {
    // SAFETY: the lease retains keys and the caller retains its own generated key sink.
    Call {
        result: unsafe {
            (super::vtable::<ITfKeystrokeMgr>(pointer(keys)).AdviseKeyEventSink)(
                pointer(keys),
                client,
                sink,
                BOOL(1),
            )
        },
    }
}

pub(crate) fn unadvise_keys(keys: &Lease<ITfKeystrokeMgr>, client: u32) -> Call {
    // SAFETY: keys is retained while the captured registration is surrendered once.
    Call {
        result: unsafe {
            (super::vtable::<ITfKeystrokeMgr>(pointer(keys)).UnadviseKeyEventSink)(
                pointer(keys),
                client,
            )
        },
    }
}

pub(crate) fn preserve(keys: &Lease<ITfKeystrokeMgr>, client: u32, binding: &Binding) -> Call {
    let description: Vec<u16> = binding.description.encode_utf16().collect();
    // SAFETY: the lease and immutable binding/output buffers remain live through the call.
    Call {
        result: unsafe {
            (super::vtable::<ITfKeystrokeMgr>(pointer(keys)).PreserveKey)(
                pointer(keys),
                client,
                &binding.id,
                &binding.key,
                windows_core::PCWSTR(description.as_ptr()),
                description.len() as u32,
            )
        },
    }
}

pub(crate) fn unpreserve(keys: &Lease<ITfKeystrokeMgr>, binding: &Binding) -> Call {
    // SAFETY: keys and this captured immutable binding remain live through the call.
    Call {
        result: unsafe {
            (super::vtable::<ITfKeystrokeMgr>(pointer(keys)).UnpreserveKey)(
                pointer(keys),
                &binding.id,
                &binding.key,
            )
        },
    }
}

pub(crate) fn create_profiles() -> Result<Lease<ITfInputProcessorProfileMgr>, InterfaceError> {
    let mut output = std::ptr::null_mut();
    // SAFETY: initialized output is distinct writable pointer storage.
    let call = unsafe { sdk_create_profiles(&mut output) };
    acquired(call, output)
}

pub(crate) fn active_profile(
    profiles: &Lease<ITfInputProcessorProfileMgr>,
) -> (Call, TF_INPUTPROCESSORPROFILE) {
    let mut output = TF_INPUTPROCESSORPROFILE::default();
    // SAFETY: the retained manager and initialized SDK output remain live through the call.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfInputProcessorProfileMgr>(pointer(profiles)).GetActiveProfile)(
                pointer(profiles),
                &GUID_TFCAT_TIP_KEYBOARD,
                &mut output,
            )
        },
    };
    (call, output)
}

pub(crate) fn substitute(
    profiles: &Lease<ITfInputProcessorProfileSubstituteLayout>,
    profile: &TF_INPUTPROCESSORPROFILE,
) -> (Call, HKL) {
    let mut output = HKL::default();
    // SAFETY: manager/profile are retained immutable input, and output is distinct writable storage.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfInputProcessorProfileSubstituteLayout>(pointer(profiles))
                .GetSubstituteKeyboardLayout)(
                pointer(profiles),
                &profile.clsid,
                profile.langid,
                &profile.guidProfile,
                &mut output,
            )
        },
    };
    (call, output)
}

pub(crate) fn layouts(
    callback: Option<unsafe extern "system" fn(i32, *mut isize) -> i32>,
    output: &mut [HKL],
) -> (Call, i32) {
    let mut count = 0;
    // SAFETY: output is a live bounded array and an injected callback is retained by the dependency owner.
    let call = unsafe {
        sdk_layouts(
            callback,
            output.len() as i32,
            output.as_mut_ptr(),
            &mut count,
        )
    };
    (call, count)
}

pub(crate) fn failure(call: Call) -> HRESULT {
    if call.result == S_OK {
        E_UNEXPECTED
    } else {
        call.result
    }
}

pub(crate) fn current_language(profiles: &Lease<ITfInputProcessorProfiles>) -> (Call, u16) {
    let mut language = 0;
    // SAFETY: the retained profiles interface receives distinct initialized scalar storage.
    let result = unsafe {
        (super::vtable::<ITfInputProcessorProfiles>(pointer(profiles)).GetCurrentLanguage)(
            pointer(profiles),
            &mut language,
        )
    };
    (Call { result }, language)
}

pub(crate) fn change_language(profiles: &Lease<ITfInputProcessorProfiles>, language: u16) -> Call {
    // SAFETY: profiles is retained throughout the bounded source-selection request.
    Call {
        result: unsafe {
            (super::vtable::<ITfInputProcessorProfiles>(pointer(profiles)).ChangeCurrentLanguage)(
                pointer(profiles),
                language,
            )
        },
    }
}

pub(crate) fn profile_enabled(
    profiles: &Lease<ITfInputProcessorProfiles>,
    profile: &crate::profiles::Profile,
) -> (Call, BOOL) {
    let mut enabled = BOOL(0);
    // SAFETY: all profile identity inputs are immutable and output is initialized writable storage.
    let result = unsafe {
        (super::vtable::<ITfInputProcessorProfiles>(pointer(profiles)).IsEnabledLanguageProfile)(
            pointer(profiles),
            &crate::profiles::SERVICE_ID,
            profile.language,
            &profile.id,
            &mut enabled,
        )
    };
    (Call { result }, enabled)
}

pub(crate) fn target_profile(
    profiles: &Lease<ITfInputProcessorProfileMgr>,
    profile: &crate::profiles::Profile,
) -> (Call, TF_INPUTPROCESSORPROFILE) {
    let mut output = TF_INPUTPROCESSORPROFILE::default();
    // SAFETY: the retained manager and exact static identity live through this output query.
    let result = unsafe {
        (super::vtable::<ITfInputProcessorProfileMgr>(pointer(profiles)).GetProfile)(
            pointer(profiles),
            TF_PROFILETYPE_INPUTPROCESSOR,
            profile.language,
            &crate::profiles::SERVICE_ID,
            &profile.id,
            HKL::default(),
            &mut output,
        )
    };
    (Call { result }, output)
}

pub(crate) fn activate_profile(
    profiles: &Lease<ITfInputProcessorProfileMgr>,
    profile: &crate::profiles::Profile,
) -> Call {
    // SAFETY: the retained manager and immutable static target live through this flags-zero request.
    Call {
        result: unsafe {
            (super::vtable::<ITfInputProcessorProfileMgr>(pointer(profiles)).ActivateProfile)(
                pointer(profiles),
                TF_PROFILETYPE_INPUTPROCESSOR,
                profile.language,
                &crate::profiles::SERVICE_ID,
                &profile.id,
                HKL::default(),
                0,
            )
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use windows::Win32::Foundation::{E_FAIL, E_NOINTERFACE, S_FALSE};
    use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
    use windows_core::{BOOL, IUnknown, Ref, Result, implement};

    #[implement(IClassFactory, Agile = false)]
    struct Controlled {
        retired: Arc<AtomicUsize>,
    }

    impl Drop for Controlled {
        fn drop(&mut self) {
            self.retired.fetch_add(1, Ordering::AcqRel);
        }
    }

    impl IClassFactory_Impl for Controlled_Impl {
        fn CreateInstance(
            &self,
            _: Ref<'_, IUnknown>,
            _: *const GUID,
            output: *mut *mut c_void,
        ) -> Result<()> {
            if !output.is_null() {
                // SAFETY: the SDK contract supplies a writable interface output.
                unsafe { output.write(std::ptr::null_mut()) };
            }
            E_NOINTERFACE.ok()
        }

        fn LockServer(&self, _: BOOL) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn refused_owned_outputs_retire_and_exact_null_stays_missing() {
        for result in [S_FALSE, E_FAIL] {
            let retired = Arc::new(AtomicUsize::new(0));
            let object = windows_core::ComObject::new(Controlled {
                retired: Arc::clone(&retired),
            })
            .to_interface::<IUnknown>();
            let output = acquired::<IUnknown>(Call { result }, object.into_raw());
            let Err(InterfaceError::Native(call)) = output else {
                panic!("nonexact status granted an interface");
            };
            assert_eq!(call.result, result);
            assert_eq!(retired.load(Ordering::Acquire), 1);
        }
        assert!(matches!(
            acquired::<IUnknown>(Call { result: S_OK }, std::ptr::null_mut()),
            Err(InterfaceError::Missing)
        ));
    }
}
