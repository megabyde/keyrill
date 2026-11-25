use super::*;

pub(super) const ITFTHREADMGR_TABLE: ITfThreadMgr_Vtbl = ITfThreadMgr_Vtbl {
    base__: UNKNOWN,
    Activate: ITfThreadMgr_Activate,
    Deactivate: ITfThreadMgr_Deactivate,
    CreateDocumentMgr: ITfThreadMgr_CreateDocumentMgr,
    EnumDocumentMgrs: ITfThreadMgr_EnumDocumentMgrs,
    GetFocus: ITfThreadMgr_GetFocus,
    SetFocus: ITfThreadMgr_SetFocus,
    AssociateFocus: ITfThreadMgr_AssociateFocus,
    IsThreadFocus: ITfThreadMgr_IsThreadFocus,
    GetFunctionProvider: ITfThreadMgr_GetFunctionProvider,
    EnumFunctionProviders: ITfThreadMgr_EnumFunctionProviders,
    GetGlobalCompartment: ITfThreadMgr_GetGlobalCompartment,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_Activate(
    this: *mut core::ffi::c_void,
    a1: *mut u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Activate");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_Deactivate(
    this: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Deactivate");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_CreateDocumentMgr(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("CreateDocumentMgr");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_EnumDocumentMgrs(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EnumDocumentMgrs");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_GetFocus(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetFocus");
            a1.write(if p.null("GetFocus") {
                std::ptr::null_mut()
            } else {
                p.own(ITfDocumentMgr::IID)
            });
            p.result("GetFocus")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_SetFocus(
    this: *mut core::ffi::c_void,
    a1: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SetFocus");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_AssociateFocus(
    this: *mut core::ffi::c_void,
    a1: windows::Win32::Foundation::HWND,
    a2: *mut core::ffi::c_void,
    a3: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("AssociateFocus");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_IsThreadFocus(
    this: *mut core::ffi::c_void,
    a1: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("IsThreadFocus");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_GetFunctionProvider(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetFunctionProvider");
            a2.write(std::ptr::null_mut());
            let result = p.result("GetFunctionProvider");
            if result != S_OK {
                return result;
            }
            if p.null("GetFunctionProvider") {
                return S_OK;
            }
            let raw = p.manager().diagnostic.borrow().as_ref().map(|s| s.as_raw());
            if let Some(raw) = raw {
                let table = *(raw.cast::<*const windows_core::IUnknown_Vtbl>());
                ((*table).QueryInterface)(raw, &ITfFunctionProvider::IID, a2)
            } else {
                TF_E_NOPROVIDER
            }
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_EnumFunctionProviders(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EnumFunctionProviders");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfThreadMgr_GetGlobalCompartment(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetGlobalCompartment");
            E_NOTIMPL
        })
    }
}
pub(super) const ITFDOCUMENTMGR_TABLE: ITfDocumentMgr_Vtbl = ITfDocumentMgr_Vtbl {
    base__: UNKNOWN,
    CreateContext: ITfDocumentMgr_CreateContext,
    Push: ITfDocumentMgr_Push,
    Pop: ITfDocumentMgr_Pop,
    GetTop: ITfDocumentMgr_GetTop,
    GetBase: ITfDocumentMgr_GetBase,
    EnumContexts: ITfDocumentMgr_EnumContexts,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfDocumentMgr_CreateContext(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u32,
    a3: *mut core::ffi::c_void,
    a4: *mut *mut core::ffi::c_void,
    a5: *mut u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("CreateContext");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfDocumentMgr_Push(
    this: *mut core::ffi::c_void,
    a1: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Push");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfDocumentMgr_Pop(
    this: *mut core::ffi::c_void,
    a1: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Pop");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfDocumentMgr_GetTop(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetTop");
            let pointer = p.manager().top.borrow().as_raw();
            a1.write(if p.null("GetTop") {
                std::ptr::null_mut()
            } else {
                retain(pointer)
            });
            p.result("GetTop")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfDocumentMgr_GetBase(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetBase");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfDocumentMgr_EnumContexts(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("EnumContexts");
            E_NOTIMPL
        })
    }
}
pub(super) const ITFSOURCE_TABLE: ITfSource_Vtbl = ITfSource_Vtbl {
    base__: UNKNOWN,
    AdviseSink: ITfSource_AdviseSink,
    UnadviseSink: ITfSource_UnadviseSink,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfSource_AdviseSink(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *mut core::ffi::c_void,
    a3: *mut u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            if *a1 == ITfStatusSink::IID || *a1 == ITfCompartmentEventSink::IID {
                let result = p.host.watch_result.get();
                if result == S_OK {
                    let cookie = p.host.watch_cookie.get() + 1;
                    p.host.watch_cookie.set(cookie);
                    let sink = IUnknown::from_raw(retain(a2));
                    p.host
                        .watches
                        .borrow_mut()
                        .push(super::super::schedule::WatchRegistration {
                            cookie,
                            id: *a1,
                            sink,
                        });
                    if !p.host.watch_missing_cookie.get() {
                        a3.write(cookie);
                    }
                }
                p.hit("AdviseAdmission");
                return result;
            }
            let m = p.manager();
            let index = if *a1 == ITfThreadMgrEventSink::IID {
                0
            } else {
                1
            };
            m.advises[index].set(m.advises[index].get() + 1);
            if index == 0 && m.sink_results[index].get() == S_OK {
                replace_reference(&m.thread_sink, Some(IUnknown::from_raw(retain(a2))));
            }
            a3.write(if m.sink_results[index].get() == S_OK {
                index as u32 + 1
            } else {
                TF_INVALID_COOKIE
            });
            p.hit("AdviseSink");
            m.sink_results[index].get()
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfSource_UnadviseSink(
    this: *mut core::ffi::c_void,
    a1: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            if a1 > 100 {
                let retired = {
                    let mut watches = p.host.watches.borrow_mut();
                    let index = watches.iter().position(|watch| watch.cookie == a1).unwrap();
                    watches.remove(index)
                };
                drop(retired);
                p.hit("UnadviseAdmission");
                return p.host.unwatch_result.get();
            }
            let m = p.manager();
            assert!(a1 == 1 || a1 == 2);
            let index = (a1 - 1) as usize;
            if index == 0 {
                replace_reference(&m.thread_sink, None);
            }
            m.unadvises[index].set(m.unadvises[index].get() + 1);
            p.hit("UnadviseSink");
            p.hit(if index == 0 {
                "UnadviseThreadSink"
            } else {
                "UnadviseProfileSink"
            });
            m.unsink_results[index].get()
        })
    }
}
pub(super) const ITFSOURCESINGLE_TABLE: ITfSourceSingle_Vtbl = ITfSourceSingle_Vtbl {
    base__: UNKNOWN,
    AdviseSingleSink: ITfSourceSingle_AdviseSingleSink,
    UnadviseSingleSink: ITfSourceSingle_UnadviseSingleSink,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfSourceSingle_AdviseSingleSink(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *const windows_core::GUID,
    a3: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.manager()
                .diagnostic_advises
                .set(p.manager().diagnostic_advises.get() + 1);
            p.hit("AdviseSingleSink");
            replace_reference(
                &p.manager().diagnostic,
                Some(IUnknown::from_raw(retain(a3))),
            );
            p.result("AdviseSingleSink")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfSourceSingle_UnadviseSingleSink(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *const windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.manager()
                .diagnostic_unadvises
                .set(p.manager().diagnostic_unadvises.get() + 1);
            replace_reference(&p.manager().diagnostic, None);
            p.hit("UnadviseSingleSink");
            p.result("UnadviseSingleSink")
        })
    }
}
pub(super) const ITFKEYSTROKEMGR_TABLE: ITfKeystrokeMgr_Vtbl = ITfKeystrokeMgr_Vtbl {
    base__: UNKNOWN,
    AdviseKeyEventSink: ITfKeystrokeMgr_AdviseKeyEventSink,
    UnadviseKeyEventSink: ITfKeystrokeMgr_UnadviseKeyEventSink,
    GetForeground: ITfKeystrokeMgr_GetForeground,
    TestKeyDown: ITfKeystrokeMgr_TestKeyDown,
    TestKeyUp: ITfKeystrokeMgr_TestKeyUp,
    KeyDown: ITfKeystrokeMgr_KeyDown,
    KeyUp: ITfKeystrokeMgr_KeyUp,
    GetPreservedKey: ITfKeystrokeMgr_GetPreservedKey,
    IsPreservedKey: ITfKeystrokeMgr_IsPreservedKey,
    PreserveKey: ITfKeystrokeMgr_PreserveKey,
    UnpreserveKey: ITfKeystrokeMgr_UnpreserveKey,
    SetPreservedKeyDescription: ITfKeystrokeMgr_SetPreservedKeyDescription,
    GetPreservedKeyDescription: ITfKeystrokeMgr_GetPreservedKeyDescription,
    SimulatePreservedKey: ITfKeystrokeMgr_SimulatePreservedKey,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_AdviseKeyEventSink(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            let m = p.manager();
            assert_eq!(a1, 42);
            m.advises[2].set(m.advises[2].get() + 1);
            let advised = if m.sink_results[2].get() == S_OK {
                if a2.is_null() {
                    return E_POINTER;
                }
                Some(Rc::new(AdvisedKeySink {
                    client: a1,
                    foreground: a3,
                    sink: ITfKeyEventSink::from_raw(retain(a2)),
                }))
            } else {
                None
            };
            let previous = m.key_sink.replace(advised);
            drop(previous);
            p.hit("AdviseKeyEventSink");
            m.sink_results[2].get()
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_UnadviseKeyEventSink(
    this: *mut core::ffi::c_void,
    a1: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            let m = p.manager();
            assert_eq!(a1, 42);
            m.unadvises[2].set(m.unadvises[2].get() + 1);
            let advised = m.key_sink.take();
            drop(advised);
            p.hit("UnadviseKeyEventSink");
            m.unsink_results[2].get()
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_GetForeground(
    this: *mut core::ffi::c_void,
    a1: *mut windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetForeground");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_TestKeyDown(
    this: *mut core::ffi::c_void,
    a1: windows::Win32::Foundation::WPARAM,
    a2: windows::Win32::Foundation::LPARAM,
    a3: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("TestKeyDown");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_TestKeyUp(
    this: *mut core::ffi::c_void,
    a1: windows::Win32::Foundation::WPARAM,
    a2: windows::Win32::Foundation::LPARAM,
    a3: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("TestKeyUp");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_KeyDown(
    this: *mut core::ffi::c_void,
    a1: windows::Win32::Foundation::WPARAM,
    a2: windows::Win32::Foundation::LPARAM,
    a3: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("KeyDown");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_KeyUp(
    this: *mut core::ffi::c_void,
    a1: windows::Win32::Foundation::WPARAM,
    a2: windows::Win32::Foundation::LPARAM,
    a3: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("KeyUp");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_GetPreservedKey(
    this: *mut core::ffi::c_void,
    a1: *mut core::ffi::c_void,
    a2: *const TF_PRESERVEDKEY,
    a3: *mut windows_core::GUID,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetPreservedKey");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_IsPreservedKey(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *const TF_PRESERVEDKEY,
    a3: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("IsPreservedKey");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_PreserveKey(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *const windows_core::GUID,
    a3: *const TF_PRESERVEDKEY,
    a4: windows_core::PCWSTR,
    a5: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            let m = p.manager();
            let registration = Registration {
                client: a1,
                id: *a2,
                key: *a3,
            };
            let index = match (*a3).uVKey {
                75 => 0,
                90 => 1,
                _ => 2,
            };
            m.preserve_calls.borrow_mut().push(registration);
            if m.preserve_results[index].get() == S_OK {
                m.preserved_keys.borrow_mut().push(registration);
            }
            p.hit("PreserveKey");
            m.preserve_results[index].get()
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_UnpreserveKey(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *const TF_PRESERVEDKEY,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            let m = p.manager();
            let registration = Registration {
                client: 42,
                id: *a1,
                key: *a2,
            };
            let index = match (*a2).uVKey {
                75 => 0,
                90 => 1,
                _ => 2,
            };
            m.unpreserved_keys.borrow_mut().push(registration);
            p.hit("UnpreserveKey");
            m.unpreserve_results[index].get()
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_SetPreservedKeyDescription(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: windows_core::PCWSTR,
    a3: u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SetPreservedKeyDescription");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_GetPreservedKeyDescription(
    this: *mut core::ffi::c_void,
    a1: *const windows_core::GUID,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetPreservedKeyDescription");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfKeystrokeMgr_SimulatePreservedKey(
    this: *mut core::ffi::c_void,
    a1: *mut core::ffi::c_void,
    a2: *const windows_core::GUID,
    a3: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SimulatePreservedKey");
            E_NOTIMPL
        })
    }
}
