use super::{providers::Provider, schedule::HostCapsule};
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, E_UNEXPECTED, S_OK};
use windows_core::{GUID, HRESULT, IUnknown_Vtbl, Interface};
#[repr(C)]
pub struct Slot<V> {
    pub vtable: *const V,
    pub owner: *const Provider,
}
impl<V> Slot<V> {
    pub const fn new(vtable: &'static V) -> Self {
        Self {
            vtable,
            owner: std::ptr::null(),
        }
    }
}
pub const UNKNOWN: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: query,
    AddRef: add,
    Release: release,
};
// Every slot has the same two-word prefix and points to one stable boxed allocation.
pub unsafe fn owner<'a>(this: *mut c_void) -> &'a Provider {
    unsafe { &*(*this.cast::<Slot<IUnknown_Vtbl>>()).owner }
}
pub fn fence<T>(host: &HostCapsule, fallback: T, call: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(value) => value,
        Err(_) => {
            host.panic_count
                .set(host.panic_count.get().saturating_add(1));
            fallback
        }
    }
}
pub unsafe fn boundary(this: *mut c_void, call: impl FnOnce(&Provider) -> HRESULT) -> HRESULT {
    let p = unsafe { owner(this) };
    fence(&p.host, E_UNEXPECTED, || call(p))
}
unsafe extern "system" fn query(
    this: *mut c_void,
    id: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    unsafe {
        boundary(this, |p| {
            if out.is_null() || id.is_null() {
                return E_POINTER;
            }
            out.write(std::ptr::null_mut());
            p.hit("QI");
            if *id == windows::Win32::UI::TextServices::ITfRangeACP::IID {
                p.hit("range.ACP");
            }
            if matches!(p.kind, super::providers::Kind::Diagnostic(_)) {
                p.hit(if *id == windows_core::IUnknown::IID {
                    "identity"
                } else {
                    "interface"
                });
            }
            let key = if *id == windows::Win32::UI::TextServices::ITfSourceSingle::IID {
                "diagnostic.QI"
            } else if *id == windows::Win32::UI::TextServices::ITfCompartmentMgr::IID {
                "compartment.QI"
            } else if *id == windows::Win32::UI::TextServices::ITfInputScope::IID {
                "scope.QI"
            } else {
                "QI"
            };
            let hr = p.result(key);
            if hr != S_OK {
                return hr;
            }
            if p.null(key) {
                return S_OK;
            }
            let value = p.interface(*id);
            if value.is_null() {
                return E_NOINTERFACE;
            }
            add(value);
            out.write(value);
            p.hit("QI.after");
            S_OK
        })
    }
}
unsafe extern "system" fn add(this: *mut c_void) -> u32 {
    let p = unsafe { owner(this) };
    fence(&p.host, 0, || {
        p.hit("AddRef.before");
        let count = p
            .references
            .get()
            .checked_add(1)
            .expect("reference overflow");
        p.references.set(count);
        p.host.adds.set(p.host.adds.get() + 1);
        p.hit("AddRef.after");
        count
    })
}
unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let pointer = unsafe { (*this.cast::<Slot<IUnknown_Vtbl>>()).owner };
    let host = Rc::clone(&unsafe { &*pointer }.host);
    fence(&host, 0, || {
        let p = unsafe { &*pointer };
        p.hit("Release.before");
        let count = p
            .references
            .get()
            .checked_sub(1)
            .expect("unbalanced release");
        p.references.set(count);
        host.releases.set(host.releases.get() + 1);
        if count == 0 {
            host.final_releases.set(host.final_releases.get() + 1);
            p.hit("Release.final");
            unsafe {
                drop(Box::from_raw(pointer.cast_mut()));
            }
        } else {
            p.hit("Release.after");
        }
        count
    })
}
pub unsafe fn retain(pointer: *mut c_void) -> *mut c_void {
    if !pointer.is_null() {
        unsafe {
            let table = *(pointer.cast::<*const IUnknown_Vtbl>());
            ((*table).AddRef)(pointer);
        }
    }
    pointer
}
pub fn replace_reference(
    slot: &std::cell::RefCell<Option<windows_core::IUnknown>>,
    value: Option<windows_core::IUnknown>,
) {
    let old = slot.replace(value);
    drop(old);
}
