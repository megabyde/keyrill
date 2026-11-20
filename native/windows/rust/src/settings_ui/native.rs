use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};
use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    NtCreateFile,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::Storage::Packaging::Appx::GetPackagesByPackageFamily;
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::Win32::System::WindowsProgramming::DRIVE_FIXED;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::HiDpi::*;
use windows_core::{PCWSTR, PWSTR, w};

const CLASS: PCWSTR = w!("KeyrillSettingsWindow");

pub(super) fn native_identity(
    kind: isize,
    text: &str,
) -> Result<ApplicationIdentity, SettingsError> {
    if kind == 0 {
        executable_identity(text)
    } else if kind == 1 {
        let identity = ApplicationIdentity::package_family(text)?;
        let text = wide(identity.as_str());
        let mut count = 0;
        let mut length = 0;
        // SAFETY: the terminated family and writable size outputs survive the bounded query.
        let result = unsafe {
            GetPackagesByPackageFamily(PCWSTR(text.as_ptr()), &mut count, None, &mut length, None)
        };
        if result != ERROR_INSUFFICIENT_BUFFER || count == 0 {
            return Err(SettingsError::Invalid);
        }
        Ok(identity)
    } else {
        Err(SettingsError::Invalid)
    }
}

fn executable_identity(text: &str) -> Result<ApplicationIdentity, SettingsError> {
    let path = LocalPath::new(text)?;
    let root = wide(&path.as_str()[..3]);
    // SAFETY: grammar validation guarantees a terminated ordinary drive root.
    if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } != DRIVE_FIXED {
        return Err(SettingsError::Invalid);
    }
    struct Operation {
        name: Vec<u16>,
        text: UNICODE_STRING,
        attributes: OBJECT_ATTRIBUTES,
        io: IO_STATUS_BLOCK,
        handle: HANDLE,
    }
    let mut operation = Box::new(Operation {
        name: format!(r"\??\{}", path.as_str()).encode_utf16().collect(),
        text: UNICODE_STRING::default(),
        attributes: OBJECT_ATTRIBUTES::default(),
        io: IO_STATUS_BLOCK::default(),
        handle: HANDLE::default(),
    });
    operation.text = UNICODE_STRING {
        Length: (operation.name.len() * 2) as u16,
        MaximumLength: (operation.name.len() * 2) as u16,
        Buffer: PWSTR(operation.name.as_mut_ptr()),
    };
    operation.attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        ObjectName: &operation.text,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..Default::default()
    };
    // SAFETY: metadata-only no-reparse open retains every parameter until synchronous completion.
    let mut result = unsafe {
        NtCreateFile(
            &mut operation.handle,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &operation.attributes,
            &mut operation.io,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    struct File(HANDLE);
    impl Drop for File {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                // SAFETY: this scope owns the returned file handle.
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }
    let file = File(operation.handle);
    if result == STATUS_PENDING {
        // SAFETY: retain the IOSB and name until the synchronous metadata open drains.
        if unsafe { WaitForSingleObject(file.0, INFINITE) } != WAIT_OBJECT_0 {
            // A failed completion wait supplies no lifetime boundary for native output storage.
            std::mem::forget(operation);
            std::mem::forget(file);
            return Err(SettingsError::Unavailable);
        }
        // SAFETY: native completion has initialized this IOSB status variant.
        result = unsafe { operation.io.Anonymous.Status };
    }
    if result.0 < 0 || file.0.is_invalid() {
        return Err(SettingsError::Invalid);
    }
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: exact information class and writable structure size are paired, on the retained file.
    unsafe {
        GetFileInformationByHandleEx(
            file.0,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    }
    .map_err(|_| SettingsError::Invalid)?;
    if tag.FileAttributes
        & (FILE_ATTRIBUTE_REPARSE_POINT.0
            | FILE_ATTRIBUTE_OFFLINE.0
            | FILE_ATTRIBUTE_RECALL_ON_OPEN.0
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0)
        != 0
        || tag.ReparseTag != 0
    {
        return Err(SettingsError::Invalid);
    }
    let mut final_path = [0u16; EDIT_LIMIT + 1];
    // SAFETY: this queries only the retained file's canonical native name into bounded storage.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.0,
            &mut final_path,
            GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0),
        )
    } as usize;
    if length == 0 || length >= final_path.len() {
        return Err(SettingsError::Invalid);
    }
    let final_path =
        String::from_utf16(&final_path[..length]).map_err(|_| SettingsError::Invalid)?;
    let path = final_path
        .strip_prefix(r"\\?\")
        .ok_or(SettingsError::Invalid)?;
    if !path.to_ascii_lowercase().ends_with(".exe") {
        return Err(SettingsError::Invalid);
    }
    ApplicationIdentity::executable(path)
}

pub(super) fn choose_file(owner: HWND, executable: bool) -> Option<String> {
    let mut buffer = [0u16; EDIT_LIMIT + 1];
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrFilter: if executable {
            w!("Applications (*.exe)\0*.exe\0\0")
        } else {
            w!("Text files (*.txt)\0*.txt\0All files\0*.*\0\0")
        },
        lpstrTitle: if executable {
            w!("Choose application executable")
        } else {
            w!("Choose dictionary file")
        },
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_DONTADDTORECENT,
        ..Default::default()
    };
    // SAFETY: all dialog pointers refer to local storage live through this synchronous call.
    if !unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
        return None;
    }
    String::from_utf16(&buffer[..buffer.iter().position(|unit| *unit == 0)?]).ok()
}

pub(super) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
pub(super) fn set_text(window: HWND, text: &str) {
    let text = wide(text);
    // SAFETY: text is terminated and valid through the synchronous message.
    unsafe {
        let _ = SetWindowTextW(window, PCWSTR(text.as_ptr()));
    }
}
pub(super) fn get_text(window: HWND) -> Result<String, SettingsError> {
    let mut text = [0u16; EDIT_LIMIT + 1];
    // SAFETY: the bounded writable buffer belongs to this call on the owning thread.
    let length = unsafe { GetWindowTextW(window, &mut text) };
    if length < 0 || length as usize >= EDIT_LIMIT {
        return Err(SettingsError::Invalid);
    }
    String::from_utf16(&text[..length as usize]).map_err(|_| SettingsError::Invalid)
}
pub(super) fn send(window: HWND, message: u32, w: usize, l: isize) -> isize {
    // SAFETY: callers supply the native message's required scalar or locally retained pointer.
    unsafe { SendMessageW(window, message, Some(WPARAM(w)), Some(LPARAM(l))).0 }
}
pub(super) fn enable(window: HWND, enabled: bool) {
    // SAFETY: HWND is an owned child and the call holds no RefCell borrow.
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(window, enabled);
    }
}
pub(super) fn checked(window: HWND, value: bool) {
    send(window, BM_SETCHECK, usize::from(value), 0);
}
pub(super) fn is_checked(window: HWND) -> bool {
    send(window, BM_GETCHECK, 0, 0) == 1
}
pub(super) fn combo_add(window: HWND, text: &str) {
    let text = wide(text);
    send(window, CB_ADDSTRING, 0, text.as_ptr() as isize);
}

// windows-rs projects the pointer aliases to LONG on x86.
unsafe fn userdata(window: HWND) -> isize {
    #[cfg(target_pointer_width = "64")]
    {
        unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) }
    }
    #[cfg(target_pointer_width = "32")]
    {
        unsafe { GetWindowLongW(window, GWLP_USERDATA) as isize }
    }
}
unsafe fn set_userdata(window: HWND, value: isize) -> isize {
    #[cfg(target_pointer_width = "64")]
    {
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, value) }
    }
    #[cfg(target_pointer_width = "32")]
    {
        unsafe { SetWindowLongW(window, GWLP_USERDATA, value as i32) as isize }
    }
}

fn fenced<T>(call: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(value) => Some(value),
        Err(payload) => {
            std::mem::forget(payload);
            None
        }
    }
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let result = fenced(|| {
        if message == WM_NCCREATE {
            // SAFETY: open passes a live State allocation, and Windows supplies CREATESTRUCTW.
            unsafe {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                let pointer = create.lpCreateParams.cast::<State>();
                Rc::increment_strong_count(pointer);
                SetLastError(ERROR_SUCCESS);
                set_userdata(window, pointer as isize);
                if GetLastError() != ERROR_SUCCESS {
                    drop(Rc::from_raw(pointer));
                    return LRESULT(0);
                }
                (*pointer).window.set(window);
            }
        }
        // SAFETY: userdata retains one Rc until WM_NCDESTROY on this thread.
        let pointer = unsafe { userdata(window) } as *const State;
        let state = if pointer.is_null() {
            None
        } else {
            // SAFETY: cloning the native owner retains state across reentrant native calls.
            unsafe {
                Rc::increment_strong_count(pointer);
                Some(Rc::from_raw(pointer))
            }
        };
        if let Some(state) = state {
            match message {
                WM_CREATE => {
                    state.busy.set(true);
                    let _busy = Busy(&state.busy);
                    if state.create_controls().is_err() {
                        state.failed.set(true);
                        return LRESULT(-1);
                    }
                }
                WM_COMMAND => state.command(
                    (wparam.0 & 0xffff) as u16,
                    ((wparam.0 >> 16) & 0xffff) as u16,
                ),
                WM_SIZE => state.layout(),
                WM_TIMER if wparam.0 == 1 => state.poll_readiness(),
                WM_VSCROLL => {
                    let mut info = SCROLLINFO {
                        cbSize: size_of::<SCROLLINFO>() as u32,
                        fMask: SIF_ALL,
                        ..Default::default()
                    };
                    // SAFETY: a writable descriptor receives the current native scrollbar state.
                    unsafe {
                        let _ = GetScrollInfo(window, SB_VERT, &mut info);
                    }
                    let target = match SCROLLBAR_COMMAND((wparam.0 & 0xffff) as i32) {
                        SB_LINEUP => info.nPos - 30,
                        SB_LINEDOWN => info.nPos + 30,
                        SB_PAGEUP => info.nPos - info.nPage as i32,
                        SB_PAGEDOWN => info.nPos + info.nPage as i32,
                        SB_THUMBTRACK | SB_THUMBPOSITION => info.nTrackPos,
                        SB_TOP => 0,
                        SB_BOTTOM => info.nMax,
                        _ => info.nPos,
                    };
                    state.scroll.set(target);
                    state.layout();
                    return LRESULT(0);
                }
                WM_MOUSEWHEEL => {
                    state.scroll_by(-((wparam.0 >> 16) as u16 as i16 as i32) / 4);
                    return LRESULT(0);
                }
                WM_GETMINMAXINFO => {
                    // SAFETY: Windows supplies writable sizing information for this message.
                    unsafe {
                        let info = &mut *(lparam.0 as *mut MINMAXINFO);
                        let dpi = GetDpiForWindow(window).max(96) as i32;
                        info.ptMinTrackSize = POINT {
                            x: 746 * dpi / 96,
                            y: 480 * dpi / 96,
                        };
                    }
                }
                WM_DPICHANGED => {
                    // SAFETY: Windows supplies the suggested window rectangle for this message.
                    unsafe {
                        let rect = *(lparam.0 as *const RECT);
                        let _ = SetWindowPos(
                            window,
                            None,
                            rect.left,
                            rect.top,
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                    state.layout();
                }
                WM_CLOSE if state.busy.get() => return LRESULT(0),
                WM_NCDESTROY => {
                    state.stop_readiness();
                    // SAFETY: retire this window's timer before releasing its state.
                    unsafe {
                        let _ = KillTimer(Some(window), 1);
                    }
                    state.window.set(HWND::default());
                    // SAFETY: clearing userdata transfers and drops exactly its one native Rc owner.
                    unsafe {
                        let owner = set_userdata(window, 0) as *const State;
                        if !owner.is_null() {
                            drop(Rc::from_raw(owner));
                        }
                    }
                    let font = state.font.replace(HFONT::default());
                    if !font.is_invalid() {
                        // SAFETY: child windows have been destroyed before WM_NCDESTROY.
                        unsafe {
                            let _ = DeleteObject(font.into());
                        }
                    }
                    // SAFETY: this standalone UI thread ends its loop after the window closes.
                    unsafe {
                        PostQuitMessage(i32::from(state.failed.get()));
                    }
                }
                _ => {}
            }
        }
        // SAFETY: forward original parameters to the standard window handler.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    });
    result.unwrap_or_else(|| {
        // SAFETY: this thread still owns the native userdata slot when a callback unwinds.
        let pointer = unsafe { userdata(window) } as *const State;
        if !pointer.is_null() {
            // SAFETY: the native Rc owner remains retained until WM_NCDESTROY.
            unsafe {
                (*pointer).failed.set(true);
            }
        }
        // SAFETY: post retirement outside the failed callback's stack.
        unsafe {
            let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        LRESULT(0)
    })
}

pub(super) fn open(state: &Rc<State>, visible: bool) -> windows_core::Result<HWND> {
    // SAFETY: the current executable module remains loaded through its message loop.
    let module = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSW {
        lpfnWndProc: Some(procedure),
        hInstance: HINSTANCE(module.0),
        lpszClassName: CLASS,
        hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as usize as *mut _),
        ..Default::default()
    };
    // SAFETY: the procedure and static class name remain valid for process lifetime.
    if unsafe { RegisterClassW(&class) } == 0 {
        // SAFETY: inspect the existing module-local class before sharing its native userdata protocol.
        let mut existing = WNDCLASSW::default();
        if unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
            || unsafe { GetClassInfoW(Some(HINSTANCE(module.0)), CLASS, &mut existing) }.is_err()
            || existing.lpfnWndProc.map(|procedure| procedure as usize)
                != Some(procedure as *const () as usize)
        {
            return Err(windows_core::Error::from_thread());
        }
    }
    // SAFETY: DPI is acquired before the initial window is created.
    let dpi = unsafe { GetDpiForSystem() }.max(96) as i32;
    // SAFETY: Rc state remains alive through creation, which retains a native owner in WM_NCCREATE.
    unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            CLASS,
            if state.source.preview() {
                w!("Keyrill Settings (Preview)")
            } else {
                w!("Keyrill Settings")
            },
            WS_OVERLAPPEDWINDOW
                | WS_VSCROLL
                | if visible {
                    WS_VISIBLE
                } else {
                    WINDOW_STYLE::default()
                },
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            770 * dpi / 96,
            820 * dpi / 96,
            Some(state.owner.get()),
            None,
            Some(HINSTANCE(module.0)),
            Some(Rc::as_ptr(state).cast()),
        )
    }
}

/// Runs the native settings application. `--preview` uses synthetic in-memory defaults.
/// Returns zero after an ordinary close and nonzero after startup or message-loop failure.
/// All persistence is performed on this standalone UI thread, outside input callbacks.
///
/// ```no_run
/// std::process::exit(keyrill_windows::settings_ui::run());
/// ```
pub fn run() -> i32 {
    fenced(|| {
        let arguments: Vec<_> = std::env::args_os().skip(1).collect();
        let mut owner = HWND::default();
        let source = match arguments.as_slice() {
            [] => Source::CurrentUser,
            [argument] if argument == "--preview" => {
                Source::Preview(RefCell::new(Snapshot::default()))
            }
            [flag, window, process, thread] if flag == "--owner" => {
                let parsed = (|| {
                    Some((
                        window.to_str()?.parse::<usize>().ok()?,
                        process.to_str()?.parse::<u32>().ok()?,
                        thread.to_str()?.parse::<u32>().ok()?,
                    ))
                })();
                let Some((window, process, thread)) = parsed else {
                    return 2;
                };
                let candidate = HWND(window as *mut _);
                let mut actual_process = 0;
                // SAFETY: owner identity is used only for native window ownership and never text access.
                let actual_thread =
                    unsafe { GetWindowThreadProcessId(candidate, Some(&mut actual_process)) };
                if actual_thread != 0 && actual_thread == thread && actual_process == process {
                    owner = candidate;
                }
                Source::CurrentUser
            }
            _ => return 2,
        };
        // SAFETY: this is the standalone executable's UI startup, before creating any window.
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let state = State::new(source);
        state.owner.set(owner);
        let Ok(window) = open(&state, true) else {
            return 1;
        };
        let mut message = MSG::default();
        loop {
            // SAFETY: a writable message receives messages only for this UI thread.
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
            if result <= 0 {
                let surviving = state.window.get();
                if !surviving.is_invalid() {
                    // SAFETY: retire the UI thread's remaining native owner before returning.
                    unsafe {
                        let _ = DestroyWindow(surviving);
                    }
                }
                return if result == 0 {
                    message.wParam.0 as i32
                } else {
                    1
                };
            }
            // SAFETY: standard dialog navigation handles Tab and access keys before dispatch.
            unsafe {
                if IsDialogMessageW(window, &message).as_bool() {
                    if !state.window.get().is_invalid() {
                        state.reveal_focus();
                    }
                } else {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    })
    .unwrap_or(1)
}
