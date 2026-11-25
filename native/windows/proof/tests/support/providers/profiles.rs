use super::*;

pub(super) const ITFINPUTPROCESSORPROFILEMGR_TABLE: ITfInputProcessorProfileMgr_Vtbl =
    ITfInputProcessorProfileMgr_Vtbl {
        base__: UNKNOWN,
        ActivateProfile: ITfInputProcessorProfileMgr_ActivateProfile,
        DeactivateProfile: ITfInputProcessorProfileMgr_DeactivateProfile,
        GetProfile: ITfInputProcessorProfileMgr_GetProfile,
        EnumProfiles: ITfInputProcessorProfileMgr_EnumProfiles,
        ReleaseInputProcessor: ITfInputProcessorProfileMgr_ReleaseInputProcessor,
        RegisterProfile: ITfInputProcessorProfileMgr_RegisterProfile,
        UnregisterProfile: ITfInputProcessorProfileMgr_UnregisterProfile,
        GetActiveProfile: ITfInputProcessorProfileMgr_GetActiveProfile,
    };
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputProcessorProfileMgr_ActivateProfile(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: *const windows_core::GUID,
    a5: windows::Win32::UI::Input::KeyboardAndMouse::HKL,
    a6: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            source::fixed(a1, a2, a3, a4, a5, a6);
            let state = p.profile();
            assert_eq!(state.language.get(), a2);
            state.activation_calls.set(state.activation_calls.get() + 1);
            if state.activation_effect.get() {
                state.profile.set(source_profile((a2 == 0x419) as usize));
                let language = a2 as usize;
                state
                    .substitute
                    .set(HKL((language << 16 | language) as *mut _));
            }
            p.hit("profile.activate");
            p.result("ActivateProfile")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputProcessorProfileMgr_DeactivateProfile(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: *const windows_core::GUID,
    a5: windows::Win32::UI::Input::KeyboardAndMouse::HKL,
    a6: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("DeactivateProfile");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputProcessorProfileMgr_GetProfile(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: *const windows_core::GUID,
    a5: windows::Win32::UI::Input::KeyboardAndMouse::HKL,
    a6: *mut TF_INPUTPROCESSORPROFILE,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            source::fixed(a1, a2, a3, a4, a5, 0);
            p.hit("target.read");
            a6.write(p.profile().available[(a2 == 0x419) as usize].get());
            p.result("GetProfile")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputProcessorProfileMgr_EnumProfiles(
    this: *mut core::ffi::c_void,
    a1: u16,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EnumProfiles");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputProcessorProfileMgr_ReleaseInputProcessor(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ReleaseInputProcessor");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputProcessorProfileMgr_RegisterProfile(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: windows_core::PCWSTR,
    a5: u32,
    a6: windows_core::PCWSTR,
    a7: u32,
    a8: u32,
    a9: windows::Win32::UI::Input::KeyboardAndMouse::HKL,
    a10: u32,
    a11: windows_core::BOOL,
    a12: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("RegisterProfile");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputProcessorProfileMgr_UnregisterProfile(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("UnregisterProfile");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputProcessorProfileMgr_GetActiveProfile(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *mut TF_INPUTPROCESSORPROFILE,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetActiveProfile");
            p.hit("profile.read");
            a2.write(p.profile().profile.get());
            p.result("GetActiveProfile")
        })
    }
}
pub(super) const ITFINPUTPROCESSORPROFILESUBSTITUTELAYOUT_TABLE:
    ITfInputProcessorProfileSubstituteLayout_Vtbl = ITfInputProcessorProfileSubstituteLayout_Vtbl {
    base__: UNKNOWN,
    GetSubstituteKeyboardLayout:
        ITfInputProcessorProfileSubstituteLayout_GetSubstituteKeyboardLayout,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputProcessorProfileSubstituteLayout_GetSubstituteKeyboardLayout(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: *mut windows::Win32::UI::Input::KeyboardAndMouse::HKL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetSubstituteKeyboardLayout");
            let index = (a2 == 0x419) as usize;
            let expected = source_profile(index);
            assert_eq!(*a1, expected.clsid);
            assert_eq!(a2, expected.langid);
            assert_eq!(*a3, expected.guidProfile);
            p.hit(if index == 0 {
                "substitute.en"
            } else {
                "substitute.ru"
            });
            let state = p.profile();
            a4.write(state.bindings[index].get().unwrap_or_else(|| {
                if state.profile.get().langid == a2 {
                    state.substitute.get()
                } else {
                    expected.hklSubstitute
                }
            }));
            p.result("GetSubstituteKeyboardLayout")
        })
    }
}
