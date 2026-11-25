use super::*;

pub(super) const ITFRANGE_TABLE: ITfRange_Vtbl = ITfRange_Vtbl {
    base__: UNKNOWN,
    GetText: ITfRange_GetText,
    SetText: ITfRange_SetText,
    GetFormattedText: ITfRange_GetFormattedText,
    GetEmbedded: ITfRange_GetEmbedded,
    InsertEmbedded: ITfRange_InsertEmbedded,
    ShiftStart: ITfRange_ShiftStart,
    ShiftEnd: ITfRange_ShiftEnd,
    ShiftStartToRange: ITfRange_ShiftStartToRange,
    ShiftEndToRange: ITfRange_ShiftEndToRange,
    ShiftStartRegion: ITfRange_ShiftStartRegion,
    ShiftEndRegion: ITfRange_ShiftEndRegion,
    IsEmpty: ITfRange_IsEmpty,
    Collapse: ITfRange_Collapse,
    IsEqualStart: ITfRange_IsEqualStart,
    IsEqualEnd: ITfRange_IsEqualEnd,
    CompareStart: ITfRange_CompareStart,
    CompareEnd: ITfRange_CompareEnd,
    AdjustForInsert: ITfRange_AdjustForInsert,
    GetGravity: ITfRange_GetGravity,
    SetGravity: ITfRange_SetGravity,
    Clone: ITfRange_Clone,
    GetContext: ITfRange_GetContext,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_GetText(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u32,
    a3: windows_core::PWSTR,
    a4: u32,
    a5: *mut u32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetText");
            let d = p.document();
            let span = p.span().get();
            d.reads.set(d.reads.get() + 1);
            d.text_reads.borrow_mut().push(span);
            let text = d.text.borrow();
            let range = &text[span.start as usize..(span.start + span.length) as usize];
            let count = range.len().min(a4 as usize);
            std::ptr::copy_nonoverlapping(range.as_ptr(), a3.0, count);
            a5.write(count as u32);
            drop(text);
            p.hit("GetText.after");
            p.result("GetText")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_SetText(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u32,
    a3: windows_core::PCWSTR,
    a4: i32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SetText");
            p.hit("SetText.before");
            if p.result("SetText.before") != S_OK {
                return p.result("SetText.before");
            }
            let d = p.document();
            let span = p.span();
            let old = span.get();
            assert!(
                d.active
                    .borrow()
                    .as_ref()
                    .is_some_and(|a| Rc::ptr_eq(&a.span, &span)),
                "only owned composition may write"
            );
            assert!(
                !d.operations
                    .borrow()
                    .iter()
                    .any(|o| o.effect == Effect::Text
                        && o.request == d.request.get()
                        && o.composition == d.starts.get()),
                "one write per composition plan"
            );
            let replacement = std::slice::from_raw_parts(a3.0, a4 as usize).to_vec();
            let unchanged = d.text.borrow()[old.start as usize..(old.start + old.length) as usize]
                == replacement;
            d.finish_selection.set(if unchanged {
                Some((d.request.get(), d.selection.get(), d.style.get()))
            } else {
                None
            });
            d.writes.set(d.writes.get() + 1);
            d.effect(Effect::Text, old, replacement.clone());
            d.text.borrow_mut().splice(
                old.start as usize..(old.start + old.length) as usize,
                replacement,
            );
            span.set(Span {
                start: old.start,
                length: a4,
            });
            if d.move_clones.get() {
                let end = d.text.borrow().len() as i32;
                for clone in d.clones.borrow().iter().filter_map(|s| s.upgrade()) {
                    clone.set(Span {
                        start: end,
                        length: 0,
                    });
                }
            }
            p.hit("SetText.after");
            p.result("SetText")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_GetFormattedText(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetFormattedText");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_GetEmbedded(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *const windows_core::GUID,
    a3: *const windows_core::GUID,
    a4: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetEmbedded");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_InsertEmbedded(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u32,
    a3: *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("InsertEmbedded");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_ShiftStart(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: i32,
    a3: *mut i32,
    a4: *const TF_HALTCOND,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ShiftStart");
            let d = p.document();
            let range = p.span();
            let old = range.get();
            let end = old.start + old.length;
            let point = old.start;
            let limit = d.shift_start_limit.get();
            let shift = a2.clamp(
                -point.min(limit),
                (d.text.borrow().len() as i32 - point).min(limit),
            );
            a3.write(shift);
            let point = point + shift;
            range.set(Span {
                start: point,
                length: (end - point).max(0),
            });
            p.hit("ShiftStart.after");
            p.result("ShiftStart")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_ShiftEnd(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: i32,
    a3: *mut i32,
    a4: *const TF_HALTCOND,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ShiftEnd");
            let d = p.document();
            let range = p.span();
            let old = range.get();
            let end = old.start + old.length;
            let point = end;
            let limit = d.shift_end_limit.get();
            let shift = a2.clamp(
                -point.min(limit),
                (d.text.borrow().len() as i32 - point).min(limit),
            );
            a3.write(shift);
            let point = point + shift;
            range.set(Span {
                start: old.start.min(point),
                length: point - old.start.min(point),
            });
            p.hit("ShiftEnd.after");
            p.result("ShiftEnd")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_ShiftStartToRange(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: TfAnchor,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ShiftStartToRange");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_ShiftEndToRange(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: TfAnchor,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ShiftEndToRange");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_ShiftStartRegion(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: TfShiftDir,
    a3: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ShiftStartRegion");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_ShiftEndRegion(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: TfShiftDir,
    a3: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("ShiftEndRegion");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_IsEmpty(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("IsEmpty");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_Collapse(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: TfAnchor,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Collapse");
            let range = p.span();
            let old = range.get();
            range.set(Span {
                start: old.start + if a2 == TF_ANCHOR_END { old.length } else { 0 },
                length: 0,
            });
            p.result("Collapse")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_IsEqualStart(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: TfAnchor,
    a4: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("IsEqualStart");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_IsEqualEnd(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: TfAnchor,
    a4: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("IsEqualEnd");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_CompareStart(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: TfAnchor,
    a4: *mut i32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("CompareStart");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_CompareEnd(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: *mut core::ffi::c_void,
    a3: TfAnchor,
    a4: *mut i32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("CompareEnd");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_AdjustForInsert(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: u32,
    a3: *mut windows_core::BOOL,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("AdjustForInsert");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_GetGravity(
    this: *mut core::ffi::c_void,
    a1: *mut TfGravity,
    a2: *mut TfGravity,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetGravity");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_SetGravity(
    this: *mut core::ffi::c_void,
    a1: u32,
    a2: TfGravity,
    a3: TfGravity,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SetGravity");
            E_NOTIMPL
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_Clone(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("Clone");
            let span = Rc::new(Cell::new(p.span().get()));
            p.document().clones.borrow_mut().push(Rc::downgrade(&span));
            a1.write(p.range(span));
            p.hit("Clone.after");
            p.result("Clone")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRange_GetContext(
    this: *mut core::ffi::c_void,
    a1: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("range.GetContext");
            let d = p.document();
            let ptr = if d.range_context.get().is_null() {
                d.context.get()
            } else {
                d.range_context.get()
            };
            a1.write(retain(ptr));
            p.result("range.GetContext")
        })
    }
}
pub(super) const ITFRANGEACP_TABLE: ITfRangeACP_Vtbl = ITfRangeACP_Vtbl {
    base__: ITFRANGE_TABLE,
    GetExtent: ITfRangeACP_GetExtent,
    SetExtent: ITfRangeACP_SetExtent,
};
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRangeACP_GetExtent(
    this: *mut core::ffi::c_void,
    a1: *mut i32,
    a2: *mut i32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("GetExtent");
            let span = p.span().get();
            a1.write(span.start);
            a2.write(span.length);
            p.result("GetExtent")
        })
    }
}
// SAFETY: Controlled callers retain live SDK inputs and writable outputs for this call.
// The live COM reference keeps this provider and its host state allocated.
unsafe extern "system" fn ITfRangeACP_SetExtent(
    this: *mut core::ffi::c_void,
    a1: i32,
    a2: i32,
) -> windows_core::HRESULT {
    unsafe {
        boundary(this, |p| {
            p.hit("SetExtent");
            E_NOTIMPL
        })
    }
}
