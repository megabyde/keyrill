//! Concrete registry, entropy and cross-session writer-object boundary.

use super::{MAX_BYTES, SettingsError};
use std::marker::PhantomData;
use std::rc::Rc;
use windows::Win32::Foundation::*;
use windows::Win32::Globalization::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::System::Com::CoCreateGuid;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows_core::{BOOL, PCWSTR, PWSTR, w};

const VALUE: PCWSTR = w!("Preferences");
const VIEW: REG_SAM_FLAGS = KEY_WOW64_64KEY;

type Query =
    unsafe extern "system" fn(HKEY, PCWSTR, *const u32, *mut u32, *mut u8, *mut u32) -> WIN32_ERROR;
type Set = unsafe extern "system" fn(HKEY, PCWSTR, u32, u32, *const u8, u32) -> WIN32_ERROR;
windows_core::link!("advapi32.dll" "system" fn RegQueryValueExW(key: HKEY, name: PCWSTR, reserved: *const u32, kind: *mut u32, bytes: *mut u8, length: *mut u32) -> WIN32_ERROR);
windows_core::link!("advapi32.dll" "system" fn RegSetValueExW(key: HKEY, name: PCWSTR, reserved: u32, kind: u32, bytes: *const u8, length: u32) -> WIN32_ERROR);

#[derive(Clone)]
pub(super) struct Store {
    path: Vec<u16>,
    name: Vec<u16>,
    security: String,
    query: Query,
    set: Set,
}

impl Store {
    #[cfg(any(test, feature = "test-api"))]
    pub(super) fn isolated_test(namespace: &str) -> Result<Self, SettingsError> {
        if namespace.len() != 32 || !namespace.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(SettingsError::Invalid);
        }
        let mut store = Self::current_user()?;
        store.path = wide(&format!("Software\\KeyrillTests\\Settings-{namespace}"));
        let base = String::from_utf16(&store.name[..store.name.len() - 1])
            .map_err(|_| SettingsError::Invalid)?;
        store.name = wide(&format!("{base}.Tests.{namespace}"));
        Ok(store)
    }

    pub(super) fn current_user() -> Result<Self, SettingsError> {
        let sid = user_sid()?;
        Ok(Self {
            path: wide("Software\\Keyrill"),
            name: wide(&format!("Global\\Keyrill.Settings.{sid}")),
            security: format!("O:{sid}D:P(A;;0x001f0001;;;{sid})S:(ML;;NW;;;ME)"),
            query: RegQueryValueExW,
            set: RegSetValueExW,
        })
    }

    pub(super) fn read(&self) -> Result<Option<Vec<u8>>, SettingsError> {
        let mut key = HKEY::default();
        // SAFETY: path is terminated and output is distinct writable handle storage.
        let result = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(self.path.as_ptr()),
                None,
                KEY_QUERY_VALUE | VIEW,
                &mut key,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        result.ok().map_err(unavailable)?;
        let key = Key(key);
        let mut bytes = vec![0; MAX_BYTES];
        let mut length = bytes.len() as u32;
        let mut kind = 0;
        // SAFETY: the live key and complete mutable buffer/output lengths survive this call.
        let result = unsafe {
            (self.query)(
                key.0,
                VALUE,
                std::ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut length,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if result == ERROR_MORE_DATA
            || (result == ERROR_SUCCESS && (kind != REG_BINARY.0 || length as usize > MAX_BYTES))
        {
            return Err(SettingsError::Invalid);
        }
        result.ok().map_err(unavailable)?;
        bytes.truncate(length as usize);
        Ok(Some(bytes))
    }

    pub(super) fn write(&self, bytes: &[u8]) -> Result<(), SettingsError> {
        let mut key = HKEY::default();
        // SAFETY: path is terminated, no class/security pointer is supplied, output is writable.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(self.path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | VIEW,
                None,
                &mut key,
                None,
            )
        }
        .ok()
        .map_err(unavailable)?;
        let key = Key(key);
        // SAFETY: the key and complete validated byte buffer remain live through the one write.
        unsafe {
            (self.set)(
                key.0,
                VALUE,
                0,
                REG_BINARY.0,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        }
        .ok()
        .map_err(unavailable)
    }

    pub(super) fn lock(&self) -> Result<Writer, SettingsError> {
        let descriptor = Descriptor::from_sddl(&self.security)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: BOOL(0),
        };
        // SAFETY: security storage and terminated name survive creation. Ownership starts at wait.
        let mutex = Handle(
            unsafe { CreateMutexW(Some(&attributes), false, PCWSTR(self.name.as_ptr())) }
                .map_err(unavailable)?,
        );
        verify_security(mutex.0, descriptor.0)?;
        // SAFETY: the mutex is live. Wait is bounded and always followed by a fresh store load.
        let result = unsafe { WaitForSingleObject(mutex.0, 5000) };
        if result != WAIT_OBJECT_0 && result != WAIT_ABANDONED {
            return Err(SettingsError::Unavailable);
        }
        Ok(Writer {
            mutex,
            _thread: PhantomData,
        })
    }
}

pub(super) struct Writer {
    mutex: Handle,
    _thread: PhantomData<Rc<()>>,
}

impl Drop for Writer {
    fn drop(&mut self) {
        // SAFETY: Writer is thread-bound and owns one successful mutex acquisition.
        let _ = unsafe { ReleaseMutex(self.mutex.0) };
    }
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: Key uniquely owns this registry handle.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: Handle uniquely owns this native handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct Descriptor(PSECURITY_DESCRIPTOR);
impl Descriptor {
    fn from_sddl(text: &str) -> Result<Self, SettingsError> {
        let text = wide(text);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: input is terminated and output is writable. Success transfers a LocalFree owner.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(unavailable)?;
        Ok(Self(descriptor))
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: this descriptor was allocated by the SDDL conversion API.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

fn verify_security(handle: HANDLE, expected: PSECURITY_DESCRIPTOR) -> Result<(), SettingsError> {
    let mut storage = vec![0usize; 1024];
    let mut length = 0;
    let actual = PSECURITY_DESCRIPTOR(storage.as_mut_ptr().cast());
    // SAFETY: aligned output storage is live and its full byte capacity is supplied.
    unsafe {
        GetKernelObjectSecurity(
            handle,
            OWNER_SECURITY_INFORMATION.0
                | DACL_SECURITY_INFORMATION.0
                | LABEL_SECURITY_INFORMATION.0,
            Some(actual),
            (storage.len() * size_of::<usize>()) as u32,
            &mut length,
        )
    }
    .map_err(|_| SettingsError::Security)?;
    // SAFETY: the native API returned a complete descriptor in this aligned buffer.
    unsafe {
        let mut owner = PSID::default();
        let mut expected_owner = PSID::default();
        let mut defaulted = BOOL(0);
        GetSecurityDescriptorOwner(actual, &mut owner, &mut defaulted)
            .map_err(|_| SettingsError::Security)?;
        GetSecurityDescriptorOwner(expected, &mut expected_owner, &mut defaulted)
            .map_err(|_| SettingsError::Security)?;
        if owner.0.is_null()
            || expected_owner.0.is_null()
            || EqualSid(owner, expected_owner).is_err()
        {
            return Err(SettingsError::Security);
        }
        let actual_acl = acl_bytes(actual)?;
        let expected_acl = acl_bytes(expected)?;
        if actual_acl != expected_acl {
            return Err(SettingsError::Security);
        }
        if label_bytes(actual)? != label_bytes(expected)? {
            return Err(SettingsError::Security);
        }
    }
    Ok(())
}

// The caller retains a valid native security descriptor through the ACL copy.
unsafe fn acl_bytes(descriptor: PSECURITY_DESCRIPTOR) -> Result<Vec<u8>, SettingsError> {
    let mut present = BOOL(0);
    let mut defaulted = BOOL(0);
    let mut acl = std::ptr::null_mut();
    // SAFETY: the caller supplies a valid retained descriptor and outputs are writable.
    unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) }
        .map_err(|_| SettingsError::Security)?;
    if !present.as_bool() || acl.is_null() || defaulted.as_bool() {
        return Err(SettingsError::Security);
    }
    // SAFETY: this complete ACL belongs to the retained native descriptor.
    Ok(unsafe { std::slice::from_raw_parts(acl.cast(), (*acl).AclSize as usize) }.to_vec())
}

// The caller retains a valid descriptor containing only mandatory labels in its SACL.
unsafe fn label_bytes(descriptor: PSECURITY_DESCRIPTOR) -> Result<Vec<u8>, SettingsError> {
    let mut present = BOOL(0);
    let mut defaulted = BOOL(0);
    let mut acl = std::ptr::null_mut();
    // SAFETY: the caller supplies a valid retained descriptor and outputs are writable.
    unsafe { GetSecurityDescriptorSacl(descriptor, &mut present, &mut acl, &mut defaulted) }
        .map_err(|_| SettingsError::Security)?;
    if !present.as_bool() || acl.is_null() || defaulted.as_bool() {
        return Err(SettingsError::Security);
    }
    // SAFETY: this complete ACL belongs to the retained native descriptor.
    Ok(unsafe { std::slice::from_raw_parts(acl.cast(), (*acl).AclSize as usize) }.to_vec())
}

fn user_sid() -> Result<String, SettingsError> {
    let mut token = HANDLE::default();
    // SAFETY: process pseudo-handle is valid and token output is writable.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(unavailable)?;
    let token = Handle(token);
    let mut storage = vec![0usize; 1024];
    let mut length = 0;
    // SAFETY: aligned output storage is live with its full byte capacity supplied.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(storage.as_mut_ptr().cast()),
            (storage.len() * size_of::<usize>()) as u32,
            &mut length,
        )
    }
    .map_err(unavailable)?;
    // SAFETY: TokenUser returned a complete TOKEN_USER and its SID in retained storage.
    let sid = unsafe { (*storage.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text = PWSTR::null();
    // SAFETY: the SID is live, and output receives a LocalFree-owned terminated string.
    unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(unavailable)?;
    // SAFETY: the successful conversion returned a terminated string retained until LocalFree.
    let result = unsafe { text.to_string() }.map_err(|_| SettingsError::Unavailable);
    // SAFETY: successful ConvertSidToStringSidW allocated this output with LocalAlloc.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    result
}

pub(super) fn nonce() -> Result<[u8; 16], SettingsError> {
    // SAFETY: CoCreateGuid has no preconditions or COM initialization requirement.
    let guid = unsafe { CoCreateGuid() }.map_err(unavailable)?;
    let value = guid.to_u128().to_le_bytes();
    if value == [0; 16] {
        return Err(SettingsError::Unavailable);
    }
    Ok(value)
}

pub(super) fn uppercase(text: &str) -> Result<String, SettingsError> {
    let source: Vec<u16> = text.encode_utf16().collect();
    let mut output = vec![0; 4096];
    // SAFETY: bounded input and distinct complete mutable output survive the synchronous map.
    let count = unsafe {
        LCMapStringEx(
            LOCALE_NAME_INVARIANT,
            LCMAP_UPPERCASE,
            &source,
            Some(&mut output),
            None,
            None,
            LPARAM(0),
        )
    };
    if count <= 0 || count > 4095 {
        return Err(SettingsError::Invalid);
    }
    String::from_utf16(&output[..count as usize]).map_err(|_| SettingsError::Invalid)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn unavailable(_: windows_core::Error) -> SettingsError {
    SettingsError::Unavailable
}

#[cfg(test)]
pub(super) mod testing {
    use super::*;

    pub(in crate::settings) fn isolated() -> Store {
        let id = nonce().expect("test namespace entropy");
        let suffix = format!("{:032x}", u128::from_le_bytes(id));
        open_isolated(&suffix)
    }

    pub(in crate::settings) fn open_isolated(suffix: &str) -> Store {
        assert_eq!(suffix.len(), 32);
        assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let mut store = Store::current_user().expect("test process user");
        store.path = wide(&format!("Software\\KeyrillTests\\Settings-{suffix}"));
        let base = String::from_utf16(&store.name[..store.name.len() - 1]).unwrap();
        store.name = wide(&format!("{base}.Tests.{suffix}"));
        store
    }

    pub(in crate::settings) fn namespace(store: &Store) -> String {
        let path = String::from_utf16(&store.path[..store.path.len() - 1]).unwrap();
        path.strip_prefix("Software\\KeyrillTests\\Settings-")
            .unwrap()
            .to_owned()
    }

    pub(in crate::settings) fn cleanup(store: &Store) {
        // SAFETY: isolated() constructs only a unique owned KeyrillTests child key.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(store.path.as_ptr())) }
            .ok()
            .unwrap();
    }

    pub(in crate::settings) fn delete_value(store: &Store) {
        let mut key = HKEY::default();
        // SAFETY: this opens only the unique test key in the production-selected registry view.
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(store.path.as_ptr()),
                None,
                KEY_SET_VALUE | VIEW,
                &mut key,
            )
        }
        .ok()
        .unwrap();
        let key = Key(key);
        // SAFETY: the key is retained and only the owned test value is removed.
        unsafe { RegDeleteValueW(key.0, VALUE) }.ok().unwrap();
    }

    pub(in crate::settings) fn fail_query(store: &mut Store, query: Query) {
        store.query = query;
    }

    pub(in crate::settings) fn fail_set(store: &mut Store, set: Set) {
        store.set = set;
    }

    pub(in crate::settings) fn unrelated_value(store: &Store, write: bool) {
        let mut key = HKEY::default();
        // SAFETY: this creates only a child of the unique test key in the explicit store view.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(store.path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE | VIEW,
                None,
                &mut key,
                None,
            )
        }
        .ok()
        .unwrap();
        let key = Key(key);
        if write {
            // SAFETY: the retained test key receives an unrelated sentinel value.
            unsafe {
                RegSetValueExW(
                    key.0,
                    w!("Unrelated"),
                    0,
                    REG_BINARY.0,
                    b"sentinel".as_ptr(),
                    8,
                )
            }
            .ok()
            .unwrap();
        }
        let mut bytes = [0u8; 8];
        let mut length = 8;
        let mut kind = 0;
        // SAFETY: the key and output storage remain live through the sentinel read.
        unsafe {
            RegQueryValueExW(
                key.0,
                w!("Unrelated"),
                std::ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut length,
            )
        }
        .ok()
        .unwrap();
        assert_eq!(&bytes, b"sentinel");
        assert_eq!(kind, REG_BINARY.0);
    }

    pub(in crate::settings) unsafe extern "system" fn real_query(
        key: HKEY,
        name: PCWSTR,
        reserved: *const u32,
        kind: *mut u32,
        bytes: *mut u8,
        length: *mut u32,
    ) -> WIN32_ERROR {
        // SAFETY: forwarded unchanged from the store's native boundary.
        unsafe { RegQueryValueExW(key, name, reserved, kind, bytes, length) }
    }

    pub(in crate::settings) fn wrong_type(store: &Store) {
        let mut key = HKEY::default();
        // SAFETY: the unique test key exists and output is writable.
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(store.path.as_ptr()),
                None,
                KEY_SET_VALUE | VIEW,
                &mut key,
            )
        }
        .ok()
        .unwrap();
        let key = Key(key);
        // SAFETY: this replaces only the isolated test value with a deliberate wrong native type.
        unsafe { RegSetValueExW(key.0, VALUE, 0, REG_DWORD.0, [0u8; 4].as_ptr(), 4) }
            .ok()
            .unwrap();
    }

    pub(in crate::settings) fn insecure_mutex(store: &Store) -> impl Drop {
        mutex_with_security(store, "D:(A;;GA;;;WD)")
    }

    pub(in crate::settings) fn low_integrity_mutex(store: &Store) -> impl Drop {
        let sid = user_sid().unwrap();
        mutex_with_security(
            store,
            &format!("O:{sid}D:P(A;;0x001f0001;;;{sid})S:(ML;;NW;;;LW)"),
        )
    }

    fn mutex_with_security(store: &Store, security: &str) -> Handle {
        let descriptor = Descriptor::from_sddl(security).unwrap();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: BOOL(0),
        };
        // SAFETY: the test owns this unique mutex name and security storage survives creation.
        Handle(
            unsafe { CreateMutexW(Some(&attributes), false, PCWSTR(store.name.as_ptr())) }.unwrap(),
        )
    }

    pub(in crate::settings) fn writer_integrity_label(store: &Store) -> String {
        let writer = store.lock().unwrap();
        let mut storage = vec![0usize; 1024];
        let actual = PSECURITY_DESCRIPTOR(storage.as_mut_ptr().cast());
        let mut length = 0;
        // SAFETY: the writer retains the handle and the aligned descriptor buffer is writable.
        unsafe {
            GetKernelObjectSecurity(
                writer.mutex.0,
                LABEL_SECURITY_INFORMATION.0,
                Some(actual),
                (storage.len() * size_of::<usize>()) as u32,
                &mut length,
            )
        }
        .unwrap();
        let mut text = PWSTR::null();
        // SAFETY: the queried descriptor is complete and output receives a LocalFree owner.
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                actual,
                SDDL_REVISION_1,
                LABEL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
        }
        .unwrap();
        // SAFETY: successful conversion returned a retained terminated string.
        let label = unsafe { text.to_string() }.unwrap();
        // SAFETY: the conversion allocated the string with LocalAlloc.
        unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
        label
    }

    pub(in crate::settings) fn abandon(store: &Store) -> impl Drop {
        let descriptor = Descriptor::from_sddl(&store.security).unwrap();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: BOOL(0),
        };
        // SAFETY: retain a handle so the named object survives the abandoning thread.
        let retained = Handle(
            unsafe { CreateMutexW(Some(&attributes), false, PCWSTR(store.name.as_ptr())) }.unwrap(),
        );
        let copy = store.clone();
        std::thread::spawn(move || {
            let writer = copy.lock().unwrap();
            let handle = writer.mutex.0;
            std::mem::forget(writer);
            // SAFETY: release this thread's handle without releasing ownership. The retained
            // outer handle keeps the object alive for abandonment when this thread exits.
            unsafe { CloseHandle(handle) }.unwrap();
        })
        .join()
        .unwrap();
        retained
    }
}
