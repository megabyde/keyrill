use super::*;

pub(super) const ITFCOMPOSITION_TABLE: ITfComposition_Vtbl = ITfComposition_Vtbl {
    base__: UNKNOWN,
    GetRange: ITfComposition_GetRange,
    ShiftStart: ITfComposition_ShiftStart,
    ShiftEnd: ITfComposition_ShiftEnd,
    EndComposition: ITfComposition_EndComposition,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfComposition_GetRange(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetRange");
            a1.write(p.range(p.span()));
            p.result("GetRange")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfComposition_ShiftStart(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ShiftStart");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfComposition_ShiftEnd(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ShiftEnd");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfComposition_EndComposition(
    this: *mut core::ffi::c_void,
    a1: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EndComposition");
            let d = p.document();
            d.ends.set(d.ends.get() + 1);
            d.effect(Effect::End, p.span().get(), vec![]);
            if !d.keep_composition.get() {
                d.active.borrow_mut().take();
            }
            if d.move_clones.get() {
                let end = d.text.borrow().len() as i32;
                for clone in d.clones.borrow().iter().filter_map(|s| s.upgrade()) {
                    clone.set(Span {
                        start: end,
                        length: 0,
                    });
                }
            }
            p.hit("EndComposition.after");
            p.result("EndComposition")
        })
    }
}
pub(super) const ITFCOMPOSITIONVIEW_TABLE: ITfCompositionView_Vtbl = ITfCompositionView_Vtbl {
    base__: UNKNOWN,
    GetOwnerClsid: ITfCompositionView_GetOwnerClsid,
    GetRange: ITfCompositionView_GetRange,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfCompositionView_GetOwnerClsid(
    this: *mut core::ffi::c_void,
    a1: *mut windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetOwnerClsid");
            a1.write(SERVICE_ID);
            S_OK
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfCompositionView_GetRange(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetRange");
            a1.write(p.range(p.span()));
            p.result("GetRange")
        })
    }
}
pub(super) const IENUMITFCOMPOSITIONVIEW_TABLE: IEnumITfCompositionView_Vtbl =
    IEnumITfCompositionView_Vtbl {
        base__: UNKNOWN,
        Clone: IEnumITfCompositionView_Clone,
        Next: IEnumITfCompositionView_Next,
        Reset: IEnumITfCompositionView_Reset,
        Skip: IEnumITfCompositionView_Skip,
    };
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn IEnumITfCompositionView_Clone(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Clone");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn IEnumITfCompositionView_Next(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut *mut core::ffi::c_void,
    a3: *mut u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Next");
            assert_eq!(a1, 1);
            a2.write(std::ptr::null_mut());
            a3.write(0);
            let Kind::Enumeration(d, index, views) = &p.kind else {
                unreachable!()
            };
            let active = d.active.borrow().as_ref().map(|a| a.pointer);
            let count = u32::from(active.is_some()) + u32::from(d.foreign.get());
            let found = index.get() < count;
            if found {
                if *views
                    && index.get() == 0
                    && let Some(ptr) = active
                {
                    let value = owner(ptr).own(ITfCompositionView::IID);
                    a2.write(value);
                }
                a3.write(1);
                index.set(index.get() + 1);
                S_OK
            } else {
                S_FALSE
            }
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn IEnumITfCompositionView_Reset(
    this: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Reset");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn IEnumITfCompositionView_Skip(
    this: *mut core::ffi::c_void,
    a1: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Skip");
            E_NOTIMPL
        })
    }
}
