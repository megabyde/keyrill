use super::*;
pub const DIAGNOSTIC_V1: GUID = GUID::from_u128(0xf75f909b_293a_4a7b_a172_f5a061880352);
pub const DIAGNOSTIC_V2: GUID = GUID::from_u128(0x4f89429e_a955_4885_bb33_c78ad1982481);
pub struct Diagnostic {
    pub snapshot: RefCell<[u32; 141]>,
    pub result: Cell<HRESULT>,
    pub legacy_only: Cell<bool>,
    pub reads: Cell<u32>,
    pub legacy_reads: Cell<u32>,
    pub foreign_function: RefCell<Option<IUnknown>>,
}
impl Default for Diagnostic {
    fn default() -> Self {
        Self {
            snapshot: RefCell::new([0; 141]),
            result: Cell::new(S_OK),
            legacy_only: Cell::new(false),
            reads: Cell::new(0),
            legacy_reads: Cell::new(0),
            foreign_function: RefCell::new(None),
        }
    }
}
#[repr(C)]
pub struct ReaderTable {
    pub base__: windows_core::IUnknown_Vtbl,
    pub copy: unsafe extern "system" fn(*mut c_void, u32, u32, *mut u32) -> HRESULT,
}
pub(super) const READER_V1: ReaderTable = ReaderTable {
    base__: UNKNOWN,
    copy: copy_v1,
};
pub(super) const READER_V2: ReaderTable = ReaderTable {
    base__: UNKNOWN,
    copy: copy_v2,
};
unsafe extern "system" fn copy_v1(
    this: *mut c_void,
    size: u32,
    version: u32,
    out: *mut u32,
) -> HRESULT {
    unsafe {
        boundary(this, |p| {
            let d = p.diagnostic();
            d.legacy_reads.set(d.legacy_reads.get() + 1);
            if size != 220 || version != 1 {
                return E_INVALIDARG;
            }
            std::ptr::copy_nonoverlapping(d.snapshot.borrow().as_ptr(), out, 55);
            S_OK
        })
    }
}
unsafe extern "system" fn copy_v2(
    this: *mut c_void,
    size: u32,
    version: u32,
    out: *mut u32,
) -> HRESULT {
    unsafe {
        boundary(this, |p| {
            assert_eq!((size, version), (564, 2));
            let d = p.diagnostic();
            d.reads.set(d.reads.get() + 1);
            p.hit("copy");
            std::ptr::write_bytes(out, 0, 141);
            if d.result.get() == S_OK {
                std::ptr::copy_nonoverlapping(d.snapshot.borrow().as_ptr(), out, 141);
            }
            d.result.get()
        })
    }
}
pub fn source_profile(index: usize) -> TF_INPUTPROCESSORPROFILE {
    assert!(index < 2);
    TF_INPUTPROCESSORPROFILE {
        dwProfileType: TF_PROFILETYPE_INPUTPROCESSOR,
        langid: if index == 0 { 0x409 } else { 0x419 },
        clsid: SERVICE_ID,
        guidProfile: if index == 0 {
            PROFILE_EN
        } else {
            GUID::from_u128(0x4c1e583c_f8e5_4b7d_b66c_b7e7319aed51)
        },
        catid: GUID_TFCAT_TIP_KEYBOARD,
        hklSubstitute: HKL(if index == 0 { 0x04090409 } else { 0x04190419 } as *mut _),
        dwFlags: TF_IPP_FLAG_ACTIVE,
        ..Default::default()
    }
}
pub(super) unsafe fn fixed(
    kind: u32,
    lang: u16,
    clsid: *const GUID,
    id: *const GUID,
    layout: HKL,
    flags: u32,
) {
    assert!(matches!(lang, 0x409 | 0x419));
    let target = source_profile((lang == 0x419) as usize);
    assert_eq!(kind, TF_PROFILETYPE_INPUTPROCESSOR);
    assert_eq!(lang, target.langid);
    unsafe {
        assert_eq!(*clsid, target.clsid);
        assert_eq!(*id, target.guidProfile);
    }
    assert!(layout.0.is_null());
    assert_eq!(flags, 0);
}
pub(super) const ITFINPUTPROCESSORPROFILES_TABLE: ITfInputProcessorProfiles_Vtbl =
    ITfInputProcessorProfiles_Vtbl {
        base__: UNKNOWN,
        Register: ITfInputProcessorProfiles_Register,
        Unregister: ITfInputProcessorProfiles_Unregister,
        AddLanguageProfile: ITfInputProcessorProfiles_AddLanguageProfile,
        RemoveLanguageProfile: ITfInputProcessorProfiles_RemoveLanguageProfile,
        EnumInputProcessorInfo: ITfInputProcessorProfiles_EnumInputProcessorInfo,
        GetDefaultLanguageProfile: ITfInputProcessorProfiles_GetDefaultLanguageProfile,
        SetDefaultLanguageProfile: ITfInputProcessorProfiles_SetDefaultLanguageProfile,
        ActivateLanguageProfile: ITfInputProcessorProfiles_ActivateLanguageProfile,
        GetActiveLanguageProfile: ITfInputProcessorProfiles_GetActiveLanguageProfile,
        GetLanguageProfileDescription: ITfInputProcessorProfiles_GetLanguageProfileDescription,
        GetCurrentLanguage: ITfInputProcessorProfiles_GetCurrentLanguage,
        ChangeCurrentLanguage: ITfInputProcessorProfiles_ChangeCurrentLanguage,
        GetLanguageList: ITfInputProcessorProfiles_GetLanguageList,
        EnumLanguageProfiles: ITfInputProcessorProfiles_EnumLanguageProfiles,
        EnableLanguageProfile: ITfInputProcessorProfiles_EnableLanguageProfile,
        IsEnabledLanguageProfile: ITfInputProcessorProfiles_IsEnabledLanguageProfile,
        EnableLanguageProfileByDefault: ITfInputProcessorProfiles_EnableLanguageProfileByDefault,
        SubstituteKeyboardLayout: ITfInputProcessorProfiles_SubstituteKeyboardLayout,
    };
unsafe extern "system" fn ITfInputProcessorProfiles_Register(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_Unregister(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_AddLanguageProfile(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: windows_core::PCWSTR,
    a5: u32,
    a6: windows_core::PCWSTR,
    a7: u32,
    a8: u32,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_RemoveLanguageProfile(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_EnumInputProcessorInfo(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_GetDefaultLanguageProfile(
    this: *mut core::ffi::c_void,
    a1: u16,
    a2: *const windows_core::GUID,
    a3: *mut windows_core::GUID,
    a4: *mut windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_SetDefaultLanguageProfile(
    this: *mut core::ffi::c_void,
    a1: u16,
    a2: *const windows_core::GUID,
    a3: *const windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_ActivateLanguageProfile(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_GetActiveLanguageProfile(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *mut u16,
    a3: *mut windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_GetLanguageProfileDescription(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_GetCurrentLanguage(
    this: *mut core::ffi::c_void,
    a1: *mut u16,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("language.read");
            a1.write(p.profile().language.get());
            p.result("GetCurrentLanguage")
        })
    }
}
unsafe extern "system" fn ITfInputProcessorProfiles_ChangeCurrentLanguage(
    this: *mut core::ffi::c_void,
    a1: u16,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            assert!(matches!(a1, 0x409 | 0x419));
            let state = p.profile();
            state.language_calls.set(state.language_calls.get() + 1);
            if state.language_effect.get() {
                state.language.set(a1);
                let mut active = source_profile((a1 == 0x419) as usize);
                active.clsid = GUID::zeroed();
                active.guidProfile = GUID::zeroed();
                state.profile.set(active);
                let language = a1 as usize;
                state
                    .substitute
                    .set(HKL((language << 16 | language) as *mut _));
            }
            p.hit("language.change");
            p.result("ChangeCurrentLanguage")
        })
    }
}
unsafe extern "system" fn ITfInputProcessorProfiles_GetLanguageList(
    this: *mut core::ffi::c_void,
    a1: *mut *mut u16,
    a2: *mut u32,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_EnumLanguageProfiles(
    this: *mut core::ffi::c_void,
    a1: u16,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_EnableLanguageProfile(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_IsEnabledLanguageProfile(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            fixed(TF_PROFILETYPE_INPUTPROCESSOR, a2, a1, a3, HKL::default(), 0);
            p.hit("enabled.read");
            a4.write(p.profile().enabled.get());
            p.result("IsEnabledLanguageProfile")
        })
    }
}
unsafe extern "system" fn ITfInputProcessorProfiles_EnableLanguageProfileByDefault(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfInputProcessorProfiles_SubstituteKeyboardLayout(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: u16,
    a3: *const windows_core::GUID,
    a4: windows::Win32::UI::Input::KeyboardAndMouse::HKL,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
pub(super) const ITFFUNCTIONPROVIDER_TABLE: ITfFunctionProvider_Vtbl = ITfFunctionProvider_Vtbl {
    base__: UNKNOWN,
    GetType: ITfFunctionProvider_GetType,
    GetDescription: ITfFunctionProvider_GetDescription,
    GetFunction: ITfFunctionProvider_GetFunction,
};
unsafe extern "system" fn ITfFunctionProvider_GetType(
    this: *mut core::ffi::c_void,
    a1: *mut windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfFunctionProvider_GetDescription(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe { boundary(this, |p| E_NOTIMPL) }
}
unsafe extern "system" fn ITfFunctionProvider_GetFunction(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *const windows_core::GUID,
    a3: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            assert_eq!(*a1, GUID::zeroed());
            assert_eq!(*a2, DIAGNOSTIC_V2);
            p.hit("function");
            let foreign = p
                .diagnostic()
                .foreign_function
                .borrow()
                .as_ref()
                .map(|p| p.as_raw());
            let raw = foreign.unwrap_or_else(|| p.interface(IUnknown::IID));
            let table = *(raw.cast::<*const windows_core::IUnknown_Vtbl>());
            ((*table).QueryInterface)(raw, a2, a3)
        })
    }
}
