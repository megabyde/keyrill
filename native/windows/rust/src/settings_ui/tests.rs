use super::*;
use windows::Win32::System::Threading::GetCurrentThreadId;

thread_local! {
    static REENTER_WINDOW: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    static REENTERED: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn reenter(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && !REENTER_WINDOW.get().is_invalid() {
        // SAFETY: WH_CALLWNDPROC supplies this borrowed native message for the hook call.
        let message = unsafe { &*(lparam.0 as *const CWPSTRUCT) };
        if message.message == WM_SETTEXT && !REENTERED.replace(true) {
            send(REENTER_WINDOW.get(), WM_COMMAND, SAVE as usize, 0);
            send(REENTER_WINDOW.get(), WM_CLOSE, 0, 0);
        }
    }
    // SAFETY: unchanged hook parameters continue on their owning thread.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

pub(super) struct Window(pub(super) Rc<State>);

#[test]
fn native_client_edges_follow_control_roles() {
    let window = Window::new(Source::Preview(RefCell::new(Snapshot::default())));
    for id in [EN_PATH, RU_PATH, IDENTITY, APPLICATIONS, AUTOMATIC, SAVE] {
        // SAFETY: the test owns these live native controls on the current thread.
        let style = unsafe { GetWindowLongW(window.0.control(id), GWL_EXSTYLE) } as u32;
        assert_eq!(
            style & WS_EX_CLIENTEDGE.0 != 0,
            matches!(id, EN_PATH | RU_PATH | IDENTITY | APPLICATIONS)
        );
    }
}

impl Window {
    pub(super) fn new(source: Source) -> Self {
        let state = State::new(source);
        open(&state, false).expect("hidden native settings window");
        Self(state)
    }

    pub(super) fn click(&self, id: u16) {
        send(self.0.control(id), BM_CLICK, 0, 0);
    }

    pub(super) fn status(&self) -> String {
        get_text(self.0.control(STATUS)).unwrap()
    }

    fn select_application(&self, identity: &ApplicationIdentity) {
        let row = self
            .0
            .draft
            .borrow()
            .applications
            .keys()
            .position(|key| key == identity)
            .unwrap();
        send(self.0.control(APPLICATIONS), LB_SETCURSEL, row, 0);
        send(
            self.0.window.get(),
            WM_COMMAND,
            APPLICATIONS as usize | ((LBN_SELCHANGE as usize) << 16),
            0,
        );
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let window = self.0.window.get();
        if !window.is_invalid() {
            // SAFETY: this test owns the hidden native window on this thread.
            unsafe {
                DestroyWindow(window).unwrap();
            }
        }
    }
}

#[test]
fn native_save_reload_and_close_use_complete_isolated_settings() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let window = Window::new(Source::Test(store.clone()));
    assert!(!is_checked(window.0.control(AUTOMATIC)));
    assert!(is_checked(window.0.control(SOURCE)));
    window.click(AUTOMATIC);
    window.click(SOURCE);
    window.click(EN_DEFAULT);
    set_text(window.0.control(EN_PATH), r"C:\PublicFixture\English.txt");
    window.click(SAVE);
    let saved = store.0.load().unwrap();
    assert!(saved.value.automatic);
    assert!(!saved.value.switch_source);
    assert_eq!(
        saved.value.dictionaries[0],
        DictionaryPath::Explicit(LocalPath::new(r"C:\PublicFixture\English.txt").unwrap())
    );
    assert!(window.status().starts_with("Saved."));
    window.click(AUTOMATIC);
    window.click(RELOAD);
    assert!(is_checked(window.0.control(AUTOMATIC)));
    window.click(AUTOMATIC);
    window.click(CLOSE);
    assert!(window.0.window.get().is_invalid());
    assert_eq!(store.0.load().unwrap(), saved);
}

#[test]
fn native_invalid_draft_preserves_every_persisted_field() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let window = Window::new(Source::Test(store.clone()));
    let before = store.0.load().unwrap();
    window.click(AUTOMATIC);
    window.click(RU_DEFAULT);
    set_text(window.0.control(RU_PATH), r"\\server\private\words.txt");
    window.click(SAVE);
    assert_eq!(store.0.load().unwrap(), before);
    assert!(window.status().contains("absolute local file paths"));
    assert!(is_checked(window.0.control(AUTOMATIC)));
}

#[test]
fn native_conflict_requires_explicit_reload_and_retains_visible_draft() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let first = Window::new(Source::Test(store.clone()));
    let second = Window::new(Source::Test(store.clone()));
    first.click(AUTOMATIC);
    first.click(SAVE);
    let saved = store.0.load().unwrap();
    second.click(SOURCE);
    second.click(SAVE);
    assert!(second.status().contains("another window"));
    assert!(!is_checked(second.0.control(SOURCE)));
    assert_eq!(store.0.load().unwrap(), saved);
    send(second.0.window.get(), WM_COMMAND, SAVE as usize, 0);
    assert_eq!(store.0.load().unwrap(), saved);
    second.click(RELOAD);
    assert!(is_checked(second.0.control(AUTOMATIC)));
    assert!(is_checked(second.0.control(SOURCE)));
    second.click(SOURCE);
    second.click(SAVE);
    assert!(!store.0.load().unwrap().value.switch_source);
}

#[test]
fn native_application_editor_resolves_executable_and_removes_selected_row() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let window = Window::new(Source::Test(store.clone()));
    let executable = std::env::current_exe().unwrap();
    set_text(window.0.control(IDENTITY), executable.to_str().unwrap());
    send(window.0.control(POLICY), CB_SETCURSEL, 2, 0);
    window.click(UPSERT);
    window.click(SAVE);
    let saved = store.0.load().unwrap();
    assert_eq!(saved.value.applications.len(), 1);
    assert_eq!(
        saved.value.applications.values().next(),
        Some(&AppPolicy::Off)
    );
    send(window.0.control(APPLICATIONS), LB_SETCURSEL, 0, 0);
    send(
        window.0.window.get(),
        WM_COMMAND,
        APPLICATIONS as usize | ((LBN_SELCHANGE as usize) << 16),
        0,
    );
    assert_eq!(
        get_text(window.0.control(IDENTITY)).unwrap(),
        saved.value.applications.keys().next().unwrap().as_str()
    );
    window.click(REMOVE);
    window.click(SAVE);
    assert!(store.0.load().unwrap().value.applications.is_empty());
}

#[test]
fn native_application_rename_replaces_the_selected_identity_and_saves_its_policy() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let original = ApplicationIdentity::executable(r"C:\PublicFixture\Original.exe").unwrap();
    let replacement =
        native_identity(0, std::env::current_exe().unwrap().to_str().unwrap()).unwrap();
    let opened = store.0.load().unwrap();
    let mut value = (*opened.value).clone();
    value.applications.insert(original.clone(), AppPolicy::Off);
    assert!(matches!(
        store.0.save(opened.revision, value),
        SaveOutcome::Saved(_)
    ));
    let window = Window::new(Source::Test(store.clone()));
    window.select_application(&original);
    set_text(window.0.control(IDENTITY), replacement.as_str());
    window.click(UPSERT);
    assert_eq!(
        window.0.draft.borrow().applications,
        BTreeMap::from([(replacement.clone(), AppPolicy::Off)])
    );
    window.click(SAVE);
    assert!(window.status().starts_with("Saved."));
    assert_eq!(
        store.0.load().unwrap().value.applications,
        BTreeMap::from([(replacement.clone(), AppPolicy::Off)])
    );
    window.select_application(&replacement);
    send(window.0.control(POLICY), CB_SETCURSEL, 1, 0);
    window.click(UPSERT);
    window.click(SAVE);
    assert_eq!(
        store.0.load().unwrap().value.applications,
        BTreeMap::from([(replacement, AppPolicy::Manual)])
    );
}

#[test]
fn native_application_rename_collision_retains_both_rows_and_blocks_save() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let original = ApplicationIdentity::executable(r"C:\PublicFixture\Original.exe").unwrap();
    let existing = native_identity(0, std::env::current_exe().unwrap().to_str().unwrap()).unwrap();
    let opened = store.0.load().unwrap();
    let mut value = (*opened.value).clone();
    value.applications = BTreeMap::from([
        (original.clone(), AppPolicy::Manual),
        (existing.clone(), AppPolicy::Off),
    ]);
    assert!(matches!(
        store.0.save(opened.revision, value.clone()),
        SaveOutcome::Saved(_)
    ));
    let before = store.0.load().unwrap();
    let window = Window::new(Source::Test(store.clone()));
    window.select_application(&original);
    set_text(window.0.control(IDENTITY), existing.as_str());
    send(window.0.control(POLICY), CB_SETCURSEL, 0, 0);
    window.click(UPSERT);
    assert_eq!(window.0.draft.borrow().applications, value.applications);
    assert_eq!(
        get_text(window.0.control(IDENTITY)).unwrap(),
        existing.as_str()
    );
    assert_eq!(send(window.0.control(POLICY), CB_GETCURSEL, 0, 0), 0);
    assert!(window.status().contains("already has an override"));
    window.click(SAVE);
    assert!(window.status().contains("Add / update before Save"));
    assert_eq!(store.0.load().unwrap(), before);
    send(window.0.control(POLICY), CB_SETCURSEL, 2, 0);
    window.click(UPSERT);
    assert_eq!(window.0.draft.borrow().applications, value.applications);
    window.click(AUTOMATIC);
    window.click(SAVE);
    assert!(window.status().contains("Add / update before Save"));
    assert_eq!(store.0.load().unwrap(), before);
    window.click(RELOAD);
    assert_eq!(window.0.draft.borrow().applications, value.applications);
    assert!(get_text(window.0.control(IDENTITY)).unwrap().is_empty());
    window.click(AUTOMATIC);
    window.click(SAVE);
    let saved = store.0.load().unwrap();
    assert!(saved.value.automatic);
    assert_eq!(saved.value.applications, value.applications);
}

#[test]
fn native_application_invalid_rename_preserves_the_selected_identity() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let original = ApplicationIdentity::executable(r"C:\PublicFixture\Original.exe").unwrap();
    let opened = store.0.load().unwrap();
    let mut value = (*opened.value).clone();
    value.applications.insert(original.clone(), AppPolicy::Off);
    assert!(matches!(
        store.0.save(opened.revision, value.clone()),
        SaveOutcome::Saved(_)
    ));
    let before = store.0.load().unwrap();
    let window = Window::new(Source::Test(store.clone()));
    window.select_application(&original);
    set_text(window.0.control(IDENTITY), r"\\server\missing.exe");
    window.click(UPSERT);
    assert_eq!(window.0.draft.borrow().applications, value.applications);
    assert_eq!(window.0.selected(), Some((original, AppPolicy::Off)));
    assert_eq!(
        get_text(window.0.control(IDENTITY)).unwrap(),
        r"\\server\missing.exe"
    );
    assert!(window.status().contains("invalid or unavailable"));
    window.click(SAVE);
    assert!(window.status().contains("Add / update before Save"));
    assert_eq!(store.0.load().unwrap(), before);
}

#[test]
fn native_new_application_keeps_the_previously_selected_override() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let original = ApplicationIdentity::executable(r"C:\PublicFixture\Original.exe").unwrap();
    let added = native_identity(0, std::env::current_exe().unwrap().to_str().unwrap()).unwrap();
    let opened = store.0.load().unwrap();
    let mut value = (*opened.value).clone();
    value.applications.insert(original.clone(), AppPolicy::Off);
    assert!(matches!(
        store.0.save(opened.revision, value.clone()),
        SaveOutcome::Saved(_)
    ));
    let window = Window::new(Source::Test(store.clone()));
    window.select_application(&original);
    window.click(NEW);
    assert_eq!(window.0.selected(), None);
    assert!(get_text(window.0.control(IDENTITY)).unwrap().is_empty());
    set_text(window.0.control(IDENTITY), added.as_str());
    window.click(UPSERT);
    value.applications.insert(added, AppPolicy::Manual);
    assert_eq!(window.0.draft.borrow().applications, value.applications);
    window.click(SAVE);
    assert_eq!(store.0.load().unwrap().value.as_ref(), &value);
}

#[test]
fn native_preview_is_named_and_native_ownership_retires() {
    let window = Window::new(Source::Preview(RefCell::new(Snapshot::default())));
    let weak = Rc::downgrade(&window.0);
    assert!(get_text(window.0.window.get()).unwrap().contains("Preview"));
    window.click(AUTOMATIC);
    window.click(SAVE);
    assert!(window.status().contains("User preferences were untouched"));
    window.click(RELOAD);
    assert!(is_checked(window.0.control(AUTOMATIC)));
    assert!(native_identity(1, "Example.DoesNotExist_123456789abcd").is_err());
    assert!(native_identity(0, r"\\server\missing.exe").is_err());
    drop(window);
    assert!(weak.upgrade().is_none());
}

#[test]
fn native_reentry_during_reload_cannot_save_partial_controls_or_close_the_window() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let window = Window::new(Source::Test(store.clone()));
    let before = store.0.load().unwrap();
    window.click(AUTOMATIC);
    REENTER_WINDOW.set(window.0.window.get());
    REENTERED.set(false);
    // SAFETY: this bounded hook observes only this test's UI thread and is retired below.
    let hook =
        unsafe { SetWindowsHookExW(WH_CALLWNDPROC, Some(reenter), None, GetCurrentThreadId()) }
            .unwrap();
    window.click(RELOAD);
    REENTER_WINDOW.set(HWND::default());
    // SAFETY: this scope owns the installed thread hook.
    unsafe {
        UnhookWindowsHookEx(hook).unwrap();
    }
    assert!(REENTERED.get());
    assert!(!window.0.window.get().is_invalid());
    assert_eq!(store.0.load().unwrap(), before);
    assert!(!is_checked(window.0.control(AUTOMATIC)));
}

#[test]
fn native_short_viewport_scrolls_and_reveals_keyboard_focus() {
    let window = Window::new(Source::Preview(RefCell::new(Snapshot::default())));
    // SAFETY: resize only the hidden window owned by this test thread.
    unsafe {
        SetWindowPos(
            window.0.window.get(),
            None,
            0,
            0,
            770,
            480,
            SWP_NOMOVE | SWP_NOZORDER,
        )
        .unwrap();
    }
    send(window.0.window.get(), WM_VSCROLL, SB_BOTTOM.0 as usize, 0);
    assert!(window.0.scroll.get() > 0);
    // SAFETY: focus is assigned only to controls in this test's hidden window.
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(
            window.0.control(AUTOMATIC),
        ));
    }
    window.0.reveal_focus();
    assert!(window.0.scroll.get() < 20);
    // SAFETY: focus remains within this test window.
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(window.0.control(SAVE)));
    }
    window.0.reveal_focus();
    assert!(window.0.scroll.get() > 0);
}

#[test]
fn native_reload_error_keeps_draft_visible_and_blocks_stale_save() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let window = Window::new(Source::Test(store.clone()));
    window.click(AUTOMATIC);
    store.invalidate();
    window.click(RELOAD);
    assert!(is_checked(window.0.control(AUTOMATIC)));
    assert!(window.status().contains("could not be loaded"));
    send(window.0.window.get(), WM_COMMAND, SAVE as usize, 0);
    assert_eq!(store.0.load(), Err(SettingsError::Invalid));
    assert!(window.status().contains("Reload current settings"));
}

#[test]
fn native_callback_panic_retires_window_without_unwinding_into_windows() {
    let window = Window::new(Source::Preview(RefCell::new(Snapshot::default())));
    let hwnd = window.0.window.get();
    let conflicting_borrow = window.0.draft.borrow_mut();
    send(hwnd, WM_COMMAND, SAVE as usize, 0);
    drop(conflicting_borrow);
    assert!(window.0.failed.get());
    let mut message = MSG::default();
    // SAFETY: inspect only the retirement message posted for this test's native window.
    assert!(
        unsafe { PeekMessageW(&mut message, Some(hwnd), WM_CLOSE, WM_CLOSE, PM_REMOVE) }.as_bool()
    );
    // SAFETY: dispatch the retrieved message on its owning thread.
    unsafe {
        DispatchMessageW(&message);
    }
    assert!(window.0.window.get().is_invalid());
}

#[test]
fn native_save_rejects_unapplied_application_editor_changes() {
    let store = Rc::new(crate::settings::testing::Isolated::new());
    let window = Window::new(Source::Test(store.clone()));
    let before = store.0.load().unwrap();
    set_text(window.0.control(IDENTITY), "unfinished application edit");
    window.click(AUTOMATIC);
    window.click(SAVE);
    assert!(window.status().contains("Add / update before Save"));
    assert_eq!(store.0.load().unwrap(), before);
    set_text(window.0.control(IDENTITY), "");
    window.click(SAVE);
    assert!(store.0.load().unwrap().value.automatic);
}
