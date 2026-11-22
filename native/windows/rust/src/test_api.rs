//! Versioned test transport. All decisions stay in the production service and edit frame.

use crate::com;
use crate::edit::{Effect, Frame};
use crate::foreign;
use crate::service::{OBJECTS, Service, WORKERS};
use crate::{field, field_contract, keys};
use keyrill_ffi::NativeEvent;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::{
    E_INVALIDARG, E_POINTER, E_UNEXPECTED, HWND, LPARAM, S_OK, WPARAM,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfContext, ITfThreadMgr, TF_INPUTPROCESSORPROFILE, TF_PRESERVEDKEY,
};
use windows_core::{BOOL, HRESULT, IUnknown, Interface};

const VERSION: u32 = 3;

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestSourceStatus(
    handle: *mut c_void,
    output: *mut u32,
) -> HRESULT {
    fence(|| {
        if handle.is_null() || output.is_null() {
            return E_POINTER;
        }
        // SAFETY: the controlled caller retains its private handle and writable output throughout.
        let handle = unsafe { &*handle.cast::<Handle>() };
        handle.service.checked(|| {
            // SAFETY: the output is one distinct writable u32 owned by this synchronous call.
            unsafe { output.write(handle.service.source_status()) };
            S_OK
        })
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestBuildDigest(count: u32, output: *mut u32) -> HRESULT {
    fence(|| {
        if output.is_null() {
            return E_POINTER;
        }
        if count != 8 {
            return E_INVALIDARG;
        }
        // SAFETY: the size-checked SDK transport supplies eight writable, aligned u32 words.
        unsafe {
            std::ptr::copy_nonoverlapping(crate::build_identity::BUILD_DIGEST.as_ptr(), output, 8)
        };
        S_OK
    })
}

struct Handle {
    service: Rc<Service>,
    owner: com::OwnServiceLease,
}

#[repr(C)]
pub(crate) struct Probe {
    size: u32,
    version: u32,
    outer: HRESULT,
    callback: HRESULT,
    reserved: u32,
    entered: BOOL,
    completed: BOOL,
    effect: u32,
    identity: usize,
}

fn fence(call: impl FnOnce() -> HRESULT) -> HRESULT {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or(E_UNEXPECTED)
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestCounts(objects: *mut i32, workers: *mut i32) -> HRESULT {
    fence(|| {
        if objects.is_null() || workers.is_null() {
            return E_POINTER;
        }
        // SAFETY: the caller supplies two distinct writable LONG slots.
        unsafe {
            objects.write(OBJECTS.load(Ordering::Acquire) as i32);
            workers.write(WORKERS.load(Ordering::Acquire) as i32);
        }
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestSetKeyTranslation(
    handle: *mut c_void,
    translation: Option<foreign::win32::KeyTranslation>,
) -> HRESULT {
    fence(|| {
        if handle.is_null() {
            return E_POINTER;
        }
        // SAFETY: the caller retains this test handle and translation code until the service drains.
        let handle = unsafe { &*handle.cast::<Handle>() };
        handle.service.checked(|| {
            handle.service.test_key_translation.set(translation);
            S_OK
        })
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestTranslate(
    key: WPARAM,
    flags: LPARAM,
    layout: HKL,
    state: *const u8,
    output: *mut NativeEvent,
    modifier_only: *mut BOOL,
) -> HRESULT {
    fence(|| {
        if output.is_null() || modifier_only.is_null() {
            return E_POINTER;
        }
        // SAFETY: callers provide complete distinct output objects.
        unsafe {
            output.write(NativeEvent::new(6, 0, 1, &[]).expect("empty wire event"));
            modifier_only.write(BOOL(0));
        }
        if state.is_null() {
            return E_POINTER;
        }
        // SAFETY: the SDK test transport supplies exactly 256 readable keyboard-state bytes.
        let state = unsafe { &*state.cast::<[u8; 256]>() };
        match keys::translate(key, flags, layout, state, foreign::win32::KEY_TRANSLATION) {
            Ok(keys::Key::Modifier) => {
                // SAFETY: the validated output remains writable.
                unsafe { modifier_only.write(BOOL(1)) };
            }
            Ok(keys::Key::Event(event)) => {
                // SAFETY: the validated output remains writable.
                unsafe { output.write(event) };
            }
            Err(_) => return E_INVALIDARG,
        }
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestExactCommandModifiers(
    state: *const u8,
    output: *mut BOOL,
) -> HRESULT {
    fence(|| {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies a writable BOOL.
        unsafe { output.write(BOOL(0)) };
        if state.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies 256 readable keyboard-state bytes and a distinct BOOL.
        unsafe {
            output.write(BOOL(
                keys::exact_command_modifiers(&*state.cast::<[u8; 256]>()) as i32,
            ))
        };
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestBinding(
    index: u32,
    id: *mut windows_core::GUID,
    key: *mut TF_PRESERVEDKEY,
) -> HRESULT {
    fence(|| {
        if id.is_null() || key.is_null() {
            return E_POINTER;
        }
        // SAFETY: the SDK caller supplies distinct GUID/key outputs.
        unsafe {
            id.write(windows_core::GUID::zeroed());
            key.write(TF_PRESERVEDKEY::default());
        }
        let Some(binding) = keys::BINDINGS.get(index as usize) else {
            return E_INVALIDARG;
        };
        // SAFETY: both validated outputs remain writable.
        unsafe {
            id.write(binding.id);
            key.write(binding.key);
        }
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestProfileLayout(
    profile: *const TF_INPUTPROCESSORPROFILE,
    mode: u32,
    substitute: HKL,
    output: *mut HKL,
) -> HRESULT {
    fence(|| {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies a writable HKL.
        unsafe { output.write(HKL::default()) };
        if profile.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies a readable SDK profile and a distinct writable output.
        unsafe { output.write(keys::profile_layout(&*profile, mode, substitute)) };
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestCapture(
    window: HWND,
    host: *const field_contract::HostIdentity,
    output: *mut field_contract::Facts,
) -> HRESULT {
    fence(|| {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies a complete writable facts object.
        unsafe { output.write(field_contract::Facts::default()) };
        if host.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies readable host metadata and a distinct writable output.
        unsafe { output.write(field::capture(window, *host)) };
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestKnownNotepad(
    host: *const field_contract::HostIdentity,
    output: *mut BOOL,
) -> HRESULT {
    fence(|| {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies a writable BOOL output.
        unsafe { output.write(BOOL(0)) };
        if host.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies readable host metadata and a distinct writable output.
        unsafe { output.write(BOOL(field::known_notepad(&*host) as i32)) };
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestMatch(
    facts: *const field_contract::Facts,
    identity: *mut field::Identity,
    matched: *mut BOOL,
) -> HRESULT {
    fence(|| {
        if identity.is_null() || matched.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies complete, distinct writable output objects.
        unsafe {
            identity.write(field::Identity::default());
            matched.write(BOOL(0));
        }
        if facts.is_null() {
            return E_POINTER;
        }
        // SAFETY: caller supplies a complete readable facts object.
        if let Some(found) = field::matches(unsafe { &*facts }) {
            // SAFETY: the validated outputs remain distinct and writable.
            unsafe {
                identity.write(found);
                matched.write(BOOL(1));
            }
        }
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestCreate(
    version: u32,
    dependencies: *const foreign::Dependencies,
    handle: *mut *mut c_void,
    output: *mut *mut c_void,
) -> HRESULT {
    fence(|| {
        if handle.is_null() || output.is_null() {
            return E_POINTER;
        }
        // SAFETY: test API callers provide distinct writable pointer slots.
        unsafe {
            handle.write(std::ptr::null_mut());
            output.write(std::ptr::null_mut());
        }
        if version != VERSION {
            return E_INVALIDARG;
        }
        let dependencies = if dependencies.is_null() {
            None
        } else {
            // SAFETY: the test API requires a readable size/version prefix before table validation.
            let size = unsafe { std::ptr::addr_of!((*dependencies).size).read() };
            // SAFETY: version is the second u32 in the required readable prefix.
            let version = unsafe { std::ptr::addr_of!((*dependencies).version).read() };
            if version != VERSION || size as usize != size_of::<foreign::Dependencies>() {
                return E_INVALIDARG;
            }
            // SAFETY: the validated size now requires a complete readable dependency table.
            let table = unsafe { *dependencies };
            if table.retain.is_some() != table.release.is_some() {
                return E_INVALIDARG;
            }
            Some(foreign::DependencyLease::new(table))
        };
        let service = Service::with_dependencies(dependencies);
        let owner = com::create_owned(Rc::clone(&service));
        let output_object = owner.processor();
        let owner = Box::new(Handle { service, owner });
        // SAFETY: outputs transfer one handle and one COM reference to the test caller.
        unsafe {
            handle.write(Box::into_raw(owner).cast());
            output.write(output_object.into_raw());
        }
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestEvent(
    handle: *mut c_void,
    context: *mut c_void,
    event: *const NativeEvent,
    eaten: *mut BOOL,
) -> HRESULT {
    fence(|| {
        if eaten.is_null() {
            return E_POINTER;
        }
        // SAFETY: the test transport supplies a writable BOOL output slot.
        unsafe { eaten.write(BOOL(0)) };
        if handle.is_null() || context.is_null() || event.is_null() {
            return E_POINTER;
        }
        // SAFETY: the test caller retains its live handle for this synchronous entry.
        let handle = unsafe { &*handle.cast::<Handle>() };
        handle.service.checked(|| {
            handle.service.revoke_release();
            // SAFETY: transport retains the context and complete bounded wire object through this entry.
            let accepted = unsafe {
                handle
                    .service
                    .execute(handle.owner.clone(), context, event.read(), false)
            }
            .eaten;
            // SAFETY: checked writable output belongs to this synchronous caller.
            unsafe { eaten.write(BOOL(accepted as i32)) };
            S_OK
        })
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestMaintain(handle: *mut c_void) -> HRESULT {
    fence(|| {
        if handle.is_null() {
            return E_POINTER;
        }
        // SAFETY: the test caller retains its live handle for this synchronous entry.
        unsafe { &*handle.cast::<Handle>() }.service.maintain()
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestMetadata(
    handle: *mut c_void,
    context: *mut c_void,
    manager: *mut c_void,
    client: u32,
    current: *const u32,
    output: *mut crate::observation::Metadata,
) -> HRESULT {
    const {
        assert!(size_of::<crate::observation::Metadata>() == 40);
    }
    fence(|| {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: the versioned SDK transport supplies a complete writable metadata object.
        unsafe { output.write(crate::observation::Metadata::default()) };
        if handle.is_null() || context.is_null() || manager.is_null() || current.is_null() {
            return E_POINTER;
        }
        // SAFETY: the test caller retains the live handle throughout this synchronous transport.
        let handle = unsafe { &*handle.cast::<Handle>() };
        handle.service.checked(|| {
            let failed = |call: foreign::Call| {
                let metadata = crate::observation::Metadata {
                    reason: crate::observation::Reason::Native,
                    result: call.result.0 as u32,
                    ..crate::observation::Metadata::default()
                };
                // SAFETY: the checked output remains writable until this synchronous call returns.
                unsafe { output.write(metadata) };
                S_OK
            };
            // SAFETY: the SDK caller retains both incoming interfaces throughout capture and observation.
            let context = match unsafe { foreign::retain::<ITfContext>(context) } {
                Ok(context) => context,
                Err(call) => return failed(call),
            };
            // SAFETY: the SDK caller retains this incoming thread manager throughout the call.
            let manager = match unsafe { foreign::retain::<ITfThreadMgr>(manager) } {
                Ok(manager) => manager,
                Err(call) => return failed(call),
            };
            let current = Rc::new(move || {
                // SAFETY: only same-thread, unexpired observation uses this synchronous caller flag.
                unsafe { current.read() != 0 }
            });
            let observed = crate::host_metadata::observe(
                handle.owner.clone(),
                context,
                manager,
                client,
                current,
            );
            // SAFETY: metadata is a copied content-free value and output remains writable.
            unsafe { output.write(observed.metadata) };
            S_OK
        })
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestRelease(handle: *mut c_void) -> HRESULT {
    fence(|| {
        if handle.is_null() {
            return E_POINTER;
        }
        // SAFETY: the caller retains this live handle until an owner-thread release succeeds.
        let retained = unsafe { &*handle.cast::<Handle>() };
        if !retained.service.owner_thread() {
            return windows::Win32::Foundation::RPC_E_WRONG_THREAD;
        }
        // SAFETY: the test caller surrenders its unique handle exactly once after calls drain.
        drop(unsafe { Box::from_raw(handle.cast::<Handle>()) });
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestPanic(handle: *mut c_void, at: u32) -> HRESULT {
    fence(|| {
        if handle.is_null() {
            return E_POINTER;
        }
        // SAFETY: test transport keeps this handle live until Release.
        let handle = unsafe { &*handle.cast::<Handle>() };
        handle.service.checked(|| {
            handle.service.arm_panic(at);
            S_OK
        })
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestStopped(handle: *mut c_void, output: *mut BOOL) -> HRESULT {
    fence(|| {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: output is a writable BOOL under the test API contract.
        unsafe { output.write(BOOL(0)) };
        if handle.is_null() {
            return E_POINTER;
        }
        // SAFETY: test transport keeps this handle live until Release.
        let handle = unsafe { &*handle.cast::<Handle>() };
        if !handle.service.owner_thread() {
            return windows::Win32::Foundation::RPC_E_WRONG_THREAD;
        }
        // SAFETY: output is initialized even when the service has been poisoned.
        unsafe { output.write(BOOL(handle.service.stopped() as i32)) };
        S_OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestProbe(
    handle: *mut c_void,
    host: *mut c_void,
    operation: u32,
    size: u32,
    output: *mut Probe,
) -> HRESULT {
    fence(|| {
        if output.is_null() {
            return E_POINTER;
        }
        if size as usize != size_of::<Probe>() {
            return E_INVALIDARG;
        }
        // SAFETY: size-checked output is a distinct writable Probe object.
        unsafe {
            output.write(Probe {
                size,
                version: VERSION,
                outer: E_UNEXPECTED,
                callback: E_UNEXPECTED,
                reserved: 0,
                entered: BOOL(0),
                completed: BOOL(0),
                effect: 0,
                identity: 0,
            });
        }
        if handle.is_null() || host.is_null() {
            return E_POINTER;
        }
        // SAFETY: test transport keeps handle and host live throughout the call.
        let handle = unsafe { &*handle.cast::<Handle>() };
        handle.service.checked(|| {
            // SAFETY: output is valid and exclusively used by this synchronous call.
            let output = unsafe { &mut *output };
            if operation == 0 {
                // SAFETY: host is an inbound IUnknown retained by the test caller.
                match unsafe { foreign::query::<IUnknown>(host) } {
                    Ok(identity) => {
                        output.identity = identity.address();
                        output.outer = S_OK;
                    }
                    Err(error) => {
                        let call = error.call();
                        output.outer = if matches!(error, foreign::InterfaceError::Missing) {
                            E_UNEXPECTED
                        } else {
                            call.result
                        };
                    }
                }
            } else if operation == 1 {
                // SAFETY: operation 1 requires an ITfContext pointer retained by the test caller.
                match unsafe { foreign::retain::<ITfContext>(host) } {
                    Ok(context) => {
                        let frame = Frame::new(Rc::clone(&handle.service), handle.owner.clone());
                        let result = frame.request(&context, 42);
                        output.outer = result.outer.result;
                        output.callback = result.callback;

                        output.entered = BOOL(result.entered as i32);
                        output.completed = BOOL(result.completed as i32);
                        output.effect = match result.effect {
                            Effect::Unattempted => 0,
                            Effect::Attempted => 1,
                            Effect::Verified => 2,
                        };
                    }
                    Err(call) => {
                        output.outer = call.result;
                    }
                }
            } else {
                return E_INVALIDARG;
            }
            S_OK
        })
    })
}
