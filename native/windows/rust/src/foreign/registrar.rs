use super::{Call, Lease, returned_interface};
use crate::profiles::{Profile, SERVICE_ID};
use std::ffi::c_void;
use windows::Win32::Foundation::{E_UNEXPECTED, S_OK};
use windows::Win32::UI::TextServices::{
    CLSID_TF_InputProcessorProfiles, IEnumTfLanguageProfiles, ITfInputProcessorProfiles,
    TF_LANGUAGEPROFILE,
};
use windows_core::{BOOL, GUID, HRESULT, Interface};

unsafe fn sdk_create(output: *mut *mut c_void) -> Call {
    // SAFETY: the typed caller retains all interfaces and buffers through this ABI call.
    windows_core::link!("ole32.dll" "system" fn CoCreateInstance(class: *const GUID, outer: *mut c_void, context: u32, id: *const GUID, output: *mut *mut c_void) -> HRESULT);
    let (class, id) = (
        &CLSID_TF_InputProcessorProfiles,
        &ITfInputProcessorProfiles::IID,
    );
    Call {
        result: unsafe { CoCreateInstance(class, std::ptr::null_mut(), 1, id, output) },
    }
}

fn pointer<T: Interface>(value: &Lease<T>) -> *mut c_void {
    value.address() as *mut c_void
}

fn acquired<T: Interface>(call: Call, output: *mut c_void) -> (Call, Result<Lease<T>, HRESULT>) {
    let returned = returned_interface::<T>(call, output);
    if call.result != S_OK {
        let result = if call.result.is_err() {
            call.result
        } else {
            E_UNEXPECTED
        };
        return (call, Err(result));
    }
    (call, returned.value.ok_or(E_UNEXPECTED))
}

pub(crate) fn profiles() -> (Call, Result<Lease<ITfInputProcessorProfiles>, HRESULT>) {
    let mut output = std::ptr::null_mut();
    // SAFETY: the selected class returns ITfInputProcessorProfiles into a writable pointer slot.
    let call = unsafe { sdk_create(&mut output) };
    acquired(call, output)
}

pub(crate) fn entries(
    manager: &Lease<ITfInputProcessorProfiles>,
    language: u16,
) -> (Call, Result<Lease<IEnumTfLanguageProfiles>, HRESULT>) {
    let mut output = std::ptr::null_mut();
    // SAFETY: the lease retains the manager and output is a writable enumerator result slot.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfInputProcessorProfiles>(pointer(manager)).EnumLanguageProfiles)(
                pointer(manager),
                language,
                &mut output,
            )
        },
    };
    acquired(call, output)
}

pub(crate) fn next(
    entries: &Lease<IEnumTfLanguageProfiles>,
    entry: &mut TF_LANGUAGEPROFILE,
    fetched: &mut u32,
) -> Call {
    // SAFETY: the lease retains the enumerator and both outputs are distinct writable storage.
    Call {
        result: unsafe {
            (super::vtable::<IEnumTfLanguageProfiles>(pointer(entries)).Next)(
                pointer(entries),
                1,
                entry,
                fetched,
            )
        },
    }
}

pub(crate) fn description(
    manager: &Lease<ITfInputProcessorProfiles>,
    profile: &Profile,
    output: &mut *const u16,
) -> Call {
    // SAFETY: manager and immutable GUIDs are live and output is a writable BSTR result slot.
    Call {
        result: unsafe {
            (super::vtable::<ITfInputProcessorProfiles>(pointer(manager))
                .GetLanguageProfileDescription)(
                pointer(manager),
                &SERVICE_ID,
                profile.language,
                &profile.id,
                (output as *mut *const u16).cast(),
            )
        },
    }
}

pub(crate) fn enable(
    manager: &Lease<ITfInputProcessorProfiles>,
    profile: &Profile,
    enabled: bool,
) -> Call {
    // SAFETY: the lease and immutable GUIDs remain live for the complete call.
    Call {
        result: unsafe {
            (super::vtable::<ITfInputProcessorProfiles>(pointer(manager)).EnableLanguageProfile)(
                pointer(manager),
                &SERVICE_ID,
                profile.language,
                &profile.id,
                BOOL::from(enabled),
            )
        },
    }
}

pub(crate) fn enabled(
    manager: &Lease<ITfInputProcessorProfiles>,
    profile: &Profile,
    output: &mut BOOL,
) -> Call {
    // SAFETY: manager and GUIDs are live and output is a writable BOOL result slot.
    Call {
        result: unsafe {
            (super::vtable::<ITfInputProcessorProfiles>(pointer(manager)).IsEnabledLanguageProfile)(
                pointer(manager),
                &SERVICE_ID,
                profile.language,
                &profile.id,
                output,
            )
        },
    }
}

#[cfg(any(test, feature = "test-api"))]
fn description_buffer(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(feature = "test-api")]
pub(crate) fn add(
    manager: &Lease<ITfInputProcessorProfiles>,
    profile: &Profile,
    dll: &[u16],
) -> Call {
    let description = description_buffer(profile.description);
    // SAFETY: buffers/GUIDs are live, and lengths fit u32 because CLI paths are capped at 32768 units.
    Call {
        result: unsafe {
            (super::vtable::<ITfInputProcessorProfiles>(pointer(manager)).AddLanguageProfile)(
                pointer(manager),
                &SERVICE_ID,
                profile.language,
                &profile.id,
                windows_core::PCWSTR(description.as_ptr()),
                (description.len() - 1) as u32,
                windows_core::PCWSTR(dll.as_ptr()),
                (dll.len() - 1) as u32,
                0,
            )
        },
    }
}

#[cfg(test)]
mod tests {
    use super::description_buffer;
    use crate::profiles::PROFILES;

    #[test]
    fn profile_description_storage_has_a_trailing_nul() {
        for (profile, expected) in PROFILES
            .iter()
            .zip(["Keyrill English\0", "Keyrill Русский\0"])
        {
            assert_eq!(
                description_buffer(profile.description),
                expected.encode_utf16().collect::<Vec<_>>()
            );
        }
        assert_eq!(description_buffer(""), [0]);
        assert_eq!(description_buffer("Я🔑"), [0x42f, 0xd83d, 0xdd11, 0]);
    }
}
