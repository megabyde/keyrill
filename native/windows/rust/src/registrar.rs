//! Maintainer source controls and delegation to the recoverable machine installer.

#[cfg(feature = "test-api")]
use crate::foreign;
use crate::foreign::{Call, Lease, registrar as native};
use crate::profiles::{PROFILES, Profile, SERVICE_ID};
#[cfg(feature = "test-api")]
use std::ffi::c_void;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::{
    E_FAIL, E_UNEXPECTED, ERROR_FILE_NOT_FOUND, ERROR_NOT_FOUND, ERROR_PATH_NOT_FOUND,
    ERROR_SUCCESS, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW,
};
use windows::Win32::UI::TextServices::{ITfInputProcessorProfiles, TF_LANGUAGEPROFILE};
use windows_core::{BOOL, BSTR, HRESULT, Interface, PCWSTR};

enum Command {
    Status,
    Check,
    Register(OsString),
    Unregister,
    Enable,
    Disable,
}

fn command(args: &[OsString]) -> Option<Command> {
    match args {
        [name] if name == "status" => Some(Command::Status),
        [name] if name == "check" => Some(Command::Check),
        [name, flag, scope] if name == "unregister" && flag == "--scope" && scope == "machine" => {
            Some(Command::Unregister)
        }
        [name] if name == "enable" => Some(Command::Enable),
        [name] if name == "disable" => Some(Command::Disable),
        [name, flag, scope, path]
            if name == "register" && flag == "--scope" && scope == "machine" =>
        {
            Some(Command::Register(path.clone()))
        }
        _ => None,
    }
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn label(profile: &Profile) -> &'static str {
    if profile.mode == 1 { "EN" } else { "RU" }
}

fn report(stage: &str, result: HRESULT, profile: &str) -> HRESULT {
    println!("{stage} [{profile}]: 0x{:08x}.", result.0 as u32);
    let _ = std::io::stdout().flush();
    result
}

fn exact(result: HRESULT) -> Result<(), HRESULT> {
    if result == S_OK {
        Ok(())
    } else if result.is_err() {
        Err(result)
    } else {
        Err(E_UNEXPECTED)
    }
}

#[cfg(test)]
fn win32_failure(result: HRESULT) -> HRESULT {
    if result.is_err() { result } else { E_FAIL }
}

fn acquisition<T: Interface>(
    stage: &str,
    (call, value): (Call, Result<Lease<T>, HRESULT>),
) -> Result<Lease<T>, HRESULT> {
    let _ = report(stage, call.result, "service");
    value
}
fn presence(manager: &Lease<ITfInputProcessorProfiles>, profile: &Profile) -> HRESULT {
    let (call, entries) = native::entries(manager, profile.language);
    let _ = report("EnumLanguageProfiles", call.result, label(profile));
    let entries = match entries {
        Ok(entries) => entries,
        Err(result) => return result,
    };
    loop {
        let mut entry = TF_LANGUAGEPROFILE::default();
        let mut fetched = 0;
        let result = native::next(&entries, &mut entry, &mut fetched).result;
        if result == S_FALSE && fetched == 0 {
            return report("Profile presence", S_FALSE, label(profile));
        }
        if result != S_OK || fetched != 1 {
            return report(
                "IEnumTfLanguageProfiles::Next",
                if result.is_err() {
                    result
                } else {
                    E_UNEXPECTED
                },
                label(profile),
            );
        }
        if entry.clsid != SERVICE_ID
            || entry.langid != profile.language
            || entry.guidProfile != profile.id
        {
            continue;
        }
        let mut description = std::ptr::null();
        let call = native::description(manager, profile, &mut description);
        let result = report("GetLanguageProfileDescription", call.result, label(profile));

        // SAFETY: a normally returned BSTR is caller-owned, including on a failed HRESULT.
        let description = unsafe { BSTR::from_raw(description) };
        if result.is_err() {
            return result;
        }
        if result != S_OK
            || !description
                .iter()
                .copied()
                .eq(profile.description.encode_utf16())
        {
            return report(
                "Profile description mismatch or unavailable",
                E_UNEXPECTED,
                label(profile),
            );
        }
        return report("Profile presence", S_OK, label(profile));
    }
}

fn preflight(manager: &Lease<ITfInputProcessorProfiles>) -> HRESULT {
    let mut result = S_OK;
    for profile in &PROFILES {
        let present = presence(manager, profile);
        if present != S_OK && result.is_ok() {
            result = if present == S_FALSE {
                ERROR_NOT_FOUND.to_hresult()
            } else {
                present
            };
        }
    }
    report("Both-profile presence preflight", result, "service")
}

fn enable_profiles(manager: &Lease<ITfInputProcessorProfiles>, enabled: bool) -> HRESULT {
    let result = preflight(manager);
    if result.is_err() {
        return result;
    }
    for profile in &PROFILES {
        let call = native::enable(manager, profile, enabled);
        let result = report("EnableLanguageProfile", call.result, label(profile));
        if let Err(result) = exact(result) {
            return report(
                "EnableLanguageProfile unexpected result",
                result,
                label(profile),
            );
        }
    }
    let result = preflight(manager);
    if result.is_err() {
        return result;
    }
    for profile in &PROFILES {
        let mut actual = BOOL(0);
        let call = native::enabled(manager, profile, &mut actual);
        let result = report(
            "IsEnabledLanguageProfile readback",
            call.result,
            label(profile),
        );
        if let Err(result) = exact(result) {
            return report(
                "Enablement readback unexpected result",
                result,
                label(profile),
            );
        }
        if actual.as_bool() != enabled {
            return report("Enablement readback mismatch", E_FAIL, label(profile));
        }
    }
    S_OK
}

fn registry_path() -> Vec<u16> {
    wide(OsStr::new(&format!(
        "Software\\Classes\\CLSID\\{{{SERVICE_ID:?}}}"
    )))
}

fn status() -> HRESULT {
    let manager: Lease<ITfInputProcessorProfiles> = match acquisition(
        "CoCreateInstance InputProcessorProfiles status",
        native::profiles(),
    ) {
        Ok(manager) => manager,
        Err(result) => return result,
    };
    let mut path = registry_path();
    path.pop();
    path.extend("\\InprocServer32".encode_utf16().chain(Some(0)));
    let mut key = HKEY::default();
    // SAFETY: path is NUL-terminated and key is a distinct writable handle slot.
    let registry = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if !key.is_invalid() {
        // SAFETY: RegOpenKeyExW returned this open key and it is closed once.
        let _ = unsafe { RegCloseKey(key) };
    }
    let absent = registry == ERROR_FILE_NOT_FOUND || registry == ERROR_PATH_NOT_FOUND;
    println!(
        "COM HKCU registration: {}.",
        if registry == ERROR_SUCCESS {
            "present"
        } else if absent {
            "absent"
        } else {
            "unavailable"
        }
    );
    let _ = report(
        "RegOpenKeyExW HKCU COM status",
        registry.to_hresult(),
        "service",
    );
    let mut result = if registry == ERROR_SUCCESS || absent {
        S_OK
    } else {
        registry.to_hresult()
    };
    for profile in &PROFILES {
        let registered = presence(&manager, profile);
        if registered.is_err() {
            result = registered;
        }
        let mut enabled = BOOL(0);
        let available = if registered == S_OK {
            report(
                "IsEnabledLanguageProfile status",
                native::enabled(&manager, profile, &mut enabled).result,
                label(profile),
            )
        } else {
            E_FAIL
        };
        if registered == S_OK
            && let Err(error) = exact(available)
        {
            result = error;
        }
        println!(
            "TSF {} profile: {}; enabled: {}.",
            label(profile),
            if registered == S_OK {
                "present"
            } else if registered == S_FALSE {
                "absent"
            } else {
                "unavailable"
            },
            if registered != S_OK {
                "not applicable"
            } else if available == S_OK {
                if enabled.as_bool() { "yes" } else { "no" }
            } else {
                "unavailable"
            }
        );
    }
    println!(
        "Machine profile definitions and current-user enablement are separate. KeyrillSetup status inspects both COM views. No elevation fallback or source selection."
    );
    result
}

fn enable(enabled: bool, check: bool) -> HRESULT {
    let manager = match acquisition(
        "CoCreateInstance InputProcessorProfiles preflight",
        native::profiles(),
    ) {
        Ok(manager) => manager,
        Err(result) => return result,
    };
    if check {
        preflight(&manager)
    } else {
        enable_profiles(&manager, enabled)
    }
}

/// Runs the maintainer registration CLI and returns its process exit code.
///
/// `status` and `check` are read-only. Installation and enablement require an explicit command.
pub fn run() -> i32 {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let Some(command) = command(&args) else {
        eprintln!(
            "Maintainer only. Usage: KeyrillRegister status | check | register --scope machine VERIFIED_PACKAGE_DIRECTORY | enable | disable | unregister --scope machine"
        );
        return 2;
    };
    if matches!(command, Command::Register(_) | Command::Unregister) {
        let result = (|| -> std::io::Result<std::process::ExitStatus> {
            let setup = std::env::current_exe()?.with_file_name("KeyrillSetup.exe");
            let mut child = std::process::Command::new(setup);
            match command {
                Command::Register(package) => {
                    child.args(["install", "--scope", "machine"]).arg(package);
                }
                Command::Unregister => {
                    child.args(["uninstall", "--scope", "machine"]);
                }
                _ => unreachable!(),
            }
            child.status()
        })();
        return match result {
            Ok(status) if status.success() => 0,
            _ => 1,
        };
    }
    // SAFETY: the CLI initializes its current thread once and balances each successful result.
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let _ = report("CoInitializeEx", initialized, "service");
    if initialized.is_err() {
        return 1;
    }
    let result = match command {
        Command::Status => status(),
        Command::Check => enable(false, true),
        Command::Enable => enable(true, false),
        Command::Disable => enable(false, false),
        Command::Register(_) | Command::Unregister => unreachable!(),
    };
    // SAFETY: all operation-owned COM leases drained before balancing successful initialization.
    unsafe { CoUninitialize() };
    if result.is_err() {
        eprintln!(
            "Operation failed (0x{:08x}). Inspect the stage above and run status before any further change.",
            result.0 as u32
        );
        return 1;
    }
    match command {
        Command::Status => {}
        Command::Check => println!(
            "Both product profiles are present. Read-only check; no enablement or source selection."
        ),
        Command::Register(_) | Command::Unregister => unreachable!(),
        Command::Enable => println!(
            "Both product profiles are present and enablement readback matches enabled. Source selection is manual."
        ),
        Command::Disable => {
            println!("Both product profiles are present and enablement readback matches disabled.")
        }
    }
    0
}

#[cfg(feature = "test-api")]
#[unsafe(no_mangle)]
unsafe extern "system" fn KeyrillTestRegistration(manager: *mut c_void, operation: u32) -> HRESULT {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if operation > 3 {
            return windows::Win32::Foundation::E_INVALIDARG;
        }
        // SAFETY: the SDK test caller retains a live ITfInputProcessorProfiles for this call.
        let manager = match unsafe { foreign::retain::<ITfInputProcessorProfiles>(manager) } {
            Ok(manager) => manager,
            Err(error) => return error.result,
        };
        match operation {
            0 => preflight(&manager),
            1 => enable_profiles(&manager, true),
            2 => enable_profiles(&manager, false),
            _ => {
                let dll = wide(OsStr::new("KeyrillTsf.dll"));
                for profile in &PROFILES {
                    let call = native::add(&manager, profile, &dll);
                    if let Err(result) = exact(call.result) {
                        return result;
                    }
                }
                S_OK
            }
        }
    }))
    .unwrap_or(E_UNEXPECTED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_require_the_existing_exact_arguments() {
        for name in ["status", "check", "enable", "disable"] {
            assert!(command(&[OsString::from(name)]).is_some());
            assert!(command(&[OsString::from(name), OsString::from("extra")]).is_none());
        }
        assert!(command(&[]).is_none());
        assert!(command(&[OsString::from("register")]).is_none());
        assert!(command(&[OsString::from("STATUS")]).is_none());
        let path = OsString::from("verified-package");
        assert!(command(&[OsString::from("register"), path.clone()]).is_none());
        assert!(command(&[OsString::from("unregister")]).is_none());
        assert!(matches!(
            command(&["unregister", "--scope", "machine"].map(OsString::from)),
            Some(Command::Unregister)
        ));
        assert!(command(&["unregister", "--scope", "user"].map(OsString::from)).is_none());
        let Some(Command::Register(actual)) = command(&[
            OsString::from("register"),
            OsString::from("--scope"),
            OsString::from("machine"),
            path.clone(),
        ]) else {
            panic!("register must preserve its package path and explicit scope");
        };
        assert_eq!(actual, path);
    }

    #[test]
    fn registration_uses_the_existing_per_user_service_key() {
        let expected = wide(OsStr::new(
            "Software\\Classes\\CLSID\\{1A746212-84DB-4ACF-88B3-CAAA26352774}",
        ));
        assert_eq!(registry_path(), expected);
    }

    #[test]
    fn failed_win32_call_cannot_report_success_without_last_error() {
        assert_eq!(win32_failure(S_OK), E_FAIL);
        assert_eq!(win32_failure(S_FALSE), E_FAIL);
        assert_eq!(win32_failure(E_UNEXPECTED), E_UNEXPECTED);
    }
}
