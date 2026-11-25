//! The only owner of host COM references. Generated ownership helpers stay out of this module too.

pub(crate) mod document;
pub(crate) mod registrar;
pub(crate) mod tsf;
pub(crate) mod win32;

use crate::field_contract::{Facts, HostIdentity};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;
use windows::Win32::Foundation::{E_POINTER, E_UNEXPECTED, HWND, S_OK};
use windows::Win32::UI::TextServices::{ITfContext, ITfEditSession};
use windows_core::{HRESULT, IUnknown, Interface};

unsafe fn vtable<'a, T: Interface>(pointer: *mut c_void) -> &'a T::Vtable {
    // SAFETY: the caller supplies a live T whose first pointer is its SDK vtable.
    unsafe { &**pointer.cast::<*const T::Vtable>() }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Call {
    pub(crate) result: HRESULT,
}

impl Call {
    pub(crate) fn exact(self) -> bool {
        self.result == S_OK
    }
}

pub(crate) fn keyboard_state(output: &mut [u8; 256]) -> bool {
    windows_core::link!("user32.dll" "system" fn GetKeyboardState(output: *mut u8) -> windows_core::BOOL);
    // The pinned projection derives its writable pointer from as_ptr(), which loses mutable
    // provenance. Keep this output mutable through the actual Win32 call.
    // SAFETY: output is a complete writable keyboard snapshot retained through the call.
    unsafe { GetKeyboardState(output.as_mut_ptr()).as_bool() }
}

#[cfg(test)]
mod keyboard_tests {
    use super::keyboard_state;
    use windows::Win32::UI::Input::KeyboardAndMouse::SetKeyboardState;

    struct Restore([u8; 256]);
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: this restores only the test thread's complete original keyboard snapshot.
            let _ = unsafe { SetKeyboardState(&self.0) };
        }
    }

    #[test]
    fn keyboard_output_retains_mutable_provenance() {
        let mut original = [0; 256];
        assert!(keyboard_state(&mut original));
        let restore = Restore(original);
        let mut pressed = restore.0;
        pressed[0xa4] = 0x80;
        pressed[0xa5] = 0;
        // SAFETY: the complete input snapshot changes only this test thread's key-state table.
        unsafe { SetKeyboardState(&pressed).unwrap() };
        let mut observed = [0; 256];
        assert!(keyboard_state(&mut observed));
        assert_eq!(observed[0xa4], 0x80);
        assert!(observed[0xa4] & 0x80 != 0);
        assert_eq!(observed[0xa5] & 0x80, 0);
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum InterfaceError {
    Native(Call),
    Missing,
}

impl InterfaceError {
    pub(crate) fn call(self) -> Call {
        match self {
            Self::Native(call) => call,
            Self::Missing => Call { result: S_OK },
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Dependencies {
    pub(crate) size: u32,
    pub(crate) version: u32,
    pub(crate) context: *mut c_void,
    pub(crate) retain: Option<unsafe extern "system" fn(*mut c_void)>,
    pub(crate) release: Option<unsafe extern "system" fn(*mut c_void)>,
    pub(crate) profiles: *mut c_void,
    pub(crate) layouts: Option<unsafe extern "system" fn(i32, *mut isize) -> i32>,
    pub(crate) capture: Option<
        unsafe extern "system" fn(*mut c_void, HWND, *const HostIdentity, *mut Facts) -> HRESULT,
    >,
}

const _: () = {
    assert!(size_of::<Dependencies>() == 8 + 6 * size_of::<usize>());
    assert!(align_of::<Dependencies>() == align_of::<usize>());
    assert!(std::mem::offset_of!(Dependencies, capture) == 8 + 5 * size_of::<usize>());
};

pub(crate) struct DependencyLease {
    pub(crate) table: Dependencies,
}

impl DependencyLease {
    #[cfg(feature = "test-api")]
    pub(crate) fn new(table: Dependencies) -> Self {
        if let Some(retain) = table.retain {
            // SAFETY: the dependency owner retains callbacks and context through release.
            unsafe { retain(table.context) };
        }
        Self { table }
    }
}

impl Drop for DependencyLease {
    fn drop(&mut self) {
        if let Some(release) = self.table.release {
            // SAFETY: this is the one release paired with the successful dependency acquisition.
            unsafe { release(self.table.context) };
        }
    }
}

pub(crate) fn pin_current_module_for_process_lifetime() {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    };
    let mut module = HMODULE::default();
    // SAFETY: FROM_ADDRESS interprets this function address as code. PIN keeps its DLL mapped.
    let _ = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            windows_core::PCWSTR(
                pin_current_module_for_process_lifetime as *const () as *const u16,
            ),
            &mut module,
        )
    };
}

struct Owner {
    pointer: NonNull<c_void>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        // SAFETY: this owner holds exactly one live reference and surrenders it once.
        unsafe { (vtable::<IUnknown>(self.pointer.as_ptr()).Release)(self.pointer.as_ptr()) };
    }
}

pub(crate) struct Lease<T: Interface> {
    owner: Rc<Owner>,
    interface: PhantomData<T>,
}

impl<T: Interface> Clone for Lease<T> {
    fn clone(&self) -> Self {
        Self {
            owner: Rc::clone(&self.owner),
            interface: PhantomData,
        }
    }
}

impl<T: Interface> Lease<T> {
    pub(crate) fn address(&self) -> usize {
        self.owner.pointer.as_ptr() as usize
    }

    pub(crate) fn query<U: Interface>(&self) -> Result<Lease<U>, InterfaceError> {
        // SAFETY: this Rust lease retains the host across the complete acquisition.
        unsafe { query(self.owner.pointer.as_ptr()) }
    }
}

/// Take exactly one live reference transferred by a normally completed typed SDK call.
/// The caller separately validates its HRESULT before using the object as native evidence.
pub(crate) unsafe fn adopt<T: Interface>(pointer: *mut c_void) -> Result<Lease<T>, Call> {
    let Some(pointer) = NonNull::new(pointer) else {
        return Err(Call {
            result: E_UNEXPECTED,
        });
    };
    Ok(Lease {
        owner: Rc::new(Owner { pointer }),
        interface: PhantomData,
    })
}

pub(crate) struct Returned<T> {
    pub(crate) call: Call,
    pub(crate) value: T,
}

fn returned_interface<T: Interface>(
    call: Call,
    pointer: *mut c_void,
) -> Returned<Option<Lease<T>>> {
    let value = if pointer.is_null() {
        None
    } else {
        // SAFETY: a COM output transfers its reference even when the caller rejects the status.
        unsafe { adopt(pointer) }.ok()
    };
    Returned { call, value }
}

/// Acquire a reference from an inbound pointer while its caller retains it.
pub(crate) unsafe fn retain<T: Interface>(pointer: *mut c_void) -> Result<Lease<T>, Call> {
    let Some(pointer) = NonNull::new(pointer) else {
        return Err(Call { result: E_POINTER });
    };
    // SAFETY: the inbound caller retains this interface until its callback returns.
    unsafe { (vtable::<IUnknown>(pointer.as_ptr()).AddRef)(pointer.as_ptr()) };
    Ok(Lease {
        owner: Rc::new(Owner { pointer }),
        interface: PhantomData,
    })
}

pub(crate) unsafe fn query<T: Interface>(pointer: *mut c_void) -> Result<Lease<T>, InterfaceError> {
    if pointer.is_null() {
        return Err(InterfaceError::Native(Call { result: E_POINTER }));
    }
    let mut output = std::ptr::null_mut();
    // SAFETY: the caller retains the live host and output is a distinct writable pointer slot.
    let result = Call {
        result: unsafe {
            (vtable::<IUnknown>(pointer).QueryInterface)(pointer, &T::IID, &mut output)
        },
    };
    if !result.exact() {
        drop(returned_interface::<T>(result, output).value);
        return Err(InterfaceError::Native(result));
    }
    if output.is_null() {
        return Err(InterfaceError::Missing);
    }
    // SAFETY: exact S_OK transfers one reference. Null output is rejected by adopt.
    unsafe { adopt(output) }.map_err(InterfaceError::Native)
}

pub(crate) fn request(
    context: &Lease<ITfContext>,
    client: u32,
    session: &ITfEditSession,
    flags: u32,
) -> (Call, HRESULT) {
    let mut session_result = E_UNEXPECTED;
    // SAFETY: local Rust COM and the lease retain both objects throughout this synchronous call.
    let result = unsafe {
        (vtable::<ITfContext>(context.owner.pointer.as_ptr()).RequestEditSession)(
            context.owner.pointer.as_ptr(),
            client,
            session.as_raw(),
            windows::Win32::UI::TextServices::TF_CONTEXT_EDIT_CONTEXT_FLAGS(flags),
            &mut session_result,
        )
    };
    (Call { result }, session_result)
}
