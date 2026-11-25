//! Package-bound standalone Settings launch from the host's COM apartment.

use keyrill_windows_build::{Target, artifact::Package};
use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE};
use windows::Win32::System::Threading::*;
use windows::Win32::System::WindowsProgramming::DRIVE_FIXED;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};
use windows_core::{HRESULT, PCWSTR, PWSTR, Result};

const TARGET: Target = if cfg!(target_pointer_width = "64") {
    Target::X64
} else {
    Target::X86
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: each instance owns one successful native handle acquisition.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn unavailable() -> windows_core::Error {
    windows_core::Error::from_hresult(HRESULT::from_win32(ERROR_NOT_SUPPORTED.0))
}

fn wide(path: &Path) -> Result<Vec<u16>> {
    Ok(path
        .to_str()
        .ok_or_else(unavailable)?
        .encode_utf16()
        .chain(Some(0))
        .collect())
}

fn module_path() -> Result<PathBuf> {
    let mut module = HMODULE::default();
    // SAFETY: this function's address belongs to the executing runtime module.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(module_path as *const () as *const u16),
            &mut module,
        )?;
    }
    let mut path = [0u16; 32768];
    let count = crate::foreign::win32::module_path(Some(module), &mut path) as usize;
    if count == 0 || count >= path.len() {
        return Err(unavailable());
    }
    Ok(PathBuf::from(
        String::from_utf16(&path[..count]).map_err(|_| unavailable())?,
    ))
}

/// Retains directory and executable identities until CreateProcess has opened the image.
struct Launch {
    executable: PathBuf,
    _leases: Vec<File>,
}

fn elevated() -> Result<bool> {
    let mut token = HANDLE::default();
    // SAFETY: the current process pseudo-handle and output token slot are valid.
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
    }
    let token = Handle(token);
    let mut value = TOKEN_ELEVATION::default();
    let mut length = 0;
    // SAFETY: the exact token-information class and output size are paired.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&mut value as *mut TOKEN_ELEVATION).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )?;
    }
    Ok(value.TokenIsElevated != 0)
}

fn privileged_safe(file: &File) -> Result<()> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let mut owner = PSID::default();
    let mut acl = std::ptr::null_mut();
    // SAFETY: file remains open and the API transfers one LocalFree-owned descriptor.
    unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut acl),
            None,
            Some(&mut descriptor),
        )
        .ok()?;
    }
    struct Descriptor(PSECURITY_DESCRIPTOR);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: GetSecurityInfo transferred this local allocation.
            unsafe {
                LocalFree(Some(HLOCAL(self.0.0)));
            }
        }
    }
    let _descriptor = Descriptor(descriptor);
    let mut system = [0u32; 17];
    let mut administrators = [0u32; 17];
    let mut length = size_of_val(&system) as u32;
    // SAFETY: aligned maximum-SID buffers are paired with their capacities.
    unsafe {
        CreateWellKnownSid(
            WinLocalSystemSid,
            None,
            Some(PSID(system.as_mut_ptr().cast())),
            &mut length,
        )?;
        length = size_of_val(&administrators) as u32;
        CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            None,
            Some(PSID(administrators.as_mut_ptr().cast())),
            &mut length,
        )?;
    }
    let trusted = |sid| {
        // SAFETY: all SIDs belong to live native descriptors or initialized local buffers.
        unsafe {
            EqualSid(sid, PSID(system.as_ptr().cast_mut().cast())).is_ok()
                || EqualSid(sid, PSID(administrators.as_ptr().cast_mut().cast())).is_ok()
        }
    };
    if owner.0.is_null() || !trusted(owner) || acl.is_null() {
        return Err(unavailable());
    }
    // SAFETY: GetSecurityInfo returned a complete ACL retained by the descriptor.
    for index in 0..unsafe { (*acl).AceCount } {
        let mut entry = std::ptr::null_mut();
        // SAFETY: the index is bounded by the native ACL's ACE count.
        unsafe {
            GetAce(acl, index as u32, &mut entry)?;
        }
        // SAFETY: each ACE begins with the documented ACE_HEADER.
        let header = unsafe { &*entry.cast::<ACE_HEADER>() };
        if u32::from(header.AceType) == ACCESS_DENIED_ACE_TYPE {
            continue;
        }
        if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
            return Err(unavailable());
        }
        // SAFETY: the accepted ACE type supplies a mask and inline SID.
        let allowed = unsafe { &*entry.cast::<ACCESS_ALLOWED_ACE>() };
        let write = FILE_WRITE_DATA.0
            | FILE_APPEND_DATA.0
            | FILE_WRITE_EA.0
            | FILE_WRITE_ATTRIBUTES.0
            | FILE_DELETE_CHILD.0
            | DELETE.0
            | WRITE_DAC.0
            | WRITE_OWNER.0
            | GENERIC_ALL.0
            | GENERIC_WRITE.0;
        if allowed.Mask & write != 0
            && !trusted(PSID((&allowed.SidStart as *const u32).cast_mut().cast()))
        {
            return Err(unavailable());
        }
    }
    Ok(())
}

fn retain(path: &Path, directory: bool) -> Result<File> {
    keyrill_windows_build::artifact::retain(path, directory).map_err(|_| unavailable())
}

impl Launch {
    fn open(module: &Path, expected: &str) -> Result<Self> {
        Self::open_for_host(module, expected, elevated()?, |directory| {
            Package::open(directory).map_err(|_| unavailable())
        })
    }

    fn open_for_host(
        module: &Path,
        expected: &str,
        privileged: bool,
        open_package: impl FnOnce(&Path) -> Result<Package>,
    ) -> Result<Self> {
        let architecture = module.parent().ok_or_else(unavailable)?;
        let directory = architecture.parent().ok_or_else(unavailable)?;
        if module
            .file_name()
            .is_none_or(|name| name != "KeyrillTsf.dll")
            || architecture
                .file_name()
                .is_none_or(|name| name != TARGET.name())
        {
            return Err(unavailable());
        }
        let text = module.to_str().ok_or_else(unavailable)?;
        let text = text.strip_prefix(r"\\?\").unwrap_or(text);
        if text.len() < 3
            || text.as_bytes()[1..3] != *b":\\"
            || !text.as_bytes()[0].is_ascii_alphabetic()
        {
            return Err(unavailable());
        }
        let root = wide(Path::new(&text[..3]))?;
        // SAFETY: grammar above establishes a terminated local drive root.
        if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } != DRIVE_FIXED {
            return Err(unavailable());
        }
        let mut leases = Vec::new();
        let mut ancestors: Vec<_> = directory.ancestors().skip(1).collect();
        ancestors.reverse();
        for ancestor in ancestors {
            leases.push(retain(ancestor, true)?);
        }
        let package_root = retain(directory, true)?;
        let architecture_directory = retain(architecture, true)?;
        let manifest = retain(&directory.join("package.json"), false)?;
        let mut dll = retain(module, false)?;
        let executable = architecture.join("KeyrillSettings.exe");
        let mut settings = retain(&executable, false)?;
        if privileged {
            for file in [
                &package_root,
                &architecture_directory,
                &manifest,
                &dll,
                &settings,
            ] {
                privileged_safe(file)?;
            }
        }
        let package = open_package(directory)?;
        if package.production(TARGET) != expected
            || directory
                .file_name()
                .is_none_or(|name| name != package.digest())
        {
            return Err(unavailable());
        }
        for (name, file) in [
            ("KeyrillTsf.dll", &mut dll),
            ("KeyrillSettings.exe", &mut settings),
        ] {
            package
                .verify_file(&format!("{}/{name}", TARGET.name()), file)
                .map_err(|_| unavailable())?;
        }
        leases.extend([
            package_root,
            architecture_directory,
            manifest,
            dll,
            settings,
        ]);
        Ok(Self {
            executable,
            _leases: leases,
        })
    }

    fn start(&self, arguments: &str) -> Result<Child> {
        let executable = wide(&self.executable)?;
        let directory = wide(self.executable.parent().ok_or_else(unavailable)?)?;
        let mut command = format!(
            "\"{}\" {arguments}",
            self.executable.to_str().ok_or_else(unavailable)?
        )
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        // SAFETY: exact application path and mutable terminated command survive synchronous launch.
        unsafe {
            CreateProcessW(
                PCWSTR(executable.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                PROCESS_CREATION_FLAGS::default(),
                None,
                PCWSTR(directory.as_ptr()),
                &startup,
                &mut process,
            )?;
        }
        Ok(Child {
            process: Handle(process.hProcess),
            _thread: Handle(process.hThread),
            #[cfg(test)]
            thread_id: process.dwThreadId,
        })
    }
}

struct Child {
    process: Handle,
    _thread: Handle,
    #[cfg(test)]
    thread_id: u32,
}

impl Child {
    fn wait(self) -> Result<()> {
        // SAFETY: retained child process handle remains live through COM-aware modal waiting.
        unsafe {
            CoWaitForMultipleHandles(
                (COWAIT_DISPATCH_CALLS | COWAIT_DISPATCH_WINDOW_MESSAGES).0 as u32,
                INFINITE,
                &[self.process.0],
            )?;
        }
        let mut exit = 0;
        // SAFETY: a signaled process has one stable exit status and output is writable.
        unsafe {
            GetExitCodeProcess(self.process.0, &mut exit)?;
        }
        if exit != 0 {
            return Err(windows_core::Error::from_hresult(E_FAIL));
        }
        Ok(())
    }
}

pub(crate) fn show(parent: HWND) -> Result<()> {
    // SAFETY: a non-null optional owner is validated without sending it a message.
    if !parent.is_invalid() && !unsafe { IsWindow(Some(parent)) }.as_bool() {
        return Err(windows_core::Error::from_hresult(E_INVALIDARG));
    }
    let expected = crate::build_identity::BUILD_DIGEST
        .iter()
        .map(|word| format!("{word:08x}"))
        .collect::<String>();
    let arguments = if parent.is_invalid() {
        String::new()
    } else {
        let mut process = 0;
        // SAFETY: query only the already validated optional owner, without messaging it.
        let thread = unsafe { GetWindowThreadProcessId(parent, Some(&mut process)) };
        if thread == 0 || process == 0 {
            return Err(windows_core::Error::from_hresult(E_INVALIDARG));
        }
        format!("--owner {} {process} {thread}", parent.0 as usize)
    };
    Launch::open(&module_path()?, &expected)?
        .start(&arguments)?
        .wait()
}

#[cfg(test)]
#[path = "configure_tests.rs"]
mod tests;
