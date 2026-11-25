//! Bounded local-file acquisition and cancellable immutable vocabulary publication.

use crate::service::WORKERS;
use crate::vocabulary_contract::{
    Boundary, Config, DictionaryOwner, Error, Model, ModuleOwner, Path, Prepared, Ready, Source,
    SourceReady, Status,
};
use keyrill_ffi::{kr_abi_version, kr_prepare_model};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    NtCreateFile,
};
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::IO::*;
use windows::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::*;
use windows::Win32::System::WindowsProgramming::DRIVE_FIXED;
use windows_core::{BOOL, PCWSTR, PWSTR};

const MAX_BYTES: usize = 1024 * 1024;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct NativeCalls {
    pub(crate) open: *mut c_void,
    pub(crate) read: *mut c_void,
    pub(crate) completion: *mut c_void,
    pub(crate) cancel: *mut c_void,
    pub(crate) event: *mut c_void,
    pub(crate) pin: *mut c_void,
    pub(crate) submit: *mut c_void,
    pub(crate) long_running: *mut c_void,
    pub(crate) volume_paths: *mut c_void,
}

/// Callback owners guarantee concurrent-safe targets and context until the paired release.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Hooks {
    pub(crate) call: *mut c_void,
    pub(crate) context: *mut c_void,
    pub(crate) native: NativeCalls,
    pub(crate) retain_context: *mut c_void,
    pub(crate) release_context: *mut c_void,
}

#[cfg(feature = "test-api")]
static TEST_HOOKS: std::sync::OnceLock<Mutex<Option<Arc<HookLease>>>> = std::sync::OnceLock::new();

// SAFETY: the dependency boundary requires every callback and context to support worker threads.
unsafe impl Send for Hooks {}
// SAFETY: Hooks is immutable and its external owner guarantees concurrent-safe callback storage.
unsafe impl Sync for Hooks {}

#[repr(C)]
struct Call {
    value: usize,
    error: u32,
}

unsafe fn native_hook(
    target: *mut c_void,
    context: *mut c_void,
    boundary: u32,
    handle: HANDLE,
    index: usize,
) {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    let call: unsafe extern "system" fn(*mut c_void, u32, HANDLE, usize) =
        unsafe { std::mem::transmute(target) };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    unsafe { call(context, boundary, handle, index) };
}

unsafe fn native_lifetime(target: *mut c_void, context: *mut c_void) {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    let call: unsafe extern "system" fn(*mut c_void) = unsafe { std::mem::transmute(target) };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    unsafe { call(context) };
}

unsafe fn native_read(
    target: *mut c_void,
    file: HANDLE,
    buffer: *mut u8,
    count: u32,
    operation: *mut OVERLAPPED,
) -> Call {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    windows_core::link!("kernel32.dll" "system" fn ReadFile(file: HANDLE, buffer: *mut u8, count: u32, transferred: *mut u32, operation: *mut OVERLAPPED) -> BOOL);
    type Native =
        unsafe extern "system" fn(HANDLE, *mut u8, u32, *mut u32, *mut OVERLAPPED) -> BOOL;
    let call: Native = if target.is_null() {
        ReadFile
    } else {
        unsafe { std::mem::transmute::<*mut c_void, Native>(target) }
    };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    let result = unsafe { call(file, buffer, count, std::ptr::null_mut(), operation) };
    let error = if result.as_bool() {
        0
    } else {
        unsafe { GetLastError() }.0
    };
    Call {
        value: result.0 as usize,
        error,
    }
}

unsafe fn native_completion(
    target: *mut c_void,
    file: HANDLE,
    operation: *mut OVERLAPPED,
    count: *mut u32,
    wait: BOOL,
) -> Call {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    windows_core::link!("kernel32.dll" "system" fn GetOverlappedResult(file: HANDLE, operation: *mut OVERLAPPED, count: *mut u32, wait: BOOL) -> BOOL);
    type Native = unsafe extern "system" fn(HANDLE, *mut OVERLAPPED, *mut u32, BOOL) -> BOOL;
    let call: Native = if target.is_null() {
        GetOverlappedResult
    } else {
        unsafe { std::mem::transmute::<*mut c_void, Native>(target) }
    };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    let result = unsafe { call(file, operation, count, wait) };
    let error = if result.as_bool() {
        0
    } else {
        unsafe { GetLastError() }.0
    };
    Call {
        value: result.0 as usize,
        error,
    }
}

unsafe fn native_cancel(target: *mut c_void, file: HANDLE, operation: *mut OVERLAPPED) -> Call {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    windows_core::link!("kernel32.dll" "system" fn CancelIoEx(file: HANDLE, operation: *mut OVERLAPPED) -> BOOL);
    type Native = unsafe extern "system" fn(HANDLE, *mut OVERLAPPED) -> BOOL;
    let call: Native = if target.is_null() {
        CancelIoEx
    } else {
        unsafe { std::mem::transmute::<*mut c_void, Native>(target) }
    };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    let result = unsafe { call(file, operation) };
    let error = if result.as_bool() {
        0
    } else {
        unsafe { GetLastError() }.0
    };
    Call {
        value: result.0 as usize,
        error,
    }
}

unsafe fn native_event(target: *mut c_void) -> Call {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    windows_core::link!("kernel32.dll" "system" fn CreateEventW(security: *const c_void, manual: BOOL, initial: BOOL, name: PCWSTR) -> HANDLE);
    type Native = unsafe extern "system" fn(*const c_void, BOOL, BOOL, PCWSTR) -> HANDLE;
    let call: Native = if target.is_null() {
        CreateEventW
    } else {
        unsafe { std::mem::transmute::<*mut c_void, Native>(target) }
    };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    let result = unsafe { call(std::ptr::null(), BOOL(1), BOOL(0), PCWSTR::null()) };
    let error = if !result.0.is_null() {
        0
    } else {
        unsafe { GetLastError() }.0
    };
    Call {
        value: result.0 as usize,
        error,
    }
}

unsafe fn native_pin(
    target: *mut c_void,
    flags: u32,
    address: PCWSTR,
    module: *mut HMODULE,
) -> Call {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    windows_core::link!("kernel32.dll" "system" fn GetModuleHandleExW(flags: u32, address: PCWSTR, module: *mut HMODULE) -> BOOL);
    type Native = unsafe extern "system" fn(u32, PCWSTR, *mut HMODULE) -> BOOL;
    let call: Native = if target.is_null() {
        GetModuleHandleExW
    } else {
        unsafe { std::mem::transmute::<*mut c_void, Native>(target) }
    };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    let result = unsafe { call(flags, address, module) };
    let error = if result.as_bool() {
        0
    } else {
        unsafe { GetLastError() }.0
    };
    Call {
        value: result.0 as usize,
        error,
    }
}

unsafe fn native_submit(
    target: *mut c_void,
    callback: PTP_SIMPLE_CALLBACK,
    context: *mut c_void,
) -> Call {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    windows_core::link!("kernel32.dll" "system" fn TrySubmitThreadpoolCallback(callback: PTP_SIMPLE_CALLBACK, context: *mut c_void, environment: *const c_void) -> BOOL);
    type Native =
        unsafe extern "system" fn(PTP_SIMPLE_CALLBACK, *mut c_void, *const c_void) -> BOOL;
    let call: Native = if target.is_null() {
        TrySubmitThreadpoolCallback
    } else {
        unsafe { std::mem::transmute::<*mut c_void, Native>(target) }
    };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    let result = unsafe { call(callback, context, std::ptr::null()) };
    let error = if result.as_bool() {
        0
    } else {
        unsafe { GetLastError() }.0
    };
    Call {
        value: result.0 as usize,
        error,
    }
}

unsafe fn native_long_running(target: *mut c_void, instance: PTP_CALLBACK_INSTANCE) -> Call {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    windows_core::link!("kernel32.dll" "system" fn CallbackMayRunLong(instance: PTP_CALLBACK_INSTANCE) -> BOOL);
    type Native = unsafe extern "system" fn(PTP_CALLBACK_INSTANCE) -> BOOL;
    let call: Native = if target.is_null() {
        CallbackMayRunLong
    } else {
        unsafe { std::mem::transmute::<*mut c_void, Native>(target) }
    };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    let result = unsafe { call(instance) };
    let error = if result.as_bool() {
        0
    } else {
        unsafe { GetLastError() }.0
    };
    Call {
        value: result.0 as usize,
        error,
    }
}

unsafe fn native_volume_paths(
    target: *mut c_void,
    guid: PCWSTR,
    paths: PWSTR,
    capacity: u32,
    length: *mut u32,
) -> Call {
    // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
    windows_core::link!("kernel32.dll" "system" fn GetVolumePathNamesForVolumeNameW(guid: PCWSTR, paths: PWSTR, capacity: u32, length: *mut u32) -> BOOL);
    type Native = unsafe extern "system" fn(PCWSTR, PWSTR, u32, *mut u32) -> BOOL;
    let call: Native = if target.is_null() {
        GetVolumePathNamesForVolumeNameW
    } else {
        unsafe { std::mem::transmute::<*mut c_void, Native>(target) }
    };
    // SAFETY: the caller retains all handles and output storage until the operation drains.
    let result = unsafe { call(guid, paths, capacity, length) };
    let error = if result.as_bool() {
        0
    } else {
        unsafe { GetLastError() }.0
    };
    Call {
        value: result.0 as usize,
        error,
    }
}

impl Hooks {
    fn at(self, boundary: Boundary, handle: HANDLE, index: usize) {
        if !self.call.is_null() {
            // SAFETY: validated callback storage lives through the dependency lease. Callbacks obey the no-throw ABI contract.
            unsafe { native_hook(self.call, self.context, boundary as u32, handle, index) };
        }
    }

    fn event(self, failure: Error) -> Result<Handle, Error> {
        // SAFETY: the event has no borrowed security descriptor or name.
        let handle = unsafe { HANDLE(native_event(self.native.event).value as *mut c_void) };
        Handle::new(handle).ok_or(failure)
    }

    fn completion(
        self,
        file: HANDLE,
        operation: &mut OVERLAPPED,
        count: &mut u32,
        wait: bool,
    ) -> Call {
        // SAFETY: the file, operation, buffer and event remain owned until completion drains.
        unsafe {
            native_completion(
                self.native.completion,
                file,
                operation,
                count,
                BOOL::from(wait),
            )
        }
    }
}

struct HookLease(Hooks);

impl HookLease {
    fn acquire(hooks: Hooks) -> Result<Self, Error> {
        if hooks.retain_context.is_null() != hooks.release_context.is_null() {
            return Err(Error::Startup);
        }
        if !hooks.retain_context.is_null() {
            // SAFETY: the caller supplies one paired acquisition and release for the complete table.
            unsafe { native_lifetime(hooks.retain_context, hooks.context) };
        }
        Ok(Self(hooks))
    }
}

impl Drop for HookLease {
    fn drop(&mut self) {
        if !self.0.release_context.is_null() {
            // SAFETY: this is the sole release paired with the successful retained callback owner.
            unsafe { native_lifetime(self.0.release_context, self.0.context) };
        }
    }
}

#[cfg(feature = "test-api")]
fn configured_hooks() -> Option<Arc<HookLease>> {
    TEST_HOOKS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()?
        .clone()
}

#[derive(Default)]
struct Handle(HANDLE);

impl Handle {
    fn new(value: HANDLE) -> Option<Self> {
        (!value.is_invalid()).then_some(Self(value))
    }
}

// SAFETY: file/event handles are kernel objects and move between threads with unique ownership.
unsafe impl Send for Handle {}
// SAFETY: shared use is restricted to kernel synchronization and immutable metadata queries.
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: this wrapper uniquely owns the handle, and pending I/O has drained.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

fn environment(name: &str) -> Result<Option<Vec<u16>>, Error> {
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut buffer = [0; 4096];
    // SAFETY: name is terminated and buffer supplies the advertised capacity.
    unsafe {
        SetLastError(ERROR_SUCCESS);
        let length =
            crate::foreign::win32::environment(PCWSTR(name.as_ptr()), &mut buffer) as usize;
        if length == 0 && GetLastError() == ERROR_ENVVAR_NOT_FOUND {
            return Ok(None);
        }
        if length == 0 || length >= buffer.len() {
            return Err(Error::Configuration);
        }
        Ok(Some(buffer[..length].to_vec()))
    }
}

fn device_name(part: &[u16]) -> bool {
    let stem = part
        .split(|unit| *unit == b'.' as u16)
        .next()
        .unwrap_or_default();
    let uppercase: Vec<u16> = stem
        .iter()
        .map(|unit| match *unit {
            97..=122 => *unit - 32,
            _ => *unit,
        })
        .collect();
    let equals = |text: &str| uppercase.iter().copied().eq(text.encode_utf16());
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .into_iter()
        .any(equals)
    {
        return true;
    }
    uppercase.len() == 4
        && (uppercase[..3] == [67, 79, 77] || uppercase[..3] == [76, 80, 84])
        && matches!(uppercase[3], 48..=57 | 0xb9 | 0xb2 | 0xb3)
}

pub(crate) fn parse_path(text: &[u16]) -> Option<Path> {
    if !(4..=4095).contains(&text.len())
        || !matches!(text[0], 65..=90 | 97..=122)
        || text[1..3] != [58, 92]
    {
        return None;
    }
    let mut path = Path {
        drive: text[..3].to_vec(),
        components: Vec::new(),
    };
    for part in text[3..].split(|unit| *unit == 92) {
        if part.is_empty()
            || part.len() > 255
            || part == [46]
            || part == [46, 46]
            || matches!(part.last(), Some(46 | 32))
            || device_name(part)
            || part
                .iter()
                .any(|unit| *unit < 32 || [47, 58, 42, 63, 34, 60, 62, 124].contains(unit))
            || path.components.len() >= 64
        {
            return None;
        }
        path.components.push(part.to_vec());
    }
    Some(path)
}

pub(crate) fn capture_config() -> Option<Config> {
    let policy = environment("KEYRILL_WINDOWS_POLICY").ok()?;
    if policy.is_none() {
        return Some(Config::Persisted);
    }
    let equals = |text: &str| {
        policy
            .as_ref()
            .is_some_and(|value| value.iter().copied().eq(text.encode_utf16()))
    };
    if equals("fixture") {
        return Some(Config::Fixture);
    }
    if equals("off") {
        return Some(Config::Off);
    }
    if !equals("model") {
        return None;
    }
    let english = environment("KEYRILL_WINDOWS_EN_DICTIONARY").ok()?;
    let russian = environment("KEYRILL_WINDOWS_RU_DICTIONARY").ok()?;
    let explicit = [english, russian].map(|value| match value {
        None => Some(None),
        Some(path) => parse_path(&path).map(Some),
    });
    let [english, russian] = explicit;
    let english = english?;
    let russian = russian?;
    let profile = environment("USERPROFILE").ok()?;
    let source = |path: Option<Path>, language: &str| {
        path.map(Source::Explicit).unwrap_or_else(|| {
            let path = profile.as_ref().and_then(|profile| {
                let mut text = profile.clone();
                text.extend(format!("\\vimfiles\\spell\\{language}.utf-8.add").encode_utf16());
                parse_path(&text)
            });
            Source::Default(path)
        })
    };
    Some(Config::Model(Model {
        sources: [source(english, "en"), source(russian, "ru")],
    }))
}

fn volume(drive: &[u16], hooks: Hooks) -> Result<Vec<u16>, Error> {
    struct Enumeration(HANDLE);
    impl Drop for Enumeration {
        fn drop(&mut self) {
            // SAFETY: this owns the volume enumeration returned by FindFirstVolumeW.
            let _ = unsafe { FindVolumeClose(self.0) };
        }
    }
    let mut guid = [0; 64];
    // SAFETY: guid is writable for the generated binding's advertised capacity.
    let enumeration = Enumeration(
        crate::foreign::win32::first_volume(&mut guid).map_err(|_| Error::Unavailable)?,
    );
    let mut selected = None;
    for _ in 0..1024 {
        let mut paths = vec![0; 32768];
        let mut length = 0;
        // SAFETY: guid is terminated by volume enumeration. All output storage is live for the call.
        let result = unsafe {
            native_volume_paths(
                hooks.native.volume_paths,
                PCWSTR(guid.as_ptr()),
                PWSTR(paths.as_mut_ptr()),
                paths.len() as u32,
                &mut length,
            )
        };
        if result.value == 0 || length as usize > paths.len() {
            return Err(Error::Unavailable);
        }
        for path in paths
            .split(|unit| *unit == 0)
            .take_while(|path| !path.is_empty())
        {
            if path.len() == drive.len()
                && path
                    .iter()
                    .zip(drive)
                    .all(|(a, b)| ascii_fold(*a) == ascii_fold(*b))
            {
                if selected.is_some() {
                    return Err(Error::Unavailable);
                }
                let end = guid
                    .iter()
                    .position(|unit| *unit == 0)
                    .ok_or(Error::Unavailable)?;
                selected = Some(guid[..end].to_vec());
            }
        }
        // SAFETY: enumeration and guid remain owned by this function.
        if crate::foreign::win32::next_volume(enumeration.0, &mut guid).is_err() {
            // SAFETY: last error is read immediately after the failed enumeration call.
            return if unsafe { GetLastError() } == ERROR_NO_MORE_FILES {
                selected.ok_or(Error::Unavailable)
            } else {
                Err(Error::Unavailable)
            };
        }
    }
    Err(Error::Unavailable)
}

fn ascii_fold(unit: u16) -> u16 {
    match unit {
        65..=90 => unit + 32,
        _ => unit,
    }
}

#[derive(Clone, Default)]
struct Metadata {
    id: FILE_ID_INFO,
    basic: FILE_BASIC_INFO,
    standard: FILE_STANDARD_INFO,
}

impl Metadata {
    fn same_id(&self, other: &Self) -> bool {
        self.id.VolumeSerialNumber == other.id.VolumeSerialNumber
            && self.id.FileId.Identifier == other.id.FileId.Identifier
    }
    fn same_file(&self, other: &Self) -> bool {
        self.same_id(other)
            && self.standard.EndOfFile == other.standard.EndOfFile
            && self.basic.CreationTime == other.basic.CreationTime
            && self.basic.LastWriteTime == other.basic.LastWriteTime
            && self.basic.ChangeTime == other.basic.ChangeTime
            && self.basic.FileAttributes == other.basic.FileAttributes
    }
}

fn inspect(handle: HANDLE, directory: bool, volume: Option<u64>) -> Result<Metadata, Error> {
    let mut metadata = Metadata::default();
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    let prohibited = FILE_ATTRIBUTE_REPARSE_POINT.0
        | FILE_ATTRIBUTE_OFFLINE.0
        | FILE_ATTRIBUTE_RECALL_ON_OPEN.0
        | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0;
    // SAFETY: every information class is paired with its exact SDK structure and writable capacity.
    unsafe {
        if GetFileType(handle) != FILE_TYPE_DISK {
            return Err(Error::Unavailable);
        }
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut metadata.id as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
        .map_err(|_| Error::Unavailable)?;
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&mut metadata.basic as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
        .map_err(|_| Error::Unavailable)?;
        GetFileInformationByHandleEx(
            handle,
            FileStandardInfo,
            (&mut metadata.standard as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
        .map_err(|_| Error::Unavailable)?;
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
        .map_err(|_| Error::Unavailable)?;
    }
    if metadata.standard.DeletePending
        || metadata.standard.Directory != directory
        || tag.FileAttributes & prohibited != 0
        || metadata.basic.FileAttributes & prohibited != 0
        || tag.ReparseTag != 0
        || volume.is_some_and(|volume| metadata.id.VolumeSerialNumber != volume)
    {
        return Err(Error::Unavailable);
    }
    Ok(metadata)
}

fn open(
    parent: HANDLE,
    name: &[u16],
    directory: bool,
    missing: Error,
    hooks: Hooks,
) -> Result<Handle, Error> {
    if name.len() > 32760 {
        return Err(Error::Unavailable);
    }
    struct OpenOperation {
        name: Vec<u16>,
        text: UNICODE_STRING,
        attributes: OBJECT_ATTRIBUTES,
        io: IO_STATUS_BLOCK,
        raw: HANDLE,
    }
    let mut owned = Box::new(OpenOperation {
        name: name.to_vec(),
        text: UNICODE_STRING::default(),
        attributes: OBJECT_ATTRIBUTES::default(),
        io: IO_STATUS_BLOCK::default(),
        raw: HANDLE::default(),
    });
    owned.text = UNICODE_STRING {
        Length: (owned.name.len() * 2) as u16,
        MaximumLength: (owned.name.len() * 2) as u16,
        Buffer: PWSTR(owned.name.as_mut_ptr()),
    };
    owned.attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent,
        ObjectName: &owned.text,
        Attributes: OBJ_CASE_INSENSITIVE,
        ..Default::default()
    };
    let access = FILE_READ_ATTRIBUTES
        | SYNCHRONIZE
        | if directory {
            FILE_TRAVERSE
        } else {
            FILE_READ_DATA
        };
    let sharing = FILE_SHARE_READ
        | FILE_SHARE_WRITE
        | if directory {
            FILE_SHARE_MODE(0)
        } else {
            FILE_SHARE_DELETE
        };
    let options = FILE_OPEN_REPARSE_POINT
        | if directory {
            FILE_SYNCHRONOUS_IO_NONALERT
        } else {
            FILE_NON_DIRECTORY_FILE
        };
    // SAFETY: native object/name/IOSB storage remains live through pending completion below.
    let mut status = unsafe {
        if hooks.native.open.is_null() {
            NtCreateFile(
                &mut owned.raw,
                access,
                &owned.attributes,
                &mut owned.io,
                None,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                sharing,
                FILE_OPEN,
                options,
                None,
                0,
            )
            .0
        } else {
            // SAFETY: the dependency owner provides this exact system ABI function and retains its code.
            let call: unsafe extern "system" fn(
                *mut HANDLE,
                u32,
                *mut OBJECT_ATTRIBUTES,
                *mut IO_STATUS_BLOCK,
                *const i64,
                u32,
                u32,
                u32,
                u32,
                *const c_void,
                u32,
            ) -> NTSTATUS = std::mem::transmute(hooks.native.open);
            // SAFETY: the caller retains all handles and output storage until the operation drains.
            let status = call(
                &mut owned.raw,
                access.0,
                &mut owned.attributes,
                &mut owned.io,
                std::ptr::null(),
                0,
                sharing.0,
                FILE_OPEN.0,
                options.0,
                std::ptr::null(),
                0,
            );
            status.0
        }
    };
    let handle = Handle::new(owned.raw);
    if status == 0x103 {
        loop {
            // SAFETY: the pending open owns its IOSB, name storage and any returned handle.
            if handle.as_ref().is_some_and(
                |handle| unsafe { WaitForSingleObject(handle.0, INFINITE) } == WAIT_OBJECT_0,
            ) {
                break;
            }
            // SAFETY: wait backs off while preserving all pending-open storage.
            unsafe { Sleep(1000) };
        }
        // SAFETY: STATUS_PENDING selected the Status union variant and the operation has completed.
        status = unsafe { owned.io.Anonymous.Status.0 };
    }
    if matches!(status as u32, 0xc0000034 | 0xc000003a) {
        return Err(missing);
    }
    if status < 0 {
        return Err(Error::Unavailable);
    }
    handle.ok_or(Error::Unavailable)
}

fn check_stop(stop: HANDLE) -> Result<(), Error> {
    // SAFETY: the State owns this event throughout every worker operation.
    if unsafe { WaitForSingleObject(stop, 0) } == WAIT_TIMEOUT {
        Ok(())
    } else {
        Err(Error::Cancelled)
    }
}

struct ReadOperation {
    operation: OVERLAPPED,
    event: Handle,
    bytes: Vec<u8>,
}

fn drain(file: &mut Handle, pending: &mut Option<Box<ReadOperation>>, hooks: Hooks) {
    let operation = &mut pending.as_mut().expect("pending I/O is owned").operation;
    // SAFETY: this is the one cancellation attempt for the still-owned operation.
    let _ = unsafe { native_cancel(hooks.native.cancel, file.0, operation) };
    let mut count = 0;
    let completion = hooks.completion(file.0, operation, &mut count, true);
    if completion.value == 0 && completion.error == ERROR_IO_INCOMPLETE.0 {
        crate::foreign::pin_current_module_for_process_lifetime();
        // Completion is uncertain. Kernel references may still target this storage and file.
        std::mem::forget(pending.take());
        std::mem::forget(std::mem::take(file));
    }
}

fn read(file: &mut Handle, stop: HANDLE, length: usize, hooks: Hooks) -> Result<Vec<u8>, Error> {
    let event = hooks.event(Error::Read)?;
    let mut pending = Some(Box::new(ReadOperation {
        operation: OVERLAPPED::default(),
        event,
        bytes: vec![0; length + 1],
    }));
    let mut offset = 0;
    // SAFETY: monotonic tick count has no borrowed inputs.
    let deadline = unsafe { GetTickCount64() } + 2000;
    while offset <= length {
        check_stop(stop)?;
        let owned = pending.as_mut().expect("read owns its operation");
        owned.operation = OVERLAPPED {
            hEvent: owned.event.0,
            Anonymous: OVERLAPPED_0 {
                Anonymous: OVERLAPPED_0_0 {
                    Offset: offset as u32,
                    OffsetHigh: 0,
                },
            },
            ..Default::default()
        };
        // SAFETY: the owned manual-reset event belongs exclusively to this read.
        unsafe { ResetEvent(owned.event.0) }.map_err(|_| Error::Read)?;
        let count = (owned.bytes.len() - offset).min(65536);
        // SAFETY: Box fixes the operation address and the vector will not reallocate until drained.
        let result = unsafe {
            native_read(
                hooks.native.read,
                file.0,
                owned.bytes.as_mut_ptr().add(offset),
                count as u32,
                &mut owned.operation,
            )
        };

        if result.value == 0 && result.error == ERROR_IO_PENDING.0 {
            // SAFETY: both wait handles remain owned through the following drain/completion.
            let waited = unsafe {
                let now = GetTickCount64();
                WaitForMultipleObjects(
                    &[stop, owned.event.0],
                    false,
                    deadline.saturating_sub(now) as u32,
                )
            };
            if waited.0 != WAIT_OBJECT_0.0 + 1 {
                drain(file, &mut pending, hooks);
                return Err(if waited == WAIT_OBJECT_0 {
                    Error::Cancelled
                } else {
                    Error::Read
                });
            }
        }
        let mut transferred = 0;
        let result = if result.value != 0 || result.error == ERROR_IO_PENDING.0 {
            hooks.completion(file.0, &mut owned.operation, &mut transferred, false)
        } else {
            result
        };
        if result.value == 0 && result.error == ERROR_IO_INCOMPLETE.0 {
            crate::foreign::pin_current_module_for_process_lifetime();
            drain(file, &mut pending, hooks);
            return Err(Error::Startup);
        }
        if result.value == 0 && result.error != ERROR_HANDLE_EOF.0 {
            return Err(Error::Read);
        }
        if transferred == 0 {
            if offset != length {
                return Err(Error::Unstable);
            }
            let mut owned = pending.take().expect("completed operation is owned");
            owned.bytes.truncate(length);
            return Ok(owned.bytes);
        }
        offset += transferred as usize;
        if offset > length {
            return Err(Error::Unstable);
        }
    }
    Err(Error::Unstable)
}

struct Observation {
    metadata: Metadata,
    bytes: Vec<u8>,
}
struct Parent {
    handle: Handle,
    metadata: Metadata,
}

fn revalidate(
    parents: &[Parent],
    drive: &[u16],
    original_volume: &[u16],
    hooks: Hooks,
) -> Result<(), Error> {
    let serial = parents[0].metadata.id.VolumeSerialNumber;
    for parent in parents {
        if !parent
            .metadata
            .same_id(&inspect(parent.handle.0, true, Some(serial))?)
        {
            return Err(Error::Unstable);
        }
    }
    match volume(drive, hooks) {
        Ok(current) if current == original_volume => {}
        Err(Error::Startup) => return Err(Error::Startup),
        _ => return Err(Error::Unstable),
    }
    Ok(())
}

fn observe(path: &Path, stop: HANDLE, hooks: Hooks) -> Result<Observation, Error> {
    check_stop(stop)?;
    if path.components.is_empty() {
        return Err(Error::Unavailable);
    }
    let selected_volume = volume(&path.drive, hooks)?;
    let terminated: Vec<u16> = selected_volume.iter().copied().chain(Some(0)).collect();
    // SAFETY: terminated holds a live zero-terminated volume GUID name.
    if unsafe { GetDriveTypeW(PCWSTR(terminated.as_ptr())) } != DRIVE_FIXED {
        return Err(Error::Unavailable);
    }
    if selected_volume.len() < 4 {
        return Err(Error::Unavailable);
    }
    let native: Vec<u16> = "\\??\\"
        .encode_utf16()
        .chain(selected_volume[4..].iter().copied())
        .collect();
    let root = open(HANDLE::default(), &native, true, Error::Unavailable, hooks)?;
    let root_meta = inspect(root.0, true, None)?;
    let serial = root_meta.id.VolumeSerialNumber;
    let mut filesystem = [0; 32];
    // SAFETY: root is an owned directory handle and output storage has its advertised capacity.
    crate::foreign::win32::volume_filesystem(root.0, &mut filesystem)
        .map_err(|_| Error::Unavailable)?;
    if filesystem[..5]
        .iter()
        .copied()
        .map(ascii_fold)
        .ne("ntfs\0".encode_utf16())
    {
        return Err(Error::Unavailable);
    }
    let mut parents = vec![Parent {
        handle: root,
        metadata: root_meta,
    }];
    let child = |parents: &[Parent], name: &[u16], directory: bool| {
        let parent = parents.last().ok_or(Error::Unavailable)?;
        inspect(parent.handle.0, true, Some(serial))?;
        open(parent.handle.0, name, directory, Error::Missing, hooks).map_err(|error| {
            if error == Error::Startup {
                return error;
            }
            revalidate(parents, &path.drive, &selected_volume, hooks)
                .err()
                .unwrap_or(error)
        })
    };
    for (index, component) in path
        .components
        .iter()
        .enumerate()
        .take(path.components.len() - 1)
    {
        hooks.at(
            Boundary::ParentOpened,
            parents.last().ok_or(Error::Unavailable)?.handle.0,
            index,
        );
        check_stop(stop)?;
        let handle = child(&parents, component, true)?;
        let metadata = inspect(handle.0, true, Some(serial))?;
        parents.push(Parent { handle, metadata });
    }
    hooks.at(
        Boundary::ParentOpened,
        parents.last().ok_or(Error::Unavailable)?.handle.0,
        path.components.len() - 1,
    );
    let mut file = child(
        &parents,
        path.components.last().ok_or(Error::Unavailable)?,
        false,
    )?;
    let before = inspect(file.0, false, Some(serial))?;
    for parent in &parents {
        if !parent
            .metadata
            .same_id(&inspect(parent.handle.0, true, Some(serial))?)
        {
            return Err(Error::Unstable);
        }
    }
    if before.standard.EndOfFile < 0 || before.standard.EndOfFile as u64 > MAX_BYTES as u64 {
        return Err(Error::Unavailable);
    }
    hooks.at(Boundary::BeforeRead, file.0, 0);
    let bytes = read(&mut file, stop, before.standard.EndOfFile as usize, hooks)?;
    hooks.at(Boundary::AfterRead, file.0, 0);
    if !before.same_file(&inspect(file.0, false, Some(serial))?) {
        return Err(Error::Unstable);
    }
    let mut cursor: Option<Handle> = None;
    for (index, component) in path.components.iter().enumerate() {
        let directory = index + 1 < path.components.len();
        let previous = cursor
            .as_ref()
            .map_or(parents[0].handle.0, |cursor| cursor.0);
        let next =
            open(previous, component, directory, Error::Unstable, hooks).map_err(|error| {
                if error == Error::Startup {
                    return error;
                }
                revalidate(&parents, &path.drive, &selected_volume, hooks)
                    .err()
                    .unwrap_or(error)
            })?;
        let metadata = inspect(next.0, directory, Some(serial))?;
        if if directory {
            !metadata.same_id(&parents[index + 1].metadata)
        } else {
            !metadata.same_file(&before)
        } {
            return Err(Error::Unstable);
        }
        cursor = Some(next);
    }
    revalidate(&parents, &path.drive, &selected_volume, hooks)?;
    check_stop(stop)?;
    Ok(Observation {
        metadata: before,
        bytes,
    })
}

struct Candidate {
    observation: Observation,
    dictionary: Arc<DictionaryOwner>,
}

#[derive(Default)]
struct SourceState {
    staged: Option<Candidate>,
    published: Option<Candidate>,
    absent: Option<Arc<DictionaryOwner>>,
    ready: SourceReady,
}

impl SourceState {
    fn retained(&self) -> Option<Arc<DictionaryOwner>> {
        self.published
            .as_ref()
            .map(|published| Arc::clone(&published.dictionary))
            .or_else(|| self.absent.clone())
    }

    fn step(&mut self, source: &Source, stop: HANDLE, hooks: Hooks) -> Result<(), Error> {
        if let Err(error) = self.observe(source, stop, hooks) {
            self.staged = None;
            if matches!(error, Error::Cancelled | Error::Startup) {
                return Err(error);
            }
            if error == Error::Missing
                && matches!(source, Source::Default(_))
                && self.published.is_none()
            {
                if self.absent.is_none() {
                    self.absent = Some(DictionaryOwner::parse(&[])?);
                }
                self.ready = SourceReady {
                    status: Status::Ready,
                    error: Error::None,
                    dictionary: self.absent.clone(),
                };
            } else {
                let dictionary = self.retained();
                self.ready = SourceReady {
                    status: if dictionary.is_some() {
                        Status::Retained
                    } else {
                        Status::Missing
                    },
                    error,
                    dictionary,
                };
            }
        }
        Ok(())
    }

    fn observe(&mut self, source: &Source, stop: HANDLE, hooks: Hooks) -> Result<(), Error> {
        let path = match source {
            Source::Default(path) => path.as_ref().ok_or(Error::Unavailable)?,
            Source::Explicit(path) => path,
        };
        let observation = observe(path, stop, hooks)?;
        let same = |candidate: &Candidate| {
            candidate
                .observation
                .metadata
                .same_file(&observation.metadata)
                && candidate.observation.bytes == observation.bytes
        };
        if self.published.as_ref().is_some_and(same) {
            self.staged = None;
            self.ready = SourceReady {
                status: Status::Ready,
                error: Error::None,
                dictionary: self.retained(),
            };
        } else if self.staged.as_ref().is_some_and(same) {
            let staged = self.staged.take().ok_or(Error::Startup)?;
            self.published = Some(Candidate {
                observation,
                dictionary: staged.dictionary,
            });
            self.absent = None;
            self.ready = SourceReady {
                status: Status::Ready,
                error: Error::None,
                dictionary: self.retained(),
            };
        } else {
            self.staged = None;
            hooks.at(Boundary::BeforeParse, HANDLE::default(), 0);
            check_stop(stop)?;
            let dictionary = DictionaryOwner::parse(&observation.bytes)?;
            check_stop(stop)?;
            self.staged = Some(Candidate {
                observation,
                dictionary,
            });
            let dictionary = self.retained();
            self.ready = SourceReady {
                status: if dictionary.is_some() {
                    Status::Retained
                } else {
                    Status::Pending
                },
                error: Error::None,
                dictionary,
            };
        }
        Ok(())
    }
}

enum WorkConfig {
    Model(Model),
    Persisted(Option<String>),
}

struct State {
    config: WorkConfig,
    activation: u32,
    stop: Handle,
    revoked: AtomicBool,
    mailbox: Mutex<Option<Ready>>,
    hooks: Hooks,
}

impl State {
    fn publish(&self, ready: Ready) -> Result<(), Error> {
        self.publish_if(ready, || true).map(|_| ())
    }

    fn publish_if(&self, ready: Ready, current: impl FnOnce() -> bool) -> Result<bool, Error> {
        self.hooks.at(Boundary::BeforePublish, HANDLE::default(), 0);
        if !current() {
            return Ok(false);
        }
        let replaced = {
            let mut mailbox = self.mailbox.lock().map_err(|_| Error::Startup)?;
            if self.revoked.load(Ordering::Acquire) {
                None
            } else {
                mailbox.replace(ready)
            }
        };
        drop(replaced);
        self.hooks.at(Boundary::Published, HANDLE::default(), 0);
        Ok(true)
    }

    fn run(&self, instance: PTP_CALLBACK_INSTANCE, ready: &mut Ready) -> Result<(), Error> {
        self.hooks.at(Boundary::BeforeWork, HANDLE::default(), 0);
        check_stop(self.stop.0)?;
        // SAFETY: instance is provided by the native threadpool for this callback.
        let long_running = unsafe { native_long_running(self.hooks.native.long_running, instance) };
        if long_running.value == 0 {
            return Err(Error::Startup);
        }
        if kr_prepare_model(kr_abi_version()) != 0 {
            return Err(Error::Model);
        }
        check_stop(self.stop.0)?;
        if let WorkConfig::Persisted(namespace) = &self.config {
            return self.run_persisted(ready, namespace.as_deref());
        }
        let WorkConfig::Model(model) = &self.config else {
            unreachable!()
        };
        let mut sources = [SourceState::default(), SourceState::default()];
        ready.model = true;
        loop {
            ready.revision = ready.revision.checked_add(1).ok_or(Error::Startup)?;
            for (index, source) in sources.iter_mut().enumerate() {
                source.step(&model.sources[index], self.stop.0, self.hooks)?;
                ready.sources[index] = source.ready.clone();
            }
            check_stop(self.stop.0)?;
            self.publish(ready.clone())?;
            // SAFETY: State retains the stop event throughout this callback.
            if unsafe { WaitForSingleObject(self.stop.0, 500) } != WAIT_TIMEOUT {
                return Ok(());
            }
        }
    }
}

pub(crate) fn model_for(preferences: &crate::settings::Preferences) -> Model {
    let profile = environment("USERPROFILE").ok().flatten();
    let sources = std::array::from_fn(|index| match &preferences.dictionaries[index] {
        crate::settings::DictionaryPath::Explicit(path) => Source::Explicit(
            parse_path(&path.as_str().encode_utf16().collect::<Vec<_>>())
                .expect("typed local path already validated"),
        ),
        crate::settings::DictionaryPath::Default => {
            let path = profile.as_ref().and_then(|profile| {
                let mut path = profile.clone();
                let language = ["en", "ru"][index];
                path.extend(format!("\\vimfiles\\spell\\{language}.utf-8.add").encode_utf16());
                parse_path(&path)
            });
            Source::Default(path)
        }
    });
    Model { sources }
}

impl State {
    fn run_persisted(&self, ready: &mut Ready, namespace: Option<&str>) -> Result<(), Error> {
        #[cfg(feature = "test-api")]
        let store = match namespace {
            Some(namespace) => crate::settings::Store::isolated_test(namespace),
            None => crate::settings::Store::current_user(),
        };
        #[cfg(not(feature = "test-api"))]
        let store = {
            debug_assert!(namespace.is_none());
            crate::settings::Store::current_user()
        };
        let mut reader = crate::settings::Reader::default();
        let identities = crate::application_identity::current();
        let mut binding: Option<Model> = None;
        let mut sources = [SourceState::default(), SourceState::default()];
        let mut complete: Option<Ready> = None;
        loop {
            check_stop(self.stop.0)?;
            let refreshed = store
                .as_ref()
                .ok()
                .and_then(|store| reader.refresh(store).ok());
            let configuration_error = if refreshed.is_some() {
                Error::None
            } else {
                Error::Configuration
            };
            let snapshot = reader.snapshot().clone();
            let model = model_for(&snapshot.value);
            for (index, source) in sources.iter_mut().enumerate() {
                if binding
                    .as_ref()
                    .is_none_or(|old| old.sources[index] != model.sources[index])
                {
                    *source = SourceState::default();
                }
                source.step(&model.sources[index], self.stop.0, self.hooks)?;
            }
            binding = Some(model);
            check_stop(self.stop.0)?;
            let current = store.as_ref().ok().and_then(|store| store.load().ok());
            let unchanged = current.as_ref().is_some_and(|current| *current == snapshot);
            let dictionaries_ready = sources
                .iter()
                .all(|source| source.ready.dictionary.is_some());
            let prepared = Prepared {
                settings: snapshot.clone(),
                application: crate::application_identity::policy(&identities, &snapshot.value),
            };
            let candidate = if unchanged && (dictionaries_ready || prepared.policy() == 0) {
                Some(Ready {
                    activation: self.activation,
                    model: true,
                    configuration: Some(prepared),
                    configuration_error: if identities.is_ok() {
                        configuration_error
                    } else {
                        Error::Unavailable
                    },
                    sources: std::array::from_fn(|index| sources[index].ready.clone()),
                    ..Ready::default()
                })
            } else {
                Some({
                    let mut previous = complete.clone().unwrap_or_else(|| Ready {
                        activation: self.activation,
                        sources: std::array::from_fn(|index| sources[index].ready.clone()),
                        ..Ready::default()
                    });
                    previous.configuration_error = if !unchanged {
                        Error::Configuration
                    } else {
                        Error::None
                    };
                    previous.error = sources
                        .iter()
                        .map(|source| source.ready.error)
                        .find(|error| *error != Error::None)
                        .unwrap_or(Error::None);
                    previous
                })
            };
            if let Some(mut candidate) = candidate {
                candidate.revision = ready.revision.checked_add(1).ok_or(Error::Startup)?;
                if self.publish_if(candidate.clone(), || {
                    !unchanged
                        || store
                            .as_ref()
                            .ok()
                            .and_then(|store| store.load().ok())
                            .as_ref()
                            == Some(&snapshot)
                })? {
                    *ready = candidate.clone();
                    if candidate.configuration.is_some() {
                        complete = Some(candidate);
                    }
                }
            }
            // SAFETY: this callback retains its own stop event until all work drains.
            if unsafe { WaitForSingleObject(self.stop.0, 500) } != WAIT_TIMEOUT {
                return Ok(());
            }
        }
    }
}

struct Launch {
    state: Arc<State>,
    module: HMODULE,
    lease: HookLease,
}

unsafe extern "system" fn work(instance: PTP_CALLBACK_INSTANCE, context: *mut c_void) {
    // SAFETY: successful submission transfers this Box exactly once to the native callback.
    let launch = unsafe { Box::from_raw(context.cast::<Launch>()) };
    if !launch.module.is_invalid() {
        // SAFETY: the successful module pin belongs to this callback and is released after its return.
        unsafe { FreeLibraryWhenCallbackReturns(instance, launch.module) };
    }
    let hooks = launch.lease.0;
    let mut ready = Ready {
        activation: launch.state.activation,
        ..Default::default()
    };
    let result = catch_unwind(AssertUnwindSafe(|| launch.state.run(instance, &mut ready)));
    let error = match result {
        Ok(Ok(())) => None,
        Ok(Err(Error::Cancelled)) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some(Error::Startup),
    };
    if let Some(error) = error {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            if let Some(revision) = ready.revision.checked_add(1) {
                ready.revision = revision;
                ready.error = error;
                let _ = launch.state.publish(ready);
            }
        }));
    }
    hooks.at(Boundary::Cleanup, HANDLE::default(), 0);
    let Launch {
        state,
        module: _,
        lease,
    } = *launch;
    let _ = catch_unwind(AssertUnwindSafe(|| drop(state)));
    WORKERS.fetch_sub(1, Ordering::AcqRel);
    hooks.at(Boundary::AfterCount, HANDLE::default(), 0);
    drop(lease);
}

#[derive(Default)]
pub(crate) struct Worker {
    state: Option<Arc<State>>,
}

impl Worker {
    pub(crate) fn start_persisted(
        &mut self,
        activation: u32,
        owner: ModuleOwner,
        hooks: Hooks,
    ) -> bool {
        #[cfg(feature = "test-api")]
        let namespace = std::env::var_os("KEYRILL_WINDOWS_TEST_SETTINGS_NAMESPACE")
            .map(|value| value.to_string_lossy().into_owned());
        #[cfg(not(feature = "test-api"))]
        let namespace = None;
        self.start_config(WorkConfig::Persisted(namespace), activation, owner, hooks)
    }

    pub(crate) fn start(
        &mut self,
        model: &Model,
        activation: u32,
        owner: ModuleOwner,
        hooks: Hooks,
    ) -> bool {
        self.start_config(WorkConfig::Model(model.clone()), activation, owner, hooks)
    }

    fn start_config(
        &mut self,
        config: WorkConfig,
        activation: u32,
        owner: ModuleOwner,
        hooks: Hooks,
    ) -> bool {
        self.stop();
        #[cfg(feature = "test-api")]
        let configured = if hooks.call.is_null() {
            configured_hooks()
        } else {
            None
        };
        #[cfg(feature = "test-api")]
        let hooks = configured.as_ref().map_or(hooks, |lease| lease.0);
        let started = catch_unwind(AssertUnwindSafe(|| {
            self.launch(config, activation, owner, hooks)
        }))
        .unwrap_or(false);
        #[cfg(feature = "test-api")]
        drop(configured);
        started
    }

    fn launch(
        &mut self,
        config: WorkConfig,
        activation: u32,
        owner: ModuleOwner,
        hooks: Hooks,
    ) -> bool {
        let Ok(lease) = HookLease::acquire(hooks) else {
            return false;
        };
        let Ok(stop) = hooks.event(Error::Startup) else {
            return false;
        };
        let state = Arc::new(State {
            config,
            activation,
            stop,
            revoked: AtomicBool::new(false),
            mailbox: Mutex::new(None),
            hooks,
        });
        let mut launch = Box::new(Launch {
            state: Arc::clone(&state),
            module: HMODULE::default(),
            lease,
        });
        if owner == ModuleOwner::Dll {
            // SAFETY: FROM_ADDRESS interprets the callback address as code. Output belongs to Launch.
            let pinned = unsafe {
                native_pin(
                    hooks.native.pin,
                    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                    PCWSTR(work as *const () as *const u16),
                    &mut launch.module,
                )
            };

            if pinned.value == 0 || launch.module.is_invalid() {
                return false;
            }
        }
        WORKERS.fetch_add(1, Ordering::AcqRel);
        let raw = Box::into_raw(launch);
        // SAFETY: submission transfers the owned Launch to one callback on success only.
        let submitted = unsafe { native_submit(hooks.native.submit, Some(work), raw.cast()) };

        if submitted.value == 0 {
            // SAFETY: exact failed submission did not transfer ownership to a callback.
            let launch = unsafe { Box::from_raw(raw) };
            let module = launch.module;
            drop(launch);
            WORKERS.fetch_sub(1, Ordering::AcqRel);
            if !module.is_invalid() {
                // SAFETY: the unsubmitted callback still owns the successful module reference.
                let _ = unsafe { FreeLibrary(module) };
            }
            return false;
        }
        self.state = Some(state);
        true
    }

    pub(crate) fn stop(&mut self) {
        if let Some(state) = self.state.take() {
            state.revoked.store(true, Ordering::Release);
            // SAFETY: state retains the event and no wait is performed on the owner thread.
            let _ = unsafe { SetEvent(state.stop.0) };
        }
    }

    pub(crate) fn poll(&mut self) -> Option<Ready> {
        self.state.as_ref()?.mailbox.try_lock().ok()?.take()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
#[path = "vocabulary_tests.rs"]
mod tests;

#[cfg(feature = "test-api")]
mod test_api {
    use super::*;
    use std::ptr::{null, null_mut};

    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "system" fn KeyrillTestWorkerHooks(hooks: *const Hooks) -> BOOL {
        BOOL::from(
            catch_unwind(AssertUnwindSafe(|| {
                let replacement = if hooks.is_null() {
                    None
                } else {
                    // SAFETY: the test caller lends a complete table through its retain callback.
                    let Ok(lease) = HookLease::acquire(unsafe { *hooks }) else {
                        return false;
                    };
                    Some(Arc::new(lease))
                };
                let retired = {
                    let Ok(mut destination) = TEST_HOOKS.get_or_init(|| Mutex::new(None)).lock()
                    else {
                        return false;
                    };
                    std::mem::replace(&mut *destination, replacement)
                };
                drop(retired);
                true
            }))
            .unwrap_or(false),
        )
    }

    #[repr(C)]
    pub(crate) struct RawSource {
        default_source: u32,
        path: *const u16,
        length: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    pub(crate) struct RawSourceReady {
        status: u32,
        error: u32,
        dictionary: *const keyrill_ffi::Dictionary,
        owner: *const DictionaryOwner,
    }

    #[repr(C)]
    #[derive(Default)]
    pub(crate) struct RawReady {
        activation: u32,
        model: u32,
        revision: u64,
        error: u32,
        reserved: u32,
        sources: [RawSourceReady; 2],
    }

    #[unsafe(no_mangle)]
    pub(crate) extern "system" fn KeyrillTestWorkerCreate() -> *mut Worker {
        catch_unwind(|| Box::into_raw(Box::new(Worker::default()))).unwrap_or(null_mut())
    }

    fn source(input: &RawSource) -> Option<Source> {
        if input.default_source > 1 || input.length > 4095 {
            return None;
        }
        let path = if input.path.is_null() {
            if input.length != 0 || input.default_source == 0 {
                return None;
            }
            None
        } else {
            // SAFETY: test callers lend an input UTF-16 span for the complete export call.
            Some(parse_path(unsafe {
                std::slice::from_raw_parts(input.path, input.length as usize)
            })?)
        };
        if input.default_source != 0 {
            Some(Source::Default(path))
        } else {
            Some(Source::Explicit(path?))
        }
    }

    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "system" fn KeyrillTestWorkerStart(
        worker: *mut Worker,
        sources: *const RawSource,
        activation: u32,
        dll_owner: u32,
        hooks: *const Hooks,
    ) -> BOOL {
        if worker.is_null() || sources.is_null() || hooks.is_null() || dll_owner > 1 {
            return BOOL(0);
        }
        BOOL::from(
            catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: validated test inputs contain exactly two sources and live worker/hooks values.
                let (worker, sources, hooks) =
                    unsafe { (&mut *worker, std::slice::from_raw_parts(sources, 2), *hooks) };
                let Some(first) = source(&sources[0]) else {
                    return false;
                };
                let Some(second) = source(&sources[1]) else {
                    return false;
                };
                worker.start(
                    &Model {
                        sources: [first, second],
                    },
                    activation,
                    if dll_owner != 0 {
                        ModuleOwner::Dll
                    } else {
                        ModuleOwner::Process
                    },
                    hooks,
                )
            }))
            .unwrap_or(false),
        )
    }

    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "system" fn KeyrillTestWorkerStop(worker: *mut Worker) {
        if !worker.is_null() {
            // SAFETY: the caller owns this worker and serializes owner-side operations.
            let _ = catch_unwind(AssertUnwindSafe(|| unsafe { &mut *worker }.stop()));
        }
    }

    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "system" fn KeyrillTestWorkerPoll(
        worker: *mut Worker,
        output: *mut RawReady,
    ) -> BOOL {
        if worker.is_null() || output.is_null() {
            return BOOL(0);
        }
        BOOL::from(
            catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: the caller serializes worker access and lends distinct writable output storage.
                let Some(ready) = unsafe { &mut *worker }.poll() else {
                    return false;
                };
                let sources = ready.sources.map(|source| {
                    let (dictionary, owner) = match source.dictionary {
                        Some(dictionary) => (dictionary.pointer(), Arc::into_raw(dictionary)),
                        None => (null(), null()),
                    };
                    RawSourceReady {
                        status: source.status as u32,
                        error: source.error as u32,
                        dictionary,
                        owner,
                    }
                });
                // SAFETY: output is one complete caller-owned RawReady and receives the token ownership.
                unsafe {
                    output.write(RawReady {
                        activation: ready.activation,
                        model: u32::from(ready.model),
                        revision: ready.revision,
                        error: ready.error as u32,
                        reserved: 0,
                        sources,
                    })
                };
                true
            }))
            .unwrap_or(false),
        )
    }

    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "system" fn KeyrillTestWorkerFree(worker: *mut Worker) {
        if !worker.is_null() {
            // SAFETY: this consumes the worker token exactly once after owner calls have stopped.
            let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(worker) })));
        }
    }

    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "system" fn KeyrillTestWorkerDictionaryRelease(
        owner: *const DictionaryOwner,
    ) {
        if !owner.is_null() {
            // SAFETY: this consumes one returned Arc token exactly once after dictionary borrows drain.
            let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Arc::from_raw(owner) })));
        }
    }

    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "system" fn KeyrillTestWorkerParsePath(
        text: *const u16,
        length: u32,
    ) -> BOOL {
        if text.is_null() || length > 4095 {
            return BOOL(0);
        }
        BOOL::from(
            catch_unwind(|| {
                // SAFETY: the test caller lends the bounded input span for this call.
                parse_path(unsafe { std::slice::from_raw_parts(text, length as usize) }).is_some()
            })
            .unwrap_or(false),
        )
    }
}
