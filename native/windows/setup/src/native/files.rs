use super::registry::{self, wide};
use crate::Result;
use keyrill_windows_build::Digest;
use keyrill_windows_build::artifact::Package;
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree, MAX_PATH};
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY};
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::*;
use windows_core::{Interface, PCWSTR, PWSTR};

const DACL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";
static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);
struct Local(*mut core::ffi::c_void);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: LocalAlloc-compatible Win32 APIs allocated this single owned pointer.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0)));
        }
    }
}
fn path_wide(path: &Path) -> Result<Vec<u16>> {
    Ok(wide(path.to_str().ok_or("non-Unicode installation path")?))
}
fn known(id: &windows_core::GUID) -> Result<PathBuf> {
    // SAFETY: fixed documented known-folder identifiers, no creation flags or token override.
    let path = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }?;
    // SAFETY: successful SHGetKnownFolderPath returns a terminated CoTaskMem string.
    let result = unsafe { path.to_string() };
    // SAFETY: the API explicitly transfers this CoTaskMem allocation to the caller.
    unsafe {
        CoTaskMemFree(Some(path.0.cast()));
    }
    Ok(PathBuf::from(result?))
}
pub(super) fn program_files() -> Result<PathBuf> {
    let value = registry::read(
        HKEY_LOCAL_MACHINE,
        KEY_WOW64_64KEY,
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion",
        "ProgramFilesDir",
    )?;
    let directory = PathBuf::from(registry::decode_string(&value)?);
    if !directory.is_absolute() {
        return Err("machine Program Files path is not absolute".into());
    }
    ordinary_ancestors(&directory)?;
    let directory = directory.canonicalize()?;
    match known(&FOLDERID_ProgramFilesX64) {
        Ok(known) if known.canonicalize()? != directory => {
            return Err("machine Program Files and native known folder differ".into());
        }
        Err(error) if cfg!(target_pointer_width = "64") => return Err(error),
        _ => {}
    }
    Ok(directory)
}
pub(super) fn programs() -> Result<PathBuf> {
    known(&FOLDERID_CommonPrograms)
}
pub(super) fn require_elevated() -> Result<()> {
    let mut token = HANDLE::default();
    // SAFETY: pseudo process handle is valid and token owns a separate output slot.
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut length = 0;
    // SAFETY: correctly sized TOKEN_ELEVATION output remains live throughout this query.
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )
    };
    // SAFETY: OpenProcessToken transferred one owned token handle.
    unsafe {
        CloseHandle(token)?;
    }
    result?;
    if elevation.TokenIsElevated == 0 {
        return Err("machine installation requires explicit administrator launch; no elevation was requested and no changes were made".into());
    }
    Ok(())
}
pub(super) fn user_sid() -> Result<Vec<u8>> {
    use windows::Win32::Security::{GetLengthSid, IsValidSid, TOKEN_USER, TokenUser};
    let mut token = HANDLE::default();
    // SAFETY: the process pseudo-handle is valid and token output is writable.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }?;
    let mut storage = [0usize; 1024];
    let mut length = 0;
    // SAFETY: aligned storage has the complete advertised writable capacity.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(storage.as_mut_ptr().cast()),
            size_of_val(&storage) as u32,
            &mut length,
        )
    };
    // SAFETY: this function owns the successful token handle and closes it exactly once.
    unsafe { CloseHandle(token) }?;
    read?;
    if (length as usize) < size_of::<TOKEN_USER>() {
        return Err("repair user identity unavailable".into());
    }
    // SAFETY: successful TokenUser returned its SID in the retained aligned buffer.
    let sid = unsafe { (*storage.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    // SAFETY: the SID belongs to the successful native TokenUser result.
    if !unsafe { IsValidSid(sid) }.as_bool() {
        return Err("repair user SID invalid".into());
    }
    // SAFETY: IsValidSid established a complete SID in retained storage.
    let bytes =
        unsafe { std::slice::from_raw_parts(sid.0.cast::<u8>(), GetLengthSid(sid) as usize) };
    Ok(bytes.to_vec())
}
pub(super) fn ordinary(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    {
        return Err("installation path is a reparse point".into());
    }
    Ok(())
}
pub(super) fn ordinary_ancestors(path: &Path) -> Result<()> {
    for parent in path.ancestors() {
        if parent.try_exists()? {
            ordinary(parent)?;
        }
    }
    Ok(())
}
fn private_root(path: &Path) -> Result<()> {
    ordinary_ancestors(path)?;
    let path_w = path_wide(path)?;
    if !path.exists() {
        let text = wide(DACL);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: fixed SDDL and distinct allocated descriptor output.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )?;
        }
        let _descriptor = Local(descriptor.0);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        // SAFETY: descriptor and terminated path remain live; no inherited writable user ACL.
        unsafe {
            CreateDirectoryW(PCWSTR(path_w.as_ptr()), Some(&attributes))?;
        }
    }
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: named ordinary root and caller-owned descriptor output.
    unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(path_w.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            &mut descriptor,
        )
    }
    .ok()?;
    let _descriptor = Local(descriptor.0);
    let mut text = PWSTR::null();
    // SAFETY: the returned descriptor is live and text receives an allocated SDDL string.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            None,
        )?;
    }
    let _text = Local(text.0.cast());
    // SAFETY: successful conversion returned a terminated SDDL string.
    if unsafe { text.to_string()? } != DACL {
        return Err("installation root ACL differs from the private installer contract".into());
    }
    Ok(())
}
pub(super) fn lock(root: &Path) -> Result<File> {
    private_root(root)?;
    let path = root.join("writer.lock");
    ordinary_ancestors(&path)?;
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(path)
        .map_err(|_| "another installer owns the private writer lock, or access is denied")?)
}
pub(super) fn create(root: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    ordinary_ancestors(path)?;
    let temporary = root.join(format!(
        "incoming-{}-{}",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    ordinary_ancestors(&temporary)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    let from = path_wide(&temporary)?;
    let to = path_wide(path)?;
    // SAFETY: the complete private scratch file is moved without replacing an existing target.
    let moved = unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if let Err(error) = moved {
        fs::remove_file(temporary)?;
        return Err(error.into());
    }
    if fs::read(path)? != bytes {
        return Err("staged file readback mismatch".into());
    }
    Ok(())
}
pub(super) fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    ordinary_ancestors(path)?;
    let pending = path.with_extension(format!(
        "next-{}-{}",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    ordinary_ancestors(&pending)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(&pending)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    let from = path_wide(&pending)?;
    let to = path_wide(path)?;
    // SAFETY: both ordinary files are under the exclusive private installer owner.
    let moved = unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if let Err(error) = moved {
        fs::remove_file(&pending)?;
        return Err(error.into());
    }
    if fs::read(path)? != bytes {
        return Err("durable receipt readback mismatch".into());
    }
    Ok(())
}
pub(super) fn read(path: &Path) -> Result<Value> {
    if !path.try_exists()? {
        return Ok(Value::Null);
    }
    ordinary(path)?;
    if fs::metadata(path)?.len() > 65536 {
        return Err("owned shortcut exceeds bound".into());
    }
    Ok(json!(fs::read(path)?))
}
pub(super) fn write(path: &Path, value: &Value) -> Result<()> {
    ordinary_ancestors(path)?;
    if value.is_null() {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    let values = value
        .as_array()
        .filter(|v| v.len() <= 65536)
        .ok_or("shortcut receipt invalid")?;
    let bytes: Vec<_> = values
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or("shortcut receipt byte invalid")
        })
        .collect::<std::result::Result<_, _>>()?;
    atomic(path, &bytes)
}
pub(super) fn shortcut(root: &Path, target: &Path) -> Result<Value> {
    let canonical = target.canonicalize()?;
    let mut components = canonical.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err("settings shortcut target has no drive prefix".into());
    };
    let Prefix::VerbatimDisk(drive) = prefix.kind() else {
        return Err("settings shortcut target requires an ordinary drive path".into());
    };
    // ShellLink rejects verbatim paths. Keep the filesystem identity in canonical form.
    let mut shell_target = PathBuf::from(format!("{}:", char::from(drive)));
    shell_target.extend(components);
    let target = path_wide(&shell_target)?;
    if target.len() > MAX_PATH as usize {
        return Err("settings shortcut target exceeds the Shell path bound".into());
    }
    if shell_target.canonicalize()? != canonical {
        return Err("settings shortcut target identity changes in Shell path form".into());
    }
    let path = root.join("settings-link.next");
    ordinary_ancestors(&path)?;
    // SAFETY: COM apartment is initialized and ShellLink returns an owned SDK interface.
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }?;
    // SAFETY: the explicitly installed executable path is live and terminated.
    unsafe {
        link.SetPath(PCWSTR(target.as_ptr()))
            .map_err(|error| format!("settings shortcut IShellLinkW::SetPath failed: {error}"))?;
        link.SetDescription(windows_core::w!("Keyrill Settings"))?;
    }
    let persist: IPersistFile = link.cast()?;
    let output = path_wide(&path)?;
    // SAFETY: the private scratch path and retained shell-link object remain valid.
    unsafe {
        persist.Save(PCWSTR(output.as_ptr()), true)?;
    }
    let result = read(&path)?;
    fs::remove_file(path)?;
    Ok(result)
}
pub(super) fn cleanup_version(root: &Path, directory: &Path) -> Result<bool> {
    match cleanup_attempt(root, directory) {
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| matches!(e.raw_os_error(), Some(32 | 33))) =>
        {
            Ok(false)
        }
        result => result,
    }
}
fn cleanup_attempt(root: &Path, directory: &Path) -> Result<bool> {
    ordinary_ancestors(directory)?;
    let identity = directory
        .file_name()
        .and_then(|v| v.to_str())
        .filter(|v| keyrill_windows_build::artifact::is_digest(v))
        .ok_or("invalid cleanup version")?;
    let receipt = root.join(format!("cleanup-{identity}.json"));
    let files = if receipt.exists() {
        ordinary(&receipt)?;
        let value: Value = serde_json::from_slice(&fs::read(&receipt)?)?;
        if value["schema"] != 1 || value["package"] != identity {
            return Err("invalid cleanup receipt".into());
        }
        value["files"]
            .as_object()
            .ok_or("cleanup files missing")?
            .clone()
    } else {
        let package = Package::open(directory)?;
        if package.digest() != identity {
            return Err("cleanup package identity mismatch".into());
        }
        let mut files = serde_json::Map::new();
        for (name, hash) in package.files() {
            files.insert(name.clone(), json!(hash));
        }
        files.insert(
            "package.json".into(),
            json!(Digest::of_bytes(&fs::read(directory.join("package.json"))?).to_string()),
        );
        atomic(
            &receipt,
            &serde_json::to_vec(&json!({"schema":1,"package":identity,"files":files}))?,
        )?;
        files
    };
    let mut paths = Vec::new();
    let mut held = Vec::new();
    for (name, hash) in &files {
        if Path::new(name)
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
            || !keyrill_windows_build::artifact::is_digest(
                hash.as_str().ok_or("cleanup hash missing")?,
            )
        {
            return Err("cleanup receipt path or hash invalid".into());
        }
        let path = directory.join(name);
        ordinary_ancestors(&path)?;
        if !path.exists() {
            continue;
        }
        if Digest::of_bytes(&fs::read(&path)?).to_string()
            != hash.as_str().ok_or("cleanup hash missing")?
        {
            return Err("cleanup refuses changed package bytes".into());
        }
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        held.push(file);
        paths.push(path);
    }
    drop(held);
    for path in paths {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => return Ok(false),
            Err(error) => return Err(error.into()),
        }
    }
    fn empty_tree(path: &Path) -> Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            ordinary(&entry.path())?;
            if !entry.file_type()?.is_dir() {
                return Err("unexpected file retained during cleanup".into());
            }
            empty_tree(&entry.path())?;
        }
        fs::remove_dir(path)?;
        Ok(())
    }
    empty_tree(directory)?;
    fs::remove_file(receipt)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_fixture::Fixture;

    #[test]
    fn native_shortcut_round_trips_canonical_and_ordinary_targets() {
        let fixture = Fixture::new();
        let _com = super::super::tsf::Com::new().unwrap();
        let root = fixture.0.join("Keyrill settings тест");
        fs::create_dir_all(root.join("x64")).unwrap();
        fs::write(root.join("x64/KeyrillSettings.exe"), b"fixture target").unwrap();
        let canonical_root = root.canonicalize().unwrap();
        for root in [&root, &canonical_root] {
            let target = root.join("x64/KeyrillSettings.exe");
            let value = shortcut(root, &target).unwrap();
            assert!(!root.join("settings-link.next").exists());
            let saved = root.join("settings.lnk");
            write(&saved, &value).unwrap();
            // SAFETY: this thread owns a live COM apartment and the returned SDK interface.
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.unwrap();
            let persist: IPersistFile = link.cast().unwrap();
            let saved_wide = path_wide(&saved).unwrap();
            let mut actual = [0u16; 260];
            let mut description = [0u16; 260];
            // SAFETY: the saved fixture and initialized bounded output buffers remain live.
            unsafe {
                persist
                    .Load(PCWSTR(saved_wide.as_ptr()), STGM_READ)
                    .unwrap();
                link.GetPath(&mut actual, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
                    .unwrap();
                link.GetDescription(&mut description).unwrap();
            }
            let actual =
                String::from_utf16(&actual[..actual.iter().position(|unit| *unit == 0).unwrap()])
                    .unwrap();
            assert_eq!(
                Path::new(&actual).canonicalize().unwrap(),
                target.canonicalize().unwrap()
            );
            let description = String::from_utf16(
                &description[..description.iter().position(|unit| *unit == 0).unwrap()],
            )
            .unwrap();
            assert_eq!(description, "Keyrill Settings");
        }
    }

    #[test]
    fn native_shortcut_refuses_shell_path_identity_changes() {
        let fixture = Fixture::new();
        let _com = super::super::tsf::Com::new().unwrap();
        let root = fixture.0.canonicalize().unwrap();
        fs::write(root.join("KeyrillSettings.exe"), b"different target").unwrap();
        let target = root.join("KeyrillSettings.exe ");
        fs::write(&target, b"exact target").unwrap();
        let error = shortcut(&root, &target).unwrap_err();
        assert_eq!(
            error.to_string(),
            "settings shortcut target identity changes in Shell path form"
        );
        assert!(!root.join("settings-link.next").exists());
    }

    #[test]
    fn native_shortcut_refuses_targets_beyond_the_shell_path_bound() {
        let fixture = Fixture::new();
        let _com = super::super::tsf::Com::new().unwrap();
        let root = fixture.0.canonicalize().unwrap();
        let directory = root.join("x".repeat(160)).join("y".repeat(80));
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("KeyrillSettings.exe");
        fs::write(&target, b"exact target").unwrap();
        let error = shortcut(&root, &target).unwrap_err();
        assert_eq!(
            error.to_string(),
            "settings shortcut target exceeds the Shell path bound"
        );
        assert!(!root.join("settings-link.next").exists());
    }

    #[test]
    fn executable_manifest_explicitly_preserves_the_callers_privileges() {
        use windows::Win32::System::LibraryLoader::*;
        // SAFETY: the current module stays loaded and owns its bounded manifest resource.
        let manifest = unsafe {
            let module = GetModuleHandleW(None).unwrap();
            let resource = FindResourceW(
                Some(module),
                PCWSTR(std::ptr::without_provenance(1)),
                PCWSTR(std::ptr::without_provenance(24)),
            );
            assert!(!resource.is_invalid());
            let length = SizeofResource(Some(module), resource) as usize;
            assert!(length > 0 && length < 65536);
            let loaded = LoadResource(Some(module), resource).unwrap();
            let pointer = LockResource(loaded);
            assert!(!pointer.is_null());
            std::str::from_utf8(std::slice::from_raw_parts(pointer.cast::<u8>(), length)).unwrap()
        };
        assert!(manifest.contains("level=\"asInvoker\""));
        assert!(manifest.contains("uiAccess=\"false\""));
        assert!(!manifest.contains("autoElevate"));
    }
    #[test]
    fn native_program_files_matches_the_explicit_machine_64_bit_directory() {
        let value = super::super::registry::read(
            windows::Win32::System::Registry::HKEY_LOCAL_MACHINE,
            windows::Win32::System::Registry::KEY_WOW64_64KEY,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion",
            "ProgramFilesDir",
        )
        .unwrap();
        let expected = PathBuf::from(super::super::registry::decode_string(&value).unwrap());
        assert_eq!(
            program_files().unwrap().canonicalize().unwrap(),
            expected.canonicalize().unwrap()
        );
    }
    #[test]
    fn native_private_root_dacl_survives_filesystem_normalization() {
        let fixture = Fixture::new();
        let root = fixture.0.join("private-root");
        let result = private_root(&root);
        fs::remove_dir(&root).unwrap();
        result.unwrap();
    }
    #[test]
    fn native_atomic_receipt_and_exclusive_writer_preserve_bytes() {
        let fixture = Fixture::new();
        let path = fixture.0.join("receipt.json");
        atomic(&path, b"before").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"before");
        atomic(&path, b"after").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"after");
        let held = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(
            OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&path)
                .is_err()
        );
        assert!(atomic(&path, b"not published").is_err());
        drop(held);
        assert_eq!(fs::read(&path).unwrap(), b"after");
        atomic(&path, b"recovered").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"recovered");
    }
    #[test]
    fn cleanup_retains_in_use_files_and_resumes_exact_partial_deletion() {
        let source = Fixture::new();
        source.candidate();
        let package = Package::open(&source.0).unwrap();
        let install = Fixture::new();
        let version = install.0.join("versions").join(package.digest());
        for name in package
            .files()
            .keys()
            .map(String::as_str)
            .chain(std::iter::once("package.json"))
        {
            let target = version.join(name);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(source.0.join(name), target).unwrap();
        }
        let blocked = version.join("x64/KeyrillTsf.dll");
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(&blocked)
            .unwrap();
        assert!(!cleanup_version(&install.0, &version).unwrap());
        assert!(version.join("LICENSE").exists());
        drop(held);
        fs::remove_file(version.join("LICENSE")).unwrap();
        assert!(cleanup_version(&install.0, &version).unwrap());
        assert!(!version.exists());
        assert!(
            !install
                .0
                .join(format!("cleanup-{}.json", package.digest()))
                .exists()
        );
    }
    #[test]
    fn cleanup_refuses_modified_bytes_without_deleting_other_files() {
        let source = Fixture::new();
        source.candidate();
        let package = Package::open(&source.0).unwrap();
        let install = Fixture::new();
        let version = install.0.join("versions").join(package.digest());
        for name in package
            .files()
            .keys()
            .map(String::as_str)
            .chain(std::iter::once("package.json"))
        {
            let target = version.join(name);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(source.0.join(name), target).unwrap();
        }
        fs::write(version.join("LICENSE"), b"foreign replacement").unwrap();
        assert!(cleanup_version(&install.0, &version).is_err());
        assert!(version.join("x64/KeyrillTsf.dll").exists());
    }
}
