//! Native current-host identity, acquired by the vocabulary worker before policy publication.

use crate::settings::{AppPolicy, ApplicationIdentity, Preferences, SettingsError};
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
use windows::Win32::System::Threading::{
    GetCurrentProcess, PROCESS_NAME_NATIVE, PROCESS_NAME_WIN32, QueryFullProcessImageNameW,
};
use windows_core::{PCWSTR, PWSTR};

pub(crate) fn current() -> Result<Vec<ApplicationIdentity>, SettingsError> {
    let mut path = [0; 4096];
    let mut length = path.len() as u32;
    // SAFETY: the pseudo handle is the current host; the output span is complete and writable.
    unsafe {
        QueryFullProcessImageNameW(
            GetCurrentProcess(),
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    }
    .map_err(|_| SettingsError::Unavailable)?;
    if length == 0 || length as usize >= path.len() {
        return Err(SettingsError::Unavailable);
    }
    let image = String::from_utf16(&path[..length as usize]).map_err(|_| SettingsError::Invalid)?;
    crate::settings::LocalPath::new(&image)?;
    let drive = [path[0], b':' as u16, 0];
    let root = [path[0], b':' as u16, b'\\' as u16, 0];
    // SAFETY: both names are bounded terminated strings; these calls query device metadata only.
    if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) }
        != windows::Win32::System::WindowsProgramming::DRIVE_FIXED
    {
        return Err(SettingsError::Unavailable);
    }
    let mut device = [0; 512];
    let count = unsafe { QueryDosDeviceW(PCWSTR(drive.as_ptr()), Some(&mut device)) } as usize;
    if count == 0 || count >= device.len() {
        return Err(SettingsError::Unavailable);
    }
    let end = device
        .iter()
        .position(|unit| *unit == 0)
        .ok_or(SettingsError::Invalid)?;
    let device = String::from_utf16(&device[..end]).map_err(|_| SettingsError::Invalid)?;
    if !device
        .strip_prefix(r"\Device\HarddiskVolume")
        .is_some_and(|volume| {
            !volume.is_empty() && volume.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(SettingsError::Unavailable);
    }
    length = path.len() as u32;
    // SAFETY: the current process and complete native image-name output remain live for this query.
    unsafe {
        QueryFullProcessImageNameW(
            GetCurrentProcess(),
            PROCESS_NAME_NATIVE,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    }
    .map_err(|_| SettingsError::Unavailable)?;
    if length == 0 || length as usize >= path.len() {
        return Err(SettingsError::Unavailable);
    }
    let native =
        String::from_utf16(&path[..length as usize]).map_err(|_| SettingsError::Invalid)?;
    let suffix = native
        .strip_prefix(&device)
        .filter(|suffix| suffix.starts_with('\\'))
        .ok_or(SettingsError::Unavailable)?;
    let canonical = format!(
        "{}:{suffix}",
        char::from_u32(u32::from(drive[0])).ok_or(SettingsError::Invalid)?
    );
    let mut identities = vec![ApplicationIdentity::executable(&canonical)?];
    let mut family = [0; 256];
    let mut length = family.len() as u32;
    // SAFETY: the complete output span remains writable throughout this scalar native call.
    let result =
        unsafe { GetCurrentPackageFamilyName(&mut length, Some(PWSTR(family.as_mut_ptr()))) };
    if result == ERROR_SUCCESS && length > 1 && length as usize <= family.len() {
        let family = String::from_utf16(&family[..length as usize - 1])
            .map_err(|_| SettingsError::Invalid)?;
        identities.push(ApplicationIdentity::package_family(&family)?);
    } else if result != APPMODEL_ERROR_NO_PACKAGE {
        return Err(SettingsError::Unavailable);
    }
    Ok(identities)
}

pub(crate) fn policy(
    identities: &Result<Vec<ApplicationIdentity>, SettingsError>,
    preferences: &Preferences,
) -> AppPolicy {
    let Ok(identities) = identities else {
        return AppPolicy::Off;
    };
    identities
        .iter()
        .filter_map(|identity| preferences.applications.get(identity).copied())
        .max_by_key(|policy| match policy {
            AppPolicy::Automatic => 0,
            AppPolicy::Manual => 1,
            AppPolicy::Off => 2,
        })
        .unwrap_or_else(|| {
            let manual_default = identities.iter().any(|identity| {
                let name = identity.as_str();
                name.rsplit('\\').next().is_some_and(|basename| {
                    matches!(
                        basename,
                        "CODE.EXE" | "CODE - INSIDERS.EXE" | "WINWORD.EXE" | "WINDOWSTERMINAL.EXE"
                    )
                }) || name.starts_with("microsoft.windowsterminal_")
                    || name.starts_with("microsoft.windowsterminalpreview_")
            });
            if manual_default {
                AppPolicy::Manual
            } else {
                AppPolicy::Automatic
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_defaults_only_restrict_and_explicit_override_can_opt_in() {
        for identity in [
            ApplicationIdentity::executable(r"C:\Apps\Code.exe").unwrap(),
            ApplicationIdentity::executable(r"C:\Apps\Code - Insiders.exe").unwrap(),
            ApplicationIdentity::executable(r"C:\Apps\WINWORD.EXE").unwrap(),
            ApplicationIdentity::package_family("Microsoft.WindowsTerminal_8wekyb3d8bbwe").unwrap(),
        ] {
            let identities = Ok(vec![identity.clone()]);
            let mut preferences = Preferences::default();
            assert_eq!(policy(&identities, &preferences), AppPolicy::Manual);
            preferences
                .applications
                .insert(identity, AppPolicy::Automatic);
            assert_eq!(policy(&identities, &preferences), AppPolicy::Automatic);
        }
    }

    #[test]
    fn both_native_identities_reduce_policy_and_failure_excludes() {
        let identities = current().expect("current process metadata");
        assert!(!identities.is_empty());
        let executable = identities[0].clone();
        let package = ApplicationIdentity::package_family("Example.Editor_123456789abcd").unwrap();
        let both = Ok(vec![executable.clone(), package.clone()]);
        let mut preferences = Preferences::default();
        assert_eq!(policy(&both, &preferences), AppPolicy::Automatic);
        preferences
            .applications
            .insert(executable.clone(), AppPolicy::Manual);
        preferences
            .applications
            .insert(package.clone(), AppPolicy::Automatic);
        assert_eq!(policy(&both, &preferences), AppPolicy::Manual);
        preferences.applications.insert(package, AppPolicy::Off);
        assert_eq!(policy(&both, &preferences), AppPolicy::Off);
        assert_eq!(
            policy(&Err(SettingsError::Unavailable), &preferences),
            AppPolicy::Off
        );
    }
}
