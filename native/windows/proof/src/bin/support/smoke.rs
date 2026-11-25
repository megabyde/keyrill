use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{ApplicationInstallationAndServicing::*, Com::*, LibraryLoader::*, SystemServices::*},
    UI::TextServices::*,
};
use windows_core::{GUID, HRESULT, Interface, PCSTR, PCWSTR};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SERVICE: GUID = GUID::from_u128(0x1a746212_84db_4acf_88b3_caaa26352774);

struct Apartment;
impl Apartment {
    fn new() -> Result<Self> {
        // SAFETY: this child owns its main thread and initializes one balanced STA entry.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: every later-declared COM interface has drained before this STA entry.
        unsafe { CoUninitialize() };
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn module_path(module: Option<HMODULE>) -> Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    windows_core::link!("kernel32.dll" "system" fn GetModuleFileNameW(module: HMODULE, output: *mut u16, count: u32) -> u32);
    let mut buffer = vec![0; 32768];
    // SAFETY: the complete writable allocation remains mutable through the native call.
    let count = unsafe {
        GetModuleFileNameW(
            module.unwrap_or_default(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } as usize;
    if count == 0 || count >= buffer.len() {
        return Err("module path unavailable".into());
    }
    Ok(std::ffi::OsString::from_wide(&buffer[..count]).into())
}

struct Module(Option<HMODULE>);
impl Drop for Module {
    fn drop(&mut self) {
        if let Some(module) = self.0.take() {
            // SAFETY: this owner releases its sole LoadLibrary reference after local calls drain.
            let _ = unsafe { FreeLibrary(module) };
        }
    }
}

pub(super) fn factory(path: &Path) -> Result<()> {
    let _apartment = Apartment::new()?;
    let path = std::fs::canonicalize(path)?;
    let encoded = wide(&path);
    // SAFETY: the absolute zero-terminated path names the explicitly selected test artifact.
    let handle = unsafe { LoadLibraryW(PCWSTR(encoded.as_ptr()))? };
    let mut module = Module(Some(handle));
    // SAFETY: these exact published exports are retained by module through every invocation.
    let get = unsafe { GetProcAddress(handle, PCSTR(c"DllGetClassObject".as_ptr().cast())) }
        .ok_or("COM factory export absent")?;
    // SAFETY: the signature is the published DllGetClassObject ABI.
    let get: unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT =
        unsafe { std::mem::transmute(get) };
    // SAFETY: the module is live and this name is a literal published export.
    let unload = unsafe { GetProcAddress(handle, PCSTR(c"DllCanUnloadNow".as_ptr().cast())) }
        .ok_or("COM unload export absent")?;
    // SAFETY: the signature is the published DllCanUnloadNow ABI.
    let unload: unsafe extern "system" fn() -> HRESULT = unsafe { std::mem::transmute(unload) };
    // SAFETY: the export pointer remains in the loaded module and takes no arguments.
    if unsafe { unload() } != S_OK {
        return Err("fresh COM module is busy".into());
    }
    let mut output = std::ptr::null_mut();
    // SAFETY: GUID inputs and output storage are complete and nonaliasing.
    let status = unsafe { get(&SERVICE, &IClassFactory::IID, &mut output) };
    // SAFETY: a normally returned non-null COM output transfers one owned interface reference.
    let factory = (!output.is_null()).then(|| unsafe { IClassFactory::from_raw(output) });
    if status != S_OK {
        return Err("COM class factory query failed".into());
    }
    let factory = factory.ok_or("COM class factory output absent")?;
    // SAFETY: factory retains the live DLL and its count must prohibit unloading.
    if unsafe { unload() } != S_FALSE {
        return Err("live factory did not prevent unloading".into());
    }
    output = std::ptr::null_mut();
    // SAFETY: factory owns this vtable, aggregation is absent and output storage is writable.
    let status = unsafe {
        (factory.vtable().CreateInstance)(
            factory.as_raw(),
            std::ptr::null_mut(),
            &ITfTextInputProcessorEx::IID,
            &mut output,
        )
    };
    // SAFETY: a normally returned non-null COM output owns the declared interface reference.
    let service = (!output.is_null()).then(|| unsafe { ITfTextInputProcessorEx::from_raw(output) });
    if status != S_OK || service.is_none() {
        return Err("COM service creation failed".into());
    }
    drop(service);
    drop(factory);
    // SAFETY: all test-created COM references have been explicitly released.
    if unsafe { unload() } != S_OK {
        return Err("released COM module remains busy".into());
    }
    let owned = module.0.take().ok_or("module reference missing")?;
    // SAFETY: every function call has returned and every object is released before code unload.
    unsafe { FreeLibrary(owned)? };
    // SAFETY: this performs a handle query for the same explicit absolute module path.
    if unsafe { GetModuleHandleW(PCWSTR(encoded.as_ptr())) }.is_ok() {
        return Err("production module remained mapped after explicit unload".into());
    }
    println!("Registration-free factory, reference lifetime and actual DLL unload passed.");
    Ok(())
}

struct File(HANDLE);
impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: this owner holds one live file handle after all metadata calls return.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn file_identity(path: &Path) -> Result<(u32, u32, u32)> {
    let encoded = wide(path);
    // SAFETY: zero access requests metadata only, sharing permits another live DLL reader.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(encoded.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?
    };
    let file = File(handle);
    let mut identity = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: file retains a live handle and identity is a writable complete output object.
    unsafe { GetFileInformationByHandle(file.0, &mut identity)? };
    Ok((
        identity.dwVolumeSerialNumber,
        identity.nFileIndexHigh,
        identity.nFileIndexLow,
    ))
}

pub(super) fn activation(missing: bool) -> Result<()> {
    let _apartment = Apartment::new()?;
    let expected = module_path(None)?.with_file_name("KeyrillTsf.dll");
    let mut mapping = ACTCTX_SECTION_KEYED_DATA {
        cbSize: size_of::<ACTCTX_SECTION_KEYED_DATA>() as u32,
        ..Default::default()
    };
    // SAFETY: both inputs are complete GUID and activation-context metadata output objects.
    unsafe {
        FindActCtxSectionGuid(
            0,
            None,
            ACTIVATION_CONTEXT_SECTION_COM_SERVER_REDIRECTION,
            Some(&SERVICE),
            &mut mapping,
        )?
    };
    let mut output = std::ptr::null_mut();
    windows_core::link!("ole32.dll" "system" fn CoCreateInstance(class: *const GUID, outer: *mut c_void, context: u32, id: *const GUID, output: *mut *mut c_void) -> HRESULT);
    // SAFETY: the child has an STA and its manifest supplies the explicit adjacent DLL mapping.
    let result = unsafe {
        CoCreateInstance(
            &SERVICE,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER.0,
            &ITfTextInputProcessorEx::IID,
            &mut output,
        )
    };
    // SAFETY: a normally returned non-null output transfers one owned interface reference.
    let service = (!output.is_null()).then(|| unsafe { ITfTextInputProcessorEx::from_raw(output) });
    if missing {
        match std::fs::metadata(&expected) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("negative activation fixture has an adjacent artifact".into()),
        }
        // SAFETY: this queries module presence without acquiring a reference or loading code.
        if result.is_ok()
            || service.is_some()
            || unsafe { GetModuleHandleW(PCWSTR(wide(Path::new("KeyrillTsf.dll")).as_ptr())) }
                .is_ok()
        {
            return Err("missing adjacent DLL did not refuse manifest activation".into());
        }
    } else {
        if result != S_OK {
            return Err("adjacent manifest COM activation failed".into());
        }
        let service = service.ok_or("manifest activation output absent")?;
        let address = service.vtable().base__.base__.QueryInterface as *const ();
        let mut actual = HMODULE::default();
        // SAFETY: FROM_ADDRESS interprets a retained interface code address without adding a ref.
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                PCWSTR(address.cast()),
                &mut actual,
            )?
        };
        if file_identity(&expected)? != file_identity(&module_path(Some(actual))?)? {
            return Err("manifest activated a different DLL file".into());
        }
        drop(service);
    }
    println!("Adjacent manifest COM activation check passed.");
    Ok(())
}
