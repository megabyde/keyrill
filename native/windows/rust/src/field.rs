use crate::field_contract::{Facts, HostIdentity, Window};
use crate::foreign::win32::{module_path, window_long};
use windows::Win32::Foundation::{ERROR_SUCCESS, GetLastError, HWND, SetLastError};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, IsWindowEnabled};
use windows::Win32::UI::WindowsAndMessaging::{
    ES_MULTILINE, ES_PASSWORD, ES_READONLY, GA_ROOT, GWL_STYLE, GetAncestor, GetDlgCtrlID,
    GetForegroundWindow, GetParent, GetWindowThreadProcessId, IsWindow, WS_CHILD, WS_DISABLED,
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) kind: u32,
    pub(crate) field: HWND,
    pub(crate) parent: HWND,
    pub(crate) root: HWND,
    pub(crate) process: u32,
    pub(crate) thread: u32,
    pub(crate) root_thread: u32,
}

const _: () = assert!(
    size_of::<Identity>()
        == if cfg!(target_pointer_width = "64") {
            48
        } else {
            28
        }
);

fn equal(text: &[u16], expected: &str) -> bool {
    text.iter()
        .copied()
        .take_while(|unit| *unit != 0)
        .eq(expected.encode_utf16())
}

fn equal_ascii_fold(text: &[u16], expected: &str) -> bool {
    let fold = |unit| {
        if (b'A' as u16..=b'Z' as u16).contains(&unit) {
            unit + 32
        } else {
            unit
        }
    };
    text.iter()
        .copied()
        .take_while(|unit| *unit != 0)
        .map(fold)
        .eq(expected.encode_utf16().map(fold))
}

fn describe(handle: HWND) -> Window {
    let mut result = Window {
        handle,
        ..Window::default()
    };
    // SAFETY: these read-only Win32 queries validate arbitrary handles without dereferencing Rust memory.
    unsafe {
        if handle.0.is_null() || !IsWindow(Some(handle)).as_bool() {
            return result;
        }
        result.thread = GetWindowThreadProcessId(handle, Some(&mut result.process));
        let count = crate::foreign::win32::class_name(handle, &mut result.class_name);
        SetLastError(ERROR_SUCCESS);
        result.style = window_long(handle, GWL_STYLE);
        let style_error = GetLastError();
        SetLastError(ERROR_SUCCESS);
        result.control = GetDlgCtrlID(handle);
        let control_error = GetLastError();
        result.enabled = IsWindowEnabled(handle).as_bool() as u32;
        result.complete = (result.thread != 0
            && count > 0
            && count < result.class_name.len() as i32 - 1
            && style_error == ERROR_SUCCESS
            && control_error == ERROR_SUCCESS) as u32;
    }
    result
}

pub(crate) fn capture(view: HWND, host: HostIdentity) -> Facts {
    let mut path = [0; 32768];
    // SAFETY: only native window/process metadata and a bounded module filename are acquired.
    unsafe {
        let parent = GetParent(view).unwrap_or_default();
        let count = module_path(None, &mut path) as usize;
        let proof_process = count != 0
            && count < path.len()
            && path[..count]
                .iter()
                .rposition(|unit| *unit == b'\\' as u16)
                .is_some_and(|separator| {
                    equal_ascii_fold(&path[separator + 1..count], "KeyrillProofEditor.exe")
                });
        let root = GetAncestor(view, GA_ROOT);
        Facts {
            host,
            process: GetCurrentProcessId(),
            thread: GetCurrentThreadId(),
            view,
            focus: GetFocus(),
            foreground: GetForegroundWindow(),
            root,
            field: describe(view),
            parent: describe(parent),
            grandparent: describe(GetParent(parent).unwrap_or_default()),
            proof_root: describe(root),
            proof_process: proof_process as u32,
        }
    }
}

pub(crate) fn known_notepad(host: &HostIdentity) -> bool {
    host.kind == 2 && matches!(host.version, [11, 2607, 14, 0] | [11, 2609, 11, 0])
}

fn node(window: &Window, process: u32, class: &str) -> bool {
    window.complete != 0
        && !window.handle.0.is_null()
        && window.process == process
        && window.thread != 0
        && window.enabled != 0
        && window.control == 0
        && window.style & WS_DISABLED.0 as isize == 0
        && equal(&window.class_name, class)
}

fn bounded(facts: &Facts) -> Option<Identity> {
    let field = &facts.field;
    if facts.proof_process != 0
        && equal_ascii_fold(&field.class_name, "RICHEDIT50W")
        && matches!(field.control, 101 | 102)
        && facts.proof_root.complete != 0
        && facts.proof_root.handle == facts.root
        && equal(&facts.proof_root.class_name, "KeyrillProofEditor")
    {
        return Some(Identity {
            kind: 0,
            field: field.handle,
            parent: HWND::default(),
            root: facts.root,
            process: facts.process,
            thread: facts.thread,
            root_thread: facts.proof_root.thread,
        });
    }
    let parent = &facts.parent;
    let root = &facts.grandparent;
    let child_multiline = WS_CHILD.0 as isize | ES_MULTILINE as isize;
    if !known_notepad(&facts.host)
        || facts.host.process != facts.process
        || facts.host.thread != facts.thread
        || !node(field, facts.process, "RichEditD2DPT")
        || field.style & child_multiline != child_multiline
        || !node(parent, facts.process, "NotepadTextBox")
        || parent.thread != facts.thread
        || !node(root, facts.process, "Notepad")
        || root.handle != facts.root
        || root.handle != facts.foreground
    {
        return None;
    }
    Some(Identity {
        kind: 1,
        field: field.handle,
        parent: parent.handle,
        root: root.handle,
        process: facts.process,
        thread: facts.thread,
        root_thread: root.thread,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Contract {
    ProofRichEdit,
    NotepadBounded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CandidateField {
    pub(crate) identity: Identity,
    pub(crate) contract: Contract,
    started: u64,
}

pub(crate) fn classify(facts: &Facts) -> Option<CandidateField> {
    let field = &facts.field;
    let native_edit = [
        "Edit",
        "RICHEDIT50W",
        "RichEdit20W",
        "RichEdit20A",
        "RichEditD2DPT",
    ]
    .iter()
    .any(|class| equal_ascii_fold(&field.class_name, class));
    if facts.process == 0
        || facts.thread == 0
        || facts.view.0.is_null()
        || facts.view != facts.focus
        || field.handle != facts.view
        || field.complete == 0
        || field.process != facts.process
        || field.thread != facts.thread
        || field.enabled == 0
        || field.style & WS_DISABLED.0 as isize != 0
        || (native_edit && field.style & (ES_PASSWORD as isize | ES_READONLY as isize) != 0)
    {
        return None;
    }
    let identity = bounded(facts)?;
    Some(CandidateField {
        contract: if identity.kind == 1 {
            Contract::NotepadBounded
        } else {
            Contract::ProofRichEdit
        },
        started: facts.host.started,
        identity,
    })
}

#[cfg(feature = "test-api")]
pub(crate) fn matches(facts: &Facts) -> Option<Identity> {
    classify(facts).map(|candidate| candidate.identity)
}
