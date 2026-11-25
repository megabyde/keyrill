use super::*;
use windows::Win32::System::Threading::GetCurrentThreadId;

#[derive(Clone, Copy, Default)]
enum Failure {
    #[default]
    None,
    Error,
    Panic,
}
#[derive(Clone, Copy, Debug)]
struct ScopeCall {
    window: HWND,
    root: HWND,
    id: i32,
    scope: InputScope,
    alive: bool,
    owning_thread: bool,
}
#[derive(Default)]
struct Calls {
    assignments: Vec<ScopeCall>,
    removals: Vec<ScopeCall>,
    removing: bool,
    fail_assignment: Option<(usize, Failure)>,
    fail_removal: [Failure; 4],
}
thread_local! {
    static CALLS: RefCell<Calls> = RefCell::new(Calls::default());
}

fn observe(window: HWND, scope: InputScope, setter: ScopeSetter) -> HRESULT {
    let call = ScopeCall {
        window,
        root: unsafe { GetParent(window) }.unwrap(),
        id: unsafe { GetDlgCtrlID(window) },
        scope,
        alive: unsafe { IsWindow(Some(window)) }.as_bool(),
        owning_thread: unsafe { GetWindowThreadProcessId(window, None) == GetCurrentThreadId() },
    };
    let failure = CALLS.with_borrow_mut(|calls| {
        if call.id == 101 && scope == IS_DEFAULT {
            calls.removing = true;
        }
        if calls.removing {
            let index = calls.removals.len();
            calls.removals.push(call);
            calls.fail_removal.get(index).copied().unwrap_or_default()
        } else {
            calls.assignments.push(call);
            calls
                .fail_assignment
                .filter(|(position, _)| *position == calls.assignments.len())
                .map(|(_, failure)| failure)
                .unwrap_or_default()
        }
    });
    match failure {
        Failure::None => unsafe { setter(window, scope) },
        Failure::Error => E_ACCESSDENIED,
        Failure::Panic => panic!("controlled fixture scope failure"),
    }
}

struct Observation;
impl Observation {
    fn start(calls: Calls) -> Self {
        CALLS.set(calls);
        SCOPE_RESET_RESULT.set(S_OK);
        SCOPE_HOOK.set(Some(observe));
        Self
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        SCOPE_HOOK.set(None);
    }
}

fn assert_scope_lifecycle(children: usize, expected: HRESULT) {
    assert_eq!(SCOPE_RESET_RESULT.get(), expected);
    CALLS.with_borrow(|calls| {
        assert_eq!(calls.assignments.len(), children);
        assert_eq!(calls.removals.len(), children);
        let scopes = [IS_TEXT, IS_TEXT, IS_DEFAULT, IS_PASSWORD];
        let root = calls.assignments[0].root;
        assert!(!unsafe { IsWindow(Some(root)) }.as_bool());
        for (index, (assigned, removed)) in
            calls.assignments.iter().zip(&calls.removals).enumerate()
        {
            assert_eq!(assigned.id, 101 + index as i32);
            assert_eq!(assigned.scope, scopes[index]);
            assert!(assigned.alive && assigned.owning_thread);
            assert_eq!(assigned.root, root);
            assert_eq!(removed.root, root);
            assert_eq!(removed.id, assigned.id);
            assert_eq!(removed.window, assigned.window);
            assert_eq!(removed.scope, IS_DEFAULT);
            assert!(removed.alive && removed.owning_thread);
            assert!(!unsafe { IsWindow(Some(removed.window)) }.as_bool());
        }
    });
}

fn manager() -> ITfThreadMgr {
    unsafe { CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER) }.unwrap()
}

unsafe extern "system" fn foreign_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[test]
fn fixture_lifecycle_exact_scopes_and_live_owning_thread_cleanup() {
    let _apartment = Apartment::initialize().unwrap();
    let _observation = Observation::start(Calls::default());
    let fixture = create_fixture(manager(), None).unwrap();
    for (id, style) in [(103, ES_READONLY), (104, ES_PASSWORD)] {
        let field = unsafe { GetDlgItem(Some(fixture.window), id) }.unwrap();
        assert_ne!(unsafe { GetWindowLongW(field, GWL_STYLE) } & style, 0);
    }
    let context = fixture.context.clone();
    drop(fixture);
    assert!(context.destroyed.get());
    assert!(!context.panicked.get());
    assert_scope_lifecycle(4, S_OK);
}

#[test]
fn fixture_lifecycle_setup_failure_and_panic_at_each_of_four_assignments() {
    let _apartment = Apartment::initialize().unwrap();
    for failure in [Failure::Error, Failure::Panic] {
        for position in 1..=4 {
            let _observation = Observation::start(Calls {
                fail_assignment: Some((position, failure)),
                ..Default::default()
            });
            let result = create_fixture(manager(), None);
            let error = result.err().expect("no partial fixture may escape");
            let expected = match failure {
                Failure::Error => E_ACCESSDENIED,
                Failure::Panic => E_UNEXPECTED,
                Failure::None => unreachable!(),
            };
            assert_eq!(error.code(), expected);
            assert_scope_lifecycle(position, S_OK);
        }
    }
}

#[test]
fn fixture_lifecycle_reset_failure_and_panic_continue_all_children_and_keep_first_error() {
    let _apartment = Apartment::initialize().unwrap();
    for (fail_removal, expected) in [
        (
            [Failure::Error, Failure::Panic, Failure::None, Failure::None],
            E_ACCESSDENIED,
        ),
        (
            [Failure::Panic, Failure::Error, Failure::None, Failure::None],
            E_UNEXPECTED,
        ),
    ] {
        let _observation = Observation::start(Calls {
            fail_removal,
            ..Default::default()
        });
        let fixture = create_fixture(manager(), None).unwrap();
        let context = fixture.context.clone();
        drop(fixture);
        assert!(context.destroyed.get());
        assert!(!context.panicked.get());
        assert_scope_lifecycle(4, expected);
    }
}

#[test]
fn fixture_lifecycle_class_reuse_refuses_different_window_procedure_before_scopes() {
    let _apartment = Apartment::initialize().unwrap();
    let _observation = Observation::start(Calls::default());
    let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }.unwrap().0);
    register_fixture_class(instance, Some(window_proc)).unwrap();
    let result = register_fixture_class(instance, Some(foreign_window_proc));
    assert_eq!(result.unwrap_err().code(), E_FAIL);
    CALLS.with_borrow(|calls| {
        assert!(calls.assignments.is_empty());
        assert!(calls.removals.is_empty());
    });
}
