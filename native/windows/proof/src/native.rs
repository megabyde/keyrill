//! Native lifetime and process identity evidence used only by proof tools.
use crate::protocol::Identity;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::Packaging::Appx::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::SystemInformation::*;
use windows::Win32::System::Threading::*;
use windows::core::{PWSTR, Result};

#[cfg(target_pointer_width = "64")]
pub(crate) use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW as window_long, SetWindowLongPtrW as set_window_long,
};

#[cfg(target_pointer_width = "32")]
pub(crate) unsafe fn window_long(
    window: windows::Win32::Foundation::HWND,
    index: windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX,
) -> isize {
    // SAFETY: the caller supplies a valid native index and borrowed window.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(window, index) as isize }
}

#[cfg(target_pointer_width = "32")]
pub(crate) unsafe fn set_window_long(
    window: windows::Win32::Foundation::HWND,
    index: windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX,
    value: isize,
) -> isize {
    // SAFETY: the caller preserves the native contract and both word types are 32 bits.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongW(window, index, value as i32)
            as isize
    }
}

/// Owned kernel handle closed on drop.
pub struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
/// Encode a native null-terminated UTF-16 string.
pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
/// Combine the native FILETIME words without changing their value.
pub fn filetime(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}
/// Read the creation time of a retained process or thread.
///
/// # Errors
/// Returns the first failed native operation or evidence predicate.
pub fn started(handle: HANDLE, thread: bool) -> Result<u64> {
    let (mut creation, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        if thread {
            GetThreadTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user)?;
        } else {
            GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user)?;
        }
    }
    Ok(filetime(creation))
}
/// Supported process architecture observed from a retained native process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessArchitecture {
    /// Native Intel or AMD 64-bit process.
    X64,
    /// Intel or AMD 32-bit process under WOW64.
    X86,
}

impl ProcessArchitecture {
    /// Return the architecture name used by the Windows build runner.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::X86 => "x86",
        }
    }
}

fn classify_architecture(
    machine: IMAGE_FILE_MACHINE,
    native: IMAGE_FILE_MACHINE,
) -> Option<ProcessArchitecture> {
    match (machine, native) {
        (IMAGE_FILE_MACHINE_UNKNOWN, IMAGE_FILE_MACHINE_AMD64) => Some(ProcessArchitecture::X64),
        (IMAGE_FILE_MACHINE_I386, IMAGE_FILE_MACHINE_AMD64) => Some(ProcessArchitecture::X86),
        _ => None,
    }
}

/// Read the process architecture, rejecting unsupported native hosts and emulation.
///
/// # Errors
/// Returns a native query failure or an unsupported architecture.
pub fn process_architecture(process: HANDLE) -> Result<ProcessArchitecture> {
    let (mut machine, mut native) = (IMAGE_FILE_MACHINE_UNKNOWN, IMAGE_FILE_MACHINE_UNKNOWN);
    unsafe {
        IsWow64Process2(process, &mut machine, Some(&mut native))?;
    }
    classify_architecture(machine, native).ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))
}

/// Capture process creation time, executable, and package identity.
///
/// # Errors
/// Returns the first failed native operation or evidence predicate.
pub fn capture_identity(process: HANDLE, pid: u32, thread: u32) -> Result<Identity> {
    let mut identity = Identity {
        kind: 3,
        process: pid,
        thread,
        started: started(process, false)?,
        ..Identity::default()
    };
    let architecture = process_architecture(process)?;
    identity.architecture = match architecture {
        ProcessArchitecture::X64 => 0x8664,
        ProcessArchitecture::X86 => 0x014c,
    };
    let mut image = vec![0; 32768];
    let mut length = image.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(image.as_mut_ptr()),
            &mut length,
        )?;
    }
    let path = String::from_utf16_lossy(&image[..length as usize]);
    let base = path
        .rsplit_once('\\')
        .ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?
        .1;
    if base.eq_ignore_ascii_case("KeyrillProofEditor.exe") {
        identity.kind = 1;
        return Ok(identity);
    }
    if architecture != ProcessArchitecture::X64 || !base.eq_ignore_ascii_case("Notepad.exe") {
        return Ok(identity);
    }
    let mut family = [0; 66];
    let mut length = family.len() as u32;
    if unsafe { GetPackageFamilyName(process, &mut length, Some(PWSTR(family.as_mut_ptr()))) }
        != ERROR_SUCCESS
        || String::from_utf16_lossy(
            &family[..family
                .iter()
                .position(|&word| word == 0)
                .unwrap_or(family.len())],
        ) != "Microsoft.WindowsNotepad_8wekyb3d8bbwe"
    {
        return Ok(identity);
    }
    let mut storage = [0u64; 512];
    let mut size = std::mem::size_of_val(&storage) as u32;
    if unsafe { GetPackageId(process, &mut size, Some(storage.as_mut_ptr().cast())) }
        != ERROR_SUCCESS
    {
        return Ok(identity);
    }
    if (size as usize) < size_of::<PACKAGE_ID>() || size as usize > size_of_val(&storage) {
        return Ok(identity);
    }
    let package = unsafe { &*storage.as_ptr().cast::<PACKAGE_ID>() };
    let start = storage.as_ptr() as usize;
    let end = start + size as usize;
    let name = package.name.0 as usize;
    if package.processorArchitecture != PROCESSOR_ARCHITECTURE_AMD64.0 as u32
        || name < start
        || name >= end
        || !name.is_multiple_of(align_of::<u16>())
    {
        return Ok(identity);
    }
    let units = unsafe { std::slice::from_raw_parts(package.name.0, (end - name) / 2) };
    let Some(length) = units.iter().position(|unit| *unit == 0) else {
        return Ok(identity);
    };
    if !units[..length]
        .iter()
        .copied()
        .eq("Microsoft.WindowsNotepad".encode_utf16())
    {
        return Ok(identity);
    }
    let version = unsafe { package.version.Anonymous.Anonymous };
    identity.version = [
        version.Major.into(),
        version.Minor.into(),
        version.Build.into(),
        version.Revision.into(),
    ];
    identity.kind = 2;
    Ok(identity)
}
/// Retained process handle and captured creation identity.
pub struct Target {
    /// Retained process handle or process identifier, as specified by the field type.
    pub process: Handle,
    /// Canonical retained provider or process identity.
    pub identity: Identity,
    /// Actual client process architecture, independent of the observer's bitness.
    pub architecture: ProcessArchitecture,
}
impl Target {
    /// Acquire the retained native identities required by this observer.
    ///
    /// # Errors
    /// Returns the first failed native operation or evidence predicate.
    pub fn open(pid: u32) -> Result<Self> {
        let process = Handle(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )?
        });
        let identity = capture_identity(process.0, pid, 0)?;
        let architecture = process_architecture(process.0)?;
        Ok(Self {
            process,
            identity,
            architecture,
        })
    }
    /// Recheck that the retained process is live and has its captured creation identity.
    pub fn current(&self) -> bool {
        (unsafe {
            GetProcessId(self.process.0) == self.identity.process
                && WaitForSingleObject(self.process.0, 0) == WAIT_TIMEOUT
        }) && started(self.process.0, false).ok() == Some(self.identity.started)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Outcome of process-bound thread lifetime capture.
pub enum ThreadState {
    /// The retained thread and its owning process are live.
    Live,
    /// The retained thread has exited.
    Exited,
    /// The requested native object or publication is absent.
    Missing,
    /// Opening the thread failed.
    OpenFailed,
    /// Querying the thread owner failed.
    OwnerFailed,
    /// The thread owner did not match the target process.
    WrongOwner,
    /// Waiting on the thread returned an unexpected result.
    WaitFailed,
    /// The retained process has exited.
    ProcessStale,
    /// Querying or waiting on the retained process failed.
    ProcessFailed,
    /// The retained process ID did not match the target.
    WrongProcess,
}
/// Thread capture facts and its retained native handle.
pub struct ThreadCheck {
    /// The thread capture result.
    pub state: ThreadState,
    /// Captured native thread identifier.
    pub tid: u32,
    /// Process ID returned for the retained thread handle.
    pub owner: u32,
    /// Native Win32 error associated with the capture failure.
    pub error: u32,
    /// Native zero-timeout thread wait result.
    pub wait: u32,
    /// Native zero-timeout process wait result.
    pub process_wait: u32,
    /// Retained native handle or captured HWND value.
    pub handle: Option<Handle>,
}
/// Capture a thread only while its process and owner identities remain live.
pub fn capture_thread(process: HANDLE, pid: u32, tid: u32) -> ThreadCheck {
    let mut result = ThreadCheck {
        state: ThreadState::OpenFailed,
        tid,
        owner: 0,
        error: 0,
        wait: WAIT_FAILED.0,
        process_wait: WAIT_FAILED.0,
        handle: None,
    };
    if !process_live(process, pid, &mut result) {
        return result;
    }
    match unsafe {
        OpenThread(
            THREAD_QUERY_LIMITED_INFORMATION | THREAD_SYNCHRONIZE,
            false,
            tid,
        )
    } {
        Err(error) => {
            result.error = error.code().0 as u32 & 0xffff;
            result.state = if result.error == ERROR_INVALID_PARAMETER.0 {
                ThreadState::Missing
            } else {
                ThreadState::OpenFailed
            };
        }
        Ok(handle) => {
            result.handle = Some(Handle(handle));
            result.owner = unsafe { GetProcessIdOfThread(handle) };
            result.state = if result.owner == 0 {
                result.error = unsafe { GetLastError().0 };
                ThreadState::OwnerFailed
            } else if result.owner != pid {
                ThreadState::WrongOwner
            } else {
                let wait = unsafe { WaitForSingleObject(handle, 0) };
                result.wait = wait.0;
                if wait == WAIT_TIMEOUT {
                    ThreadState::Live
                } else if wait == WAIT_OBJECT_0 {
                    ThreadState::Exited
                } else {
                    result.error = unsafe { GetLastError().0 };
                    ThreadState::WaitFailed
                }
            };
        }
    }
    process_live(process, pid, &mut result);
    result
}
fn process_live(process: HANDLE, pid: u32, result: &mut ThreadCheck) -> bool {
    let owner = unsafe { GetProcessId(process) };
    if owner == 0 {
        result.error = unsafe { GetLastError().0 };
        result.state = ThreadState::ProcessFailed;
        return false;
    }
    if owner != pid {
        result.state = ThreadState::WrongProcess;
        return false;
    }
    let wait = unsafe { WaitForSingleObject(process, 0) };
    result.process_wait = wait.0;
    if wait == WAIT_TIMEOUT {
        return true;
    }
    result.state = if wait == WAIT_OBJECT_0 {
        ThreadState::ProcessStale
    } else {
        ThreadState::ProcessFailed
    };
    if wait == WAIT_FAILED {
        result.error = unsafe { GetLastError().0 };
    }
    false
}
/// Enumerate snapshot threads belonging to an explicit target PID.
///
/// # Errors
/// Returns the first failed native operation or evidence predicate.
pub fn threads(pid: u32) -> Result<Vec<u32>> {
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)? });
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut result = Vec::new();
    let mut step = unsafe { Thread32First(snapshot.0, &mut entry) };
    loop {
        if let Err(error) = step {
            return if error.code() == ERROR_NO_MORE_FILES.to_hresult() {
                Ok(result)
            } else {
                Err(error)
            };
        }
        if entry.th32OwnerProcessID == pid {
            result.push(entry.th32ThreadID);
        }
        step = unsafe { Thread32Next(snapshot.0, &mut entry) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_architecture_distinguishes_native_x64_and_wow64() {
        for (machine, native, expected) in [
            (0, 0x8664, Some(ProcessArchitecture::X64)),
            (0x014c, 0x8664, Some(ProcessArchitecture::X86)),
            (0, 0x014c, None),
            (0, 0xaa64, None),
            (0x014c, 0xaa64, None),
            (0x8664, 0xaa64, None),
            (0xaa64, 0x8664, None),
            (0xffff, 0x8664, None),
        ] {
            assert_eq!(
                classify_architecture(IMAGE_FILE_MACHINE(machine), IMAGE_FILE_MACHINE(native)),
                expected
            );
        }
        let actual = process_architecture(unsafe { GetCurrentProcess() }).unwrap();
        #[cfg(target_pointer_width = "64")]
        assert_eq!(actual, ProcessArchitecture::X64);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(actual, ProcessArchitecture::X86);
        assert_eq!(
            actual.name(),
            if cfg!(target_pointer_width = "64") {
                "x64"
            } else {
                "x86"
            }
        );
    }
    #[test]
    fn thread_capture_retains_live_thread_and_rejects_wrong_process() {
        let process = unsafe { GetCurrentProcess() };
        let pid = unsafe { GetCurrentProcessId() };
        let tid = unsafe { GetCurrentThreadId() };
        let captured = capture_thread(process, pid, tid);
        assert_eq!(captured.state, ThreadState::Live);
        assert_eq!(captured.owner, pid);
        assert!(captured.handle.is_some());
        assert_eq!(
            capture_thread(process, pid.wrapping_add(1), tid).state,
            ThreadState::WrongProcess
        );
        assert!(threads(pid).unwrap().contains(&tid));
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
pub(crate) mod baseline_tests;
