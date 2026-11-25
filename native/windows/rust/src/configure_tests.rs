use super::*;
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use windows::Win32::UI::WindowsAndMessaging::*;

thread_local! {
    static PUMPED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

unsafe extern "system" fn host_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_APP + 19 {
        PUMPED.set(true);
        return LRESULT(417);
    }
    // SAFETY: preserve every message outside this controlled test's private callback.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[path = "../../build-support/src/artifact/fixture.rs"]
mod fixture;

#[test]
fn package_identity_paths_and_retained_leases_refuse_replacement() {
    let fixture = fixture::Fixture::new();
    fixture.candidate();
    let package = Package::open(&fixture.0).unwrap();
    assert!(
        Launch::open(
            &fixture.0.join("x64/KeyrillTsf.dll"),
            package.production(TARGET)
        )
        .is_err()
    );
    assert!(
        Launch::open(
            &fixture.0.join(TARGET.name()).join("wrong.dll"),
            package.production(TARGET)
        )
        .is_err()
    );
    let dll = fixture.0.join(TARGET.name()).join("KeyrillTsf.dll");
    let lease = retain(&dll, false).unwrap();
    assert!(std::fs::write(&dll, b"replacement").is_err());
    assert!(std::fs::remove_file(&dll).is_err());
    let directory = retain(&fixture.0, true).unwrap();
    assert!(std::fs::rename(&fixture.0, fixture.0.with_extension("replaced")).is_err());
    drop(directory);
    drop(lease);
    std::fs::write(&dll, b"modified").unwrap();
    assert!(Package::open(&fixture.0).is_err());
    assert!(Launch::open(Path::new(r"\\server\package\x64\KeyrillTsf.dll"), "").is_err());
}

#[test]
fn package_launch_accepts_canonical_local_drive_paths() {
    let fixture = fixture::Fixture::new();
    fixture.candidate();
    let package = Package::open(&fixture.0).unwrap();
    let expected = package.production(TARGET).to_owned();
    let directory = fixture.0.join(package.digest());
    let entries: Vec<_> = std::fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    std::fs::create_dir(&directory).unwrap();
    for entry in entries {
        std::fs::rename(&entry, directory.join(entry.file_name().unwrap())).unwrap();
    }
    let module = directory.join(TARGET.name()).join("KeyrillTsf.dll");
    let canonical = std::fs::canonicalize(&module).unwrap();
    assert!(canonical.to_str().unwrap().starts_with(r"\\?\"));
    for (spelling, path) in [("ordinary", &module), ("canonical", &canonical)] {
        let launch = Launch::open_for_host(path, &expected, false, |directory| {
            Package::open(directory).map_err(|_| unavailable())
        })
        .unwrap_or_else(|error| panic!("complete local-drive package {spelling} path: {error}"));
        assert_eq!(
            launch.executable,
            path.parent().unwrap().join("KeyrillSettings.exe")
        );
    }
}

#[test]
fn package_launch_refuses_other_path_namespaces() {
    for directory in [
        r"\\server\share\package",
        r"\\?\UNC\server\share\package",
        r"\\.\C:\package",
        r"\??\C:\package",
        r"C:package",
        r"\package",
        r"package",
        r"\\?\C:package",
        r"C:\package:stream",
        r"\\?\C:\package:stream",
    ] {
        let module = Path::new(directory)
            .join(TARGET.name())
            .join("KeyrillTsf.dll");
        assert!(
            Launch::open_for_host(&module, "", false, |_| {
                panic!("an unsupported path reached package body validation")
            })
            .is_err()
        );
    }
}

#[test]
fn package_launch_refuses_changed_build_missing_files_and_wrong_pe() {
    let fixture = fixture::Fixture::new();
    fixture.candidate();
    let package = Package::open(&fixture.0).unwrap();
    let directory = fixture.0.join(package.digest());
    let expected = package.production(TARGET).to_owned();
    let entries: Vec<_> = std::fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    std::fs::create_dir(&directory).unwrap();
    for entry in entries {
        std::fs::rename(&entry, directory.join(entry.file_name().unwrap())).unwrap();
    }
    let dll = directory.join(TARGET.name()).join("KeyrillTsf.dll");
    assert!(Launch::open(&dll, &"0".repeat(64)).is_err());
    let settings = directory.join(TARGET.name()).join("KeyrillSettings.exe");
    let bytes = std::fs::read(&settings).unwrap();
    std::fs::remove_file(&settings).unwrap();
    assert!(Launch::open(&dll, &expected).is_err());
    std::fs::write(&settings, &bytes).unwrap();
    let mut changed = bytes;
    changed[68..70].copy_from_slice(&0u16.to_le_bytes());
    std::fs::write(&settings, changed).unwrap();
    assert!(Launch::open(&dll, &expected).is_err());
}

#[test]
fn privileged_acl_refusal_precedes_package_body_validation() {
    let fixture = fixture::Fixture::new();
    fixture.candidate();
    let package = Package::open(&fixture.0).unwrap();
    let expected = package.production(TARGET).to_owned();
    let directory = fixture.0.join(package.digest());
    let entries: Vec<_> = std::fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    std::fs::create_dir(&directory).unwrap();
    for entry in entries {
        std::fs::rename(&entry, directory.join(entry.file_name().unwrap())).unwrap();
    }
    let module = directory.join(TARGET.name()).join("KeyrillTsf.dll");
    let manifest = directory.join("package.json");
    let original = std::fs::read(&manifest).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&manifest)
        .unwrap()
        .set_len(1024 * 1024 + 1)
        .unwrap();
    assert!(
        Launch::open_for_host(&module, &expected, true, |_| {
            panic!("an ACL-refused elevated launch reached package body validation")
        })
        .is_err()
    );
    std::fs::write(&manifest, original).unwrap();
    let loaded = std::cell::Cell::new(false);
    let launch = Launch::open_for_host(&module, &expected, false, |directory| {
        loaded.set(true);
        Package::open(directory).map_err(|_| unavailable())
    })
    .unwrap();
    assert!(loaded.get());
    assert_eq!(
        launch.executable,
        directory.join(TARGET.name()).join("KeyrillSettings.exe")
    );
}

#[test]
fn package_reparse_and_lower_integrity_writable_peer_are_refused() {
    let fixture = fixture::Fixture::new();
    fixture.candidate();
    let file = retain(&fixture.0.join("package.json"), false).unwrap();
    assert!(
        privileged_safe(&file).is_err(),
        "a user-writable package is unavailable to an elevated host"
    );
    drop(file);
    let relocated = fixture::Fixture::new();
    let target = relocated.0.join("resources");
    std::fs::rename(fixture.0.join("resources"), &target).unwrap();
    let junction = fixture.0.join("resources");
    std::fs::create_dir(&junction).unwrap();
    let lease = retain(&junction, true).unwrap();
    create_junction(&junction, &target).unwrap();
    assert!(retain(&target.join("english.ico"), false).is_ok());
    assert!(retain(&junction.join("english.ico"), false).is_err());
    drop(lease);
    assert!(retain(&junction, true).is_err());
    let error = Package::open(&fixture.0).err().unwrap().to_string();
    assert!(
        error.contains("reparse") || error.contains("symbolic link"),
        "{error}"
    );
    let path = wide(&junction).unwrap();
    // SAFETY: remove only the created junction itself before recursive owned-fixture cleanup.
    unsafe {
        RemoveDirectoryW(PCWSTR(path.as_ptr())).unwrap();
    }
    let populated = relocated.0.join("populated");
    std::fs::create_dir(&populated).unwrap();
    let child = populated.join("retained-child");
    std::fs::write(&child, b"synthetic child").unwrap();
    let _directory = retain(&populated, true).unwrap();
    let _child = retain(&child, false).unwrap();
    let error = create_junction(&populated, &target).unwrap_err();
    assert_eq!(
        error.downcast_ref::<windows_core::Error>().unwrap().code(),
        HRESULT::from_win32(ERROR_DIR_NOT_EMPTY.0)
    );
    assert!(std::fs::remove_file(&child).is_err());
    assert!(std::fs::rename(&populated, relocated.0.join("replaced")).is_err());
}

fn create_junction(
    junction: &Path,
    target: &Path,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let directory = OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES.0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0)
        .open(junction)?;
    let substitute = format!(r"\??\{}", target.display())
        .encode_utf16()
        .collect::<Vec<_>>();
    let printable = target.to_str().unwrap().encode_utf16().collect::<Vec<_>>();
    let substitute_length = (substitute.len() * 2) as u16;
    let printable_length = (printable.len() * 2) as u16;
    let mut packet = Vec::new();
    packet.extend(0xa0000003u32.to_le_bytes());
    for word in [
        8 + substitute_length + printable_length + 4,
        0,
        0,
        substitute_length,
        substitute_length + 2,
        printable_length,
    ] {
        packet.extend(word.to_le_bytes());
    }
    for unit in substitute
        .into_iter()
        .chain(Some(0))
        .chain(printable)
        .chain(Some(0))
    {
        packet.extend(unit.to_le_bytes());
    }
    let mut returned = 0;
    // SAFETY: a complete mount-point reparse packet targets only this test's owned empty directory.
    unsafe {
        windows::Win32::System::IO::DeviceIoControl(
            HANDLE(directory.as_raw_handle()),
            0x900a4,
            Some(packet.as_ptr().cast()),
            packet.len() as u32,
            None,
            0,
            Some(&mut returned),
            None,
        )?;
    }
    Ok(())
}

#[test]
fn native_launch_failure_keeps_the_callers_quit_message() {
    let launch = Launch {
        executable: std::env::temp_dir().join("Keyrill-missing-controlled-settings.exe"),
        _leases: Vec::new(),
    };
    // SAFETY: this test owns its thread message queue and quit message.
    unsafe {
        PostQuitMessage(731);
    }
    assert!(launch.start("").is_err());
    let mut message = MSG::default();
    // SAFETY: inspect and remove only this test's own quit message.
    assert!(unsafe { PeekMessageW(&mut message, None, WM_QUIT, WM_QUIT, PM_REMOVE) }.as_bool());
    assert_eq!(message.wParam.0, 731);
}

unsafe extern "system" fn close_preview(window: HWND, found: LPARAM) -> windows_core::BOOL {
    let mut name = [0u16; 80];
    // SAFETY: the enumerator supplies a live HWND and a local writable name buffer.
    let length = unsafe { GetWindowTextW(window, &mut name) } as usize;
    if String::from_utf16_lossy(&name[..length]) == "Keyrill Settings (Preview)" {
        // SAFETY: the caller lends one writable bool through synchronous enumeration.
        unsafe {
            *(found.0 as *mut bool) = true;
        }
        // SAFETY: the child owns this preview window and handles its ordinary Close operation.
        unsafe {
            PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
        }
    }
    windows_core::BOOL(1)
}

#[test]
#[ignore = "requires the explicitly supplied same-architecture built Settings preview executable"]
fn native_settings_preview_child_waits_for_close_and_preserves_host_quit() {
    let executable = PathBuf::from(
        std::env::var_os("KEYRILL_SETTINGS_PREVIEW")
            .expect("native runner supplies built Settings"),
    );
    // SAFETY: this native test owns a fresh STA on the current test thread.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap();
    }
    let launch = Launch {
        executable,
        _leases: Vec::new(),
    };
    let child = launch.start("--preview").unwrap();
    let thread = child.thread_id;
    let class = windows_core::w!("KeyrillConfigurePumpTest");
    let definition = WNDCLASSW {
        lpfnWndProc: Some(host_procedure),
        lpszClassName: class,
        ..Default::default()
    };
    // SAFETY: this test's static class and procedure survive the hidden host window.
    unsafe {
        RegisterClassW(&definition);
    }
    // SAFETY: the test owns this message-only host window on its STA thread.
    let host = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            windows_core::w!(""),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
        .unwrap()
    };
    PUMPED.set(false);
    let host_address = host.0 as usize;
    // SAFETY: the retained child's process handle is valid and the child owns a live UI loop.
    assert_eq!(
        unsafe { WaitForSingleObject(child.process.0, 0) },
        WAIT_TIMEOUT
    );
    let closer = std::thread::spawn(move || {
        let mut result = 0;
        // SAFETY: the parent retains its host HWND until this bounded cross-thread send completes.
        let sent = unsafe {
            SendMessageTimeoutW(
                HWND(host_address as *mut _),
                WM_APP + 19,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                5000,
                Some(&mut result),
            )
        };
        assert_ne!(
            sent.0, 0,
            "host pumps a synchronous native callback during the child wait"
        );
        assert_eq!(result, 417);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut found = false;
        while !found && std::time::Instant::now() < deadline {
            // SAFETY: enumerate only the created child's primary UI thread with writable local context.
            let _ = unsafe {
                EnumThreadWindows(
                    thread,
                    Some(close_preview),
                    LPARAM((&mut found as *mut bool) as isize),
                )
            };
            std::thread::yield_now();
        }
        assert!(found, "child preview window appeared");
    });
    // SAFETY: this native test owns its thread's existing quit request.
    unsafe {
        PostQuitMessage(812);
    }
    child.wait().unwrap();
    closer.join().unwrap();
    assert!(PUMPED.get());
    let mut message = MSG::default();
    // SAFETY: the modal wait must leave this host-owned request available to its original loop.
    assert!(unsafe { PeekMessageW(&mut message, None, WM_QUIT, WM_QUIT, PM_REMOVE) }.as_bool());
    assert_eq!(message.wParam.0, 812);
    assert!(
        launch
            .start("--invalid-controlled-option")
            .unwrap()
            .wait()
            .is_err()
    );
    // SAFETY: the child and cross-thread callback drained before host window destruction.
    unsafe {
        DestroyWindow(host).unwrap();
    }
    // SAFETY: pair this test's successful COM initialization after its child and interface work drains.
    unsafe {
        CoUninitialize();
    }
}
