//! RichEdit proof editor with the native TSF keystroke and message-pump entry points.
#[cfg(test)]
#[path = "editor_fixture_tests.rs"]
mod fixture_tests;
#[cfg(test)]
#[path = "editor_shutdown_tests.rs"]
mod shutdown_tests;
use crate::diagnostics::{Poller, format_runtime, read_runtime};
use crate::native::{set_window_long, wide, window_long};
use crate::options::FixtureMode;
use crate::{routing, shutdown, source};
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;
use std::rc::Rc;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Controls::EM_EMPTYUNDOBUFFER;
use windows::Win32::UI::Controls::RichEdit::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HRESULT, Interface, PCSTR, PCWSTR, Result, w};

/// Native class of the proof-editor root window.
pub const FIXTURE_CLASS: PCWSTR = w!("KeyrillProofEditor");
/// Exact native title of the TSF lock-denial mode.
pub const DENIAL_TITLE: &str = "Keyrill TSF native lock-denial fixture";
/// Status row published after the native lock-denial style is verified.
pub const DENIAL_STATUS: &str = "Keyrill native denial v1; setup=Confirmed";
type ScopeSetter = unsafe extern "system" fn(HWND, InputScope) -> HRESULT;
#[cfg(test)]
type ScopeHook = fn(HWND, InputScope, ScopeSetter) -> HRESULT;
#[cfg(test)]
thread_local! {
    static SCOPE_HOOK: Cell<Option<ScopeHook>> = const { Cell::new(None) };
    static SCOPE_RESET_RESULT: Cell<HRESULT> = const { Cell::new(S_OK) };
}
fn set_scope(window: HWND, scope: InputScope, setter: ScopeSetter) -> HRESULT {
    #[cfg(test)]
    if let Some(hook) = SCOPE_HOOK.get() {
        return catch_unwind(AssertUnwindSafe(|| hook(window, scope, setter)))
            .unwrap_or(E_UNEXPECTED);
    }
    unsafe { setter(window, scope) }
}
struct Module(HMODULE);
impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}
/// An initialized single-threaded COM apartment, uninitialized on drop.
pub struct Apartment;
impl Apartment {
    /// Initialize a single-threaded native COM apartment.
    ///
    /// # Errors
    /// Returns the first failed native operation or evidence predicate.
    pub fn initialize() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
struct Activated(ITfThreadMgr, Cell<bool>);
impl Activated {
    fn deactivate(&self) -> HRESULT {
        if !self.1.replace(false) {
            return S_OK;
        }
        unsafe { (self.0.vtable().Deactivate)(self.0.as_raw()) }
    }
}
impl Drop for Activated {
    fn drop(&mut self) {
        let _ = self.deactivate();
    }
}
struct Context {
    manager: ITfThreadMgr,
    shutdown: Option<Rc<shutdown::Probe>>,
    source: RefCell<Option<Rc<source::Probe>>>,
    destroyed: Cell<bool>,
    panicked: Cell<bool>,
    scope: ScopeSetter,
}
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let result = catch_unwind(AssertUnwindSafe(|| {
        window_message(window, message, wparam, lparam)
    }));
    match result {
        Ok(value) => value,
        Err(_) => {
            let pointer = unsafe { window_long(window, GWLP_USERDATA) } as *const Context;
            if !pointer.is_null() {
                unsafe { &*pointer }.panicked.set(true);
            }
            unsafe {
                PostQuitMessage(1);
            }
            LRESULT(0)
        }
    }
}
fn window_message(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let pointer = unsafe { window_long(window, GWLP_USERDATA) } as *const Context;
    let context = if pointer.is_null() {
        None
    } else {
        unsafe {
            Rc::increment_strong_count(pointer);
            Some(Rc::from_raw(pointer))
        }
    };
    if let Some(context) = &context {
        if message == WM_CLOSE || message == WM_DESTROY {
            let source = context.source.borrow().clone();
            if let Some(source) = source {
                source.close();
            }
        }
        if message == WM_CLOSE
            && let Some(probe) = &context.shutdown
        {
            probe.capture_close(&context.manager);
        }
        if message == WM_DESTROY {
            let _ = reset_scopes(window, context.scope);
            context.destroyed.set(true);
        }
    }
    if message == WM_DESTROY {
        unsafe {
            PostQuitMessage(0);
        }
        return LRESULT(0);
    }
    if message == WM_NCDESTROY && !pointer.is_null() {
        unsafe {
            set_window_long(window, GWLP_USERDATA, 0);
            drop(Rc::from_raw(pointer));
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
fn reset_scopes(root: HWND, setter: ScopeSetter) -> HRESULT {
    let mut first = S_OK;
    for id in [101, 102, 103, 104] {
        if let Ok(child) = unsafe { GetDlgItem(Some(root), id) } {
            let result = set_scope(child, IS_DEFAULT, setter);
            if first == S_OK && result != S_OK {
                first = result;
            }
        }
    }
    #[cfg(test)]
    SCOPE_RESET_RESULT.set(first);
    first
}
struct Fixture {
    window: HWND,
    _rich_edit: Module,
    _tsf: Module,
    context: Rc<Context>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if unsafe { IsWindow(Some(self.window)) }.as_bool() {
            unsafe {
                let _ = DestroyWindow(self.window);
            }
        }
    }
}
fn row(window: HWND, id: i32, value: &str, y: i32, height: i32) -> Result<HWND> {
    let text = wide(value);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE,
            16,
            y,
            990,
            height,
            Some(window),
            Some(HMENU(id as usize as *mut _)),
            None,
            None,
        )
    }
}
fn set_text(window: HWND, value: &str) -> Result<()> {
    let text = wide(value);
    unsafe { SetWindowTextW(window, PCWSTR(text.as_ptr())) }
}
fn register_fixture_class(instance: HINSTANCE, procedure: WNDPROC) -> Result<()> {
    let class = WNDCLASSW {
        hInstance: instance,
        lpfnWndProc: procedure,
        lpszClassName: FIXTURE_CLASS,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let mut existing = WNDCLASSW::default();
        if unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
            || unsafe { GetClassInfoW(Some(instance), FIXTURE_CLASS, &mut existing) }.is_err()
            || existing.lpfnWndProc.map(|function| function as usize)
                != procedure.map(|function| function as usize)
        {
            return Err(windows::core::Error::from_hresult(E_FAIL));
        }
    }
    Ok(())
}
fn create_fixture(manager: ITfThreadMgr, shutdown: Option<Rc<shutdown::Probe>>) -> Result<Fixture> {
    let rich_edit = Module(unsafe { LoadLibraryW(w!("msftedit.dll"))? });
    let tsf = Module(unsafe { LoadLibraryW(w!("msctf.dll"))? });
    let raw = unsafe { GetProcAddress(tsf.0, PCSTR(c"SetInputScope".as_ptr().cast())) }
        .ok_or_else(|| windows::core::Error::from_hresult(E_NOINTERFACE))?;
    let scope: ScopeSetter = unsafe { std::mem::transmute(raw) };
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None)? }.0);
    register_fixture_class(instance, Some(window_proc))?;
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            FIXTURE_CLASS,
            w!("Keyrill TSF proof editor"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            780,
            620,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    let context = Rc::new(Context {
        manager,
        shutdown,
        source: RefCell::new(None),
        destroyed: Cell::new(false),
        panicked: Cell::new(false),
        scope,
    });
    let transferred = Rc::into_raw(context.clone());
    unsafe {
        SetLastError(ERROR_SUCCESS);
    }
    let previous = unsafe { set_window_long(window, GWLP_USERDATA, transferred as isize) };
    if previous == 0 && unsafe { GetLastError() } != ERROR_SUCCESS {
        unsafe {
            drop(Rc::from_raw(transferred));
            let _ = DestroyWindow(window);
        }
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    let fixture = Fixture {
        window,
        _rich_edit: rich_edit,
        _tsf: tsf,
        context,
    };
    let base = WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOVSCROLL as u32);
    let multiline = WINDOW_STYLE((ES_MULTILINE | ES_WANTRETURN) as u32);
    for (index, (id, label, style, scope_value)) in [
        (
            101,
            "A. Writable ordinary RichEdit",
            base | multiline,
            IS_TEXT,
        ),
        (
            102,
            "B. Writable ordinary RichEdit",
            base | multiline,
            IS_TEXT,
        ),
        (
            103,
            "Excluded. Read-only",
            base | multiline | WINDOW_STYLE(ES_READONLY as u32),
            IS_DEFAULT,
        ),
        (
            104,
            "Excluded. Password",
            base | WINDOW_STYLE(ES_PASSWORD as u32),
            IS_PASSWORD,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        row(window, 0, label, 16 + 132 * index as i32, 24)?;
        let edit = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                MSFTEDIT_CLASS,
                w!(""),
                style,
                16,
                42 + 132 * index as i32,
                720,
                90,
                Some(window),
                Some(HMENU(id as usize as *mut _)),
                Some(instance),
                None,
            )?
        };
        unsafe {
            SendMessageW(
                edit,
                EM_SETEDITSTYLE,
                Some(WPARAM(SES_USECTF as usize)),
                Some(LPARAM(SES_USECTF as isize)),
            );
        }
        let result = set_scope(edit, scope_value, scope);
        if result != S_OK {
            return Err(windows::core::Error::from_hresult(result));
        }
        unsafe {
            SendMessageW(edit, EM_EMPTYUNDOBUFFER, None, None);
        }
    }
    Ok(fixture)
}
/// Run the proof fixture on this thread until its native message loop ends.
///
/// # Errors
/// Returns the first failed native operation or evidence predicate.
pub fn run_editor(mode: FixtureMode) -> Result<ExitCode> {
    let _apartment = Apartment::initialize()?;
    let manager: ITfThreadMgr =
        unsafe { CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)? };
    let _client = unsafe { manager.Activate()? };
    let activated = Activated(manager.clone(), Cell::new(true));
    let keys: ITfKeystrokeMgr = manager.cast()?;
    let pump: ITfMessagePump = manager.cast()?;
    let shutdown =
        (mode == FixtureMode::ProveOwnedShutdown).then(|| Rc::new(shutdown::Probe::default()));
    let fixture = create_fixture(manager.clone(), shutdown.clone())?;
    let window = fixture.window;
    unsafe {
        SetWindowPos(
            window,
            None,
            0,
            0,
            1040,
            1140,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )?;
    }
    let source = if mode == FixtureMode::ProveOwnedEnToRu {
        Some(Rc::new(source::Probe::new(
            window,
            manager.clone(),
            unsafe { GetCurrentThreadId() },
        )))
    } else {
        None
    };
    fixture.context.source.replace(source.clone());
    if source.is_some() {
        set_text(window, source::TITLE)?;
        row(window, source::MODE_ID, source::MODE_STATUS, 552, 24)?;
        row(
            window,
            source::RESULT_ID,
            &source::Status::default()
                .format()
                .ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?,
            580,
            96,
        )?;
    }
    if mode == FixtureMode::DenyTsfLocks {
        let field = unsafe { GetDlgItem(Some(window), 101)? };
        let implemented = unsafe {
            SendMessageW(
                field,
                EM_SETEDITSTYLE,
                Some(WPARAM(SES_CTFNOLOCK as usize)),
                Some(LPARAM(SES_CTFNOLOCK as isize)),
            )
        };
        let observed = unsafe { SendMessageW(field, EM_GETEDITSTYLE, None, None) };
        if implemented != observed || observed.0 & SES_CTFNOLOCK as isize == 0 {
            return Err(windows::core::Error::from_hresult(E_FAIL));
        }
        set_text(window, DENIAL_TITLE)?;
        row(window, 107, DENIAL_STATUS, 552, 24)?;
    }
    if shutdown.is_some() {
        set_text(window, shutdown::TITLE)?;
        row(window, shutdown::MODE_ID, shutdown::MODE_STATUS, 552, 24)?;
    }
    let mut routing = routing::Snapshot::default();
    let service = Poller::default();
    let mode_row = mode != FixtureMode::Normal;
    let status = row(
        window,
        105,
        &routing.format(),
        if source.is_some() {
            680
        } else if mode_row {
            580
        } else {
            552
        },
        if mode_row { 96 } else { 120 },
    )?;
    let service_status = row(
        window,
        106,
        &service
            .value()
            .format()
            .ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?,
        if source.is_some() { 780 } else { 680 },
        if source.is_some() { 220 } else { 320 },
    )?;
    let runtime_status = row(window, 111, "Keyrill runtime unavailable", 1010, 70)?;
    if unsafe { SetTimer(Some(window), 1, 250, None) } == 0 {
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetFocus(Some(GetDlgItem(Some(window), 101)?));
    }
    let mut message = MSG::default();
    let mut available = BOOL(0);
    let pumped = loop {
        let result = unsafe {
            (pump.vtable().GetMessageW)(
                pump.as_raw(),
                &mut message,
                HWND::default(),
                0,
                0,
                &mut available,
            )
        };
        if result.is_err() || !available.as_bool() {
            break result;
        }
        if message.hwnd == window && message.message == WM_TIMER && message.wParam.0 == 1 {
            if let Some(source) = &source
                && let Some(value) = source.tick(unsafe { GetTickCount64() }).format()
            {
                let _ = set_text(
                    unsafe { GetDlgItem(Some(window), source::RESULT_ID)? },
                    &value,
                );
            }
            routing.publication = routing.publication.saturating_add(1);
            let _ = set_text(status, &routing.format());
            service.poll(&manager);
            if let Some(probe) = &shutdown {
                probe.observe(&manager);
            }
            let _ = set_text(
                service_status,
                &service
                    .value()
                    .format()
                    .unwrap_or_else(|| "Keyrill service unavailable".into()),
            );
            let runtime = read_runtime(&manager)
                .map(|value| format_runtime(&value))
                .unwrap_or_else(|_| "Keyrill runtime unavailable".into());
            let _ = set_text(runtime_status, &runtime);
            continue;
        }
        if [WM_KEYDOWN, WM_SYSKEYDOWN].contains(&message.message) {
            let route = routing::route_down(
                |interest| {
                    let mut output = BOOL(0);
                    let result = unsafe {
                        (keys.vtable().TestKeyDown)(
                            keys.as_raw(),
                            message.wParam,
                            message.lParam,
                            &mut output,
                        )
                    };
                    *interest = output.as_bool();
                    result.0
                },
                |eaten| {
                    let mut output = BOOL(0);
                    let result = unsafe {
                        (keys.vtable().KeyDown)(
                            keys.as_raw(),
                            message.wParam,
                            message.lParam,
                            &mut output,
                        )
                    };
                    *eaten = output.as_bool();
                    result.0
                },
            );
            routing.record(route);
            if route.consumed() {
                continue;
            }
        }
        if [WM_KEYUP, WM_SYSKEYUP].contains(&message.message) {
            let mut interest = BOOL(0);
            let mut eaten = BOOL(0);
            if unsafe {
                (keys.vtable().TestKeyUp)(
                    keys.as_raw(),
                    message.wParam,
                    message.lParam,
                    &mut interest,
                )
            }
            .is_ok()
                && interest.as_bool()
                && unsafe {
                    (keys.vtable().KeyUp)(keys.as_raw(), message.wParam, message.lParam, &mut eaten)
                }
                .is_ok()
                && eaten.as_bool()
            {
                continue;
            }
        }
        if message.message == WM_KEYDOWN && message.wParam.0 == VK_TAB.0 as usize {
            if let Ok(next) = unsafe {
                GetNextDlgTabItem(window, Some(GetFocus()), GetKeyState(VK_SHIFT.0 as i32) < 0)
            } {
                unsafe {
                    let _ = SetFocus(Some(next));
                }
            }
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    };
    unsafe {
        let _ = KillTimer(Some(window), 1);
    }
    if (shutdown.is_some() || source.is_some())
        && !fixture.context.destroyed.get()
        && unsafe { IsWindow(Some(window)) }.as_bool()
    {
        unsafe {
            let _ = DestroyWindow(window);
        }
    }
    if let Some(source) = &source {
        source.close();
    }
    fixture.context.source.replace(None);
    drop(source);
    drop(service);
    let deactivation = activated.deactivate();
    let verdict = if fixture.context.panicked.get() {
        1
    } else if let Some(shutdown) = shutdown {
        let verdict = shutdown.finish(deactivation) as u8;
        if pumped == S_OK
            && !available.as_bool()
            && message.message == WM_QUIT
            && fixture.context.destroyed.get()
        {
            verdict
        } else {
            1
        }
    } else {
        0
    };
    Ok(ExitCode::from(verdict))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;

    fn check_fixture_from_mta(window: HWND) {
        let window = window.0 as usize;
        let client = std::thread::spawn(move || {
            let _apartment = crate::uia_test_process::MtaApartment::initialize();
            let window = HWND(window as *mut std::ffi::c_void);
            assert!(crate::proof_inspect::check_owned_fixture(window, "привет "));
            assert!(!crate::proof_inspect::check_owned_fixture(
                window, "ghbdtn "
            ));
        });
        let handle = HANDLE(client.as_raw_handle());
        let mut quit = None;
        loop {
            // SAFETY: The retained JoinHandle owns the wait handle until the client is joined.
            let wait = unsafe {
                MsgWaitForMultipleObjectsEx(
                    Some(&[handle]),
                    windows::Win32::System::Threading::INFINITE,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                )
            };
            if wait == WAIT_OBJECT_0 {
                break;
            }
            if wait.0 != WAIT_OBJECT_0.0 + 1 {
                eprintln!("UIA owner message wait failed: {}", wait.0);
                std::process::abort();
            }
            let mut message = MSG::default();
            // SAFETY: This STA owns the queue and message storage throughout each call.
            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                if message.message == WM_QUIT {
                    quit = Some(message.wParam.0 as i32);
                } else {
                    // SAFETY: The creating STA dispatches its queue while the fixture is alive.
                    unsafe {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            }
        }
        if let Some(code) = quit {
            // SAFETY: The owner STA posts only to its own still-live thread queue.
            unsafe { PostQuitMessage(code) };
        }
        client.join().unwrap();
    }

    #[test]
    fn actual_rich_edit_fixture_preserves_ids_exclusion_styles_and_tsf_setup() {
        crate::uia_test_process::run(
            "editor::tests::actual_rich_edit_fixture_preserves_ids_exclusion_styles_and_tsf_setup",
            actual_rich_edit_fixture,
        );
    }

    fn actual_rich_edit_fixture() {
        let _apartment = Apartment::initialize().unwrap();
        let manager: ITfThreadMgr =
            unsafe { CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER) }.unwrap();
        unsafe { manager.Activate() }.unwrap();
        let activated = Activated(manager.clone(), Cell::new(true));
        let fixture = create_fixture(manager, None).unwrap();
        for id in [101, 102, 103, 104] {
            let field = unsafe { GetDlgItem(Some(fixture.window), id) }.unwrap();
            let facts = crate::uia::describe(field);
            assert!(facts.complete && facts.enabled);
            assert_eq!(facts.control, id);
            assert_eq!(String::from_utf16_lossy(&facts.class_name), "RICHEDIT50W");
            assert_eq!(facts.style & ES_READONLY as isize != 0, id == 103);
            assert_eq!(facts.style & ES_PASSWORD as isize != 0, id == 104);
            let style = unsafe { SendMessageW(field, EM_GETEDITSTYLE, None, None) }.0;
            assert_ne!(style & SES_USECTF as isize, 0);
        }
        let field = unsafe { GetDlgItem(Some(fixture.window), 101) }.unwrap();
        set_text(field, "привет ").unwrap();
        unsafe {
            SendMessageW(
                field,
                windows::Win32::UI::Controls::EM_SETSEL,
                Some(WPARAM(7)),
                Some(LPARAM(7)),
            );
        }
        check_fixture_from_mta(fixture.window);
        assert_eq!(reset_scopes(fixture.window, fixture.context.scope), S_OK);
        drop(fixture);
        assert_eq!(activated.deactivate(), S_OK);
    }
}
