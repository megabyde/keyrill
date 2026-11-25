use super::*;

pub(super) const ITFCONTEXT_TABLE: ITfContext_Vtbl = ITfContext_Vtbl {
    base__: UNKNOWN,
    RequestEditSession: ITfContext_RequestEditSession,
    InWriteSession: ITfContext_InWriteSession,
    GetSelection: ITfContext_GetSelection,
    SetSelection: ITfContext_SetSelection,
    GetStart: ITfContext_GetStart,
    GetEnd: ITfContext_GetEnd,
    GetActiveView: ITfContext_GetActiveView,
    EnumViews: ITfContext_EnumViews,
    GetStatus: ITfContext_GetStatus,
    GetProperty: ITfContext_GetProperty,
    GetAppProperty: ITfContext_GetAppProperty,
    TrackProperties: ITfContext_TrackProperties,
    EnumProperties: ITfContext_EnumProperties,
    GetDocumentMgr: ITfContext_GetDocumentMgr,
    CreateRangeBackup: ITfContext_CreateRangeBackup,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_RequestEditSession(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: TF_CONTEXT_EDIT_CONTEXT_FLAGS,
    a4: *mut windows_core::HRESULT,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("RequestEditSession");
            assert_eq!(a1, 42);
            assert_eq!(a3, p.document().request_flags.get());
            let d = p.document();
            d.request.set(d.request.get() + 1);
            d.finish_selection.set(
                d.active
                    .borrow()
                    .as_ref()
                    .map(|_| (d.request.get(), d.selection.get(), d.style.get())),
            );
            let session = IUnknown::from_raw(retain(a2));
            if d.retain_callback.get() || !d.invoke.get() {
                replace_reference(&d.retained, Some(session.clone()));
            }
            let mut result = d.result("session");
            if d.invoke.get() {
                let es = session.cast::<ITfEditSession>().unwrap();
                let actual = (es.vtable().DoEditSession)(es.as_raw(), 123);
                if d.repeat.get() {
                    assert_eq!((es.vtable().DoEditSession)(es.as_raw(), 123), E_FAIL);
                }
                if result == S_OK {
                    result = actual;
                }
            }
            a4.write(result);
            p.hit("RequestEditSession.after");
            d.result("RequestEditSession")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_InWriteSession(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("InWriteSession");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_GetSelection(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u32,
    a3: u32,
    a4: *mut TF_SELECTION,
    a5: *mut u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetSelection");
            assert_eq!(a1, 123);
            assert_eq!(a3, 1);
            let d = p.document();
            let range = if d.null("GetSelection") {
                None
            } else {
                Some(ITfRange::from_raw(
                    p.range(Rc::new(Cell::new(d.selection.get()))),
                ))
            };
            a4.write(TF_SELECTION {
                range: std::mem::ManuallyDrop::new(range),
                style: d.style.get(),
            });
            a5.write(d.selection_fetched.get());
            d.result("GetSelection")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_SetSelection(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u32,
    a3: *const TF_SELECTION,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SetSelection");
            assert_eq!(a2, 1);
            let d = p.document();
            let value = &*a3;
            let range = value.range.as_ref().unwrap();
            let span = owner(range.as_raw()).span().get();
            if d.active.borrow().is_some() {
                assert_eq!(span.length, 0, "no temporary selection");
            } else {
                let (request, expected, style) = d
                    .finish_selection
                    .get()
                    .expect("selection restoration requires unchanged owned finish");
                assert_eq!(request, d.request.get());
                assert_eq!(span, expected);
                assert_eq!(value.style.ase, style.ase);
                assert_eq!(value.style.fInterimChar, style.fInterimChar);
                assert_eq!(
                    d.operations.borrow().last().unwrap().effect,
                    Effect::End,
                    "only one selection restoration after EndComposition"
                );
            }
            d.selection.set(span);
            d.style.set(value.style);
            d.effect(Effect::Selection, span, vec![]);
            p.hit("SetSelection.after");
            d.result("SetSelection")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_GetStart(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetStart");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_GetEnd(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetEnd");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_GetActiveView(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetActiveView");
            a1.write(if p.null("GetActiveView") {
                std::ptr::null_mut()
            } else {
                p.own(ITfContextView::IID)
            });
            p.result("GetActiveView")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_EnumViews(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EnumViews");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_GetStatus(
    this: *mut core::ffi::c_void,
    a1: *mut TS_STATUS,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetStatus");
            a1.write(TS_STATUS {
                dwDynamicFlags: p.document().status.get(),
                dwStaticFlags: 0,
            });
            p.result("GetStatus")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_GetProperty(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetProperty");
            a2.write(std::ptr::null_mut());
            S_FALSE
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_GetAppProperty(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetAppProperty");
            a2.write(if p.null("GetAppProperty") {
                std::ptr::null_mut()
            } else {
                p.own(ITfReadOnlyProperty::IID)
            });
            p.result("GetAppProperty")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_TrackProperties(
    this: *mut core::ffi::c_void,
    a1: *const *const windows_core::GUID,
    a2: u32,
    a3: *const *const windows_core::GUID,
    a4: u32,
    a5: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("TrackProperties");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_EnumProperties(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EnumProperties");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_GetDocumentMgr(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetDocumentMgr");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContext_CreateRangeBackup(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("CreateRangeBackup");
            E_NOTIMPL
        })
    }
}
pub(super) const ITFCONTEXTCOMPOSITION_TABLE: ITfContextComposition_Vtbl =
    ITfContextComposition_Vtbl {
        base__: UNKNOWN,
        StartComposition: ITfContextComposition_StartComposition,
        EnumCompositions: ITfContextComposition_EnumCompositions,
        FindComposition: ITfContextComposition_FindComposition,
        TakeOwnership: ITfContextComposition_TakeOwnership,
    };
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContextComposition_StartComposition(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: *mut core::ffi::c_void,
    a4: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("StartComposition");
            let d = p.document();
            a4.write(std::ptr::null_mut());
            d.starts.set(d.starts.get() + 1);
            if d.reject_composition.get() {
                return S_OK;
            }
            let span = owner(a2).span();
            let value = Provider::create(
                Rc::clone(&p.host),
                Kind::Composition(Rc::clone(d), Rc::clone(&span)),
                ITfComposition::IID,
            );
            d.active.replace(Some(Active {
                pointer: value,
                span: Rc::clone(&span),
                serial: d.starts.get(),
            }));
            d.effect(Effect::Start, span.get(), vec![]);
            p.hit("StartComposition.after");
            if p.null("StartComposition") {
                drop(ITfComposition::from_raw(value));
            } else {
                a4.write(value);
            }
            p.result("StartComposition")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContextComposition_EnumCompositions(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EnumCompositions");
            a1.write(Provider::create(
                Rc::clone(&p.host),
                Kind::Enumeration(Rc::clone(p.document()), Cell::new(0), true),
                IEnumITfCompositionView::IID,
            ));
            p.result("EnumCompositions")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContextComposition_FindComposition(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("FindComposition");
            a3.write(Provider::create(
                Rc::clone(&p.host),
                Kind::Enumeration(Rc::clone(p.document()), Cell::new(0), false),
                IEnumITfCompositionView::IID,
            ));
            p.result("FindComposition")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContextComposition_TakeOwnership(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: *mut core::ffi::c_void,
    a4: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("TakeOwnership");
            E_NOTIMPL
        })
    }
}
pub(super) const ITFCONTEXTVIEW_TABLE: ITfContextView_Vtbl = ITfContextView_Vtbl {
    base__: UNKNOWN,
    GetRangeFromPoint: ITfContextView_GetRangeFromPoint,
    GetTextExt: ITfContextView_GetTextExt,
    GetScreenExt: ITfContextView_GetScreenExt,
    GetWnd: ITfContextView_GetWnd,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContextView_GetRangeFromPoint(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *const windows::Win32::Foundation::POINT,
    a3: u32,
    a4: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetRangeFromPoint");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContextView_GetTextExt(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: *mut windows::Win32::Foundation::RECT,
    a4: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetTextExt");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContextView_GetScreenExt(
    this: *mut core::ffi::c_void,
    a1: *mut windows::Win32::Foundation::RECT,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetScreenExt");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfContextView_GetWnd(
    this: *mut core::ffi::c_void,
    a1: *mut windows::Win32::Foundation::HWND,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetWnd");
            a1.write(p.document().view_window.get());
            p.result("GetWnd")
        })
    }
}
pub(super) const ITFPROPERTY_TABLE: ITfProperty_Vtbl = ITfProperty_Vtbl {
    base__: ITFREADONLYPROPERTY_TABLE,
    FindRange: ITfProperty_FindRange,
    SetValueStore: ITfProperty_SetValueStore,
    SetValue: ITfProperty_SetValue,
    Clear: ITfProperty_Clear,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfProperty_FindRange(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: *mut *mut core::ffi::c_void,
    a4: TfAnchor,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("FindRange");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfProperty_SetValueStore(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SetValueStore");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfProperty_SetValue(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: *const windows::Win32::System::Variant::VARIANT,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SetValue");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfProperty_Clear(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Clear");
            E_NOTIMPL
        })
    }
}
pub(super) const ITFREADONLYPROPERTY_TABLE: ITfReadOnlyProperty_Vtbl = ITfReadOnlyProperty_Vtbl {
    base__: UNKNOWN,
    GetType: ITfReadOnlyProperty_GetType,
    EnumRanges: ITfReadOnlyProperty_EnumRanges,
    GetValue: ITfReadOnlyProperty_GetValue,
    GetContext: ITfReadOnlyProperty_GetContext,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfReadOnlyProperty_GetType(
    this: *mut core::ffi::c_void,
    a1: *mut windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetType");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfReadOnlyProperty_EnumRanges(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut *mut core::ffi::c_void,
    a3: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EnumRanges");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfReadOnlyProperty_GetValue(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: *mut windows::Win32::System::Variant::VARIANT,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("scope.GetValue");
            let d = p.document();
            let span = owner(a2).span().get();
            if d.excluded_scope.get() == Some(span) {
                a3.write(VARIANT::default());
                return E_ACCESSDENIED;
            }
            let mut value = VARIANT::default();
            (*value.Anonymous.Anonymous).vt = d.scope_type.get();
            if d.scope_type.get() == VT_UNKNOWN && d.scope_value_present.get() {
                (*value.Anonymous.Anonymous).Anonymous.punkVal = std::mem::ManuallyDrop::new(Some(
                    IUnknown::from_raw(p.own(ITfInputScope::IID)),
                ));
            }
            a3.write(value);
            p.result("scope.GetValue")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfReadOnlyProperty_GetContext(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetContext");
            E_NOTIMPL
        })
    }
}
pub(super) const ITFINPUTSCOPE_TABLE: ITfInputScope_Vtbl = ITfInputScope_Vtbl {
    base__: UNKNOWN,
    GetInputScopes: ITfInputScope_GetInputScopes,
    GetPhrase: ITfInputScope_GetPhrase,
    GetRegularExpression: ITfInputScope_GetRegularExpression,
    GetSRGS: ITfInputScope_GetSRGS,
    GetXML: ITfInputScope_GetXML,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputScope_GetInputScopes(
    this: *mut core::ffi::c_void,
    a1: *mut *mut InputScope,
    a2: *mut u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetInputScopes");
            let d = p.document();
            a1.write(std::ptr::null_mut());
            a2.write(d.scope_count.get());
            if !d.null("GetInputScopes") {
                let ptr =
                    CoTaskMemAlloc((d.scope_count.get().max(1) as usize) * size_of::<InputScope>())
                        .cast::<InputScope>();
                assert!(!ptr.is_null());
                for n in 0..d.scope_count.get().max(1) {
                    ptr.add(n as usize).write(if n == 0 {
                        d.scope.get()
                    } else {
                        d.scope_second.get()
                    });
                }
                a1.write(ptr);
            }
            d.result("GetInputScopes")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputScope_GetPhrase(
    this: *mut core::ffi::c_void,
    a1: *mut *mut *mut core::ffi::c_void,
    a2: *mut u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetPhrase");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputScope_GetRegularExpression(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetRegularExpression");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputScope_GetSRGS(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetSRGS");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfInputScope_GetXML(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetXML");
            E_NOTIMPL
        })
    }
}
pub(super) const ITFCOMPARTMENTMGR_TABLE: ITfCompartmentMgr_Vtbl = ITfCompartmentMgr_Vtbl {
    base__: UNKNOWN,
    GetCompartment: ITfCompartmentMgr_GetCompartment,
    ClearCompartment: ITfCompartmentMgr_ClearCompartment,
    EnumCompartments: ITfCompartmentMgr_EnumCompartments,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfCompartmentMgr_GetCompartment(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetCompartment");
            if let Kind::Document(d) = &p.kind {
                d.queried_empty.set(*a1 == GUID_COMPARTMENT_EMPTYCONTEXT);
            }
            a2.write(if p.null("GetCompartment") {
                std::ptr::null_mut()
            } else {
                p.own(ITfCompartment::IID)
            });
            p.result("GetCompartment")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfCompartmentMgr_ClearCompartment(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *const windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ClearCompartment");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfCompartmentMgr_EnumCompartments(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EnumCompartments");
            E_NOTIMPL
        })
    }
}
pub(super) const ITFCOMPARTMENT_TABLE: ITfCompartment_Vtbl = ITfCompartment_Vtbl {
    base__: UNKNOWN,
    SetValue: ITfCompartment_SetValue,
    GetValue: ITfCompartment_GetValue,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfCompartment_SetValue(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *const windows::Win32::System::Variant::VARIANT,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SetValue");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfCompartment_GetValue(
    this: *mut core::ffi::c_void,
    a1: *mut windows::Win32::System::Variant::VARIANT,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("compartment.GetValue");
            let disabled = match &p.kind {
                Kind::Document(d) => {
                    if d.queried_empty.get() {
                        d.empty_disabled.get()
                    } else {
                        d.disabled.get()
                    }
                }
                Kind::Manager(m) => m.disabled.get(),
                _ => false,
            };
            let (kind, result) = match &p.kind {
                Kind::Document(d) => {
                    let index = d.queried_empty.get() as usize;
                    (
                        d.compartment_types.get()[index],
                        d.compartment_results.get()[index],
                    )
                }
                Kind::Manager(m) => (m.compartment_type.get(), m.compartment_result.get()),
                _ => unreachable!(),
            };
            let mut value = if kind == VT_I4 {
                VARIANT::from(disabled as i32)
            } else {
                VARIANT::default()
            };
            (*value.Anonymous.Anonymous).vt = kind;
            a1.write(value);
            if result != S_OK {
                result
            } else {
                p.result("compartment.GetValue")
            }
        })
    }
}
