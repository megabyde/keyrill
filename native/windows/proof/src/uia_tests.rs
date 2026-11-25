use super::*;
use crate::native::wide;
use crate::public_fixture::{PREFIX, SUFFIX};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
use windows::core::{PCWSTR, w};

#[test]
fn hidden_owned_rich_edit_has_independent_uia_text_and_utf16_selection() {
    crate::uia_test_process::run(
        "uia::public_provider_tests::hidden_owned_rich_edit_has_independent_uia_text_and_utf16_selection",
        hidden_owned_rich_edit,
    );
}

fn hidden_owned_rich_edit() {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let _apartment = crate::editor::Apartment::initialize().unwrap();
        let module = unsafe { LoadLibraryW(w!("Msftedit.dll")) }.unwrap();
        let instance = unsafe { GetModuleHandleW(None) }.unwrap();
        let root = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Keyrill public hidden UIA fixture"),
                WS_OVERLAPPED,
                0,
                0,
                500,
                200,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .unwrap();
        let text = format!("{PREFIX}привет{SUFFIX}");
        let child = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("RICHEDIT50W"),
                PCWSTR(wide(&text).as_ptr()),
                WS_CHILD | WINDOW_STYLE(ES_MULTILINE as u32),
                0,
                0,
                480,
                180,
                Some(root),
                None,
                Some(instance.into()),
                None,
            )
        }
        .unwrap();
        let offset = format!("{PREFIX}привет").encode_utf16().count() as i32;
        let selection = windows::Win32::UI::Controls::RichEdit::CHARRANGE {
            cpMin: offset,
            cpMax: offset,
        };
        unsafe {
            SendMessageW(
                child,
                windows::Win32::UI::Controls::RichEdit::EM_EXSETSEL,
                None,
                Some(LPARAM(std::ptr::from_ref(&selection) as isize)),
            )
        };
        sender
            .send((child.0 as usize, unsafe { GetCurrentThreadId() }))
            .unwrap();
        let mut message = MSG::default();
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        unsafe {
            DestroyWindow(root).unwrap();
            windows::Win32::Foundation::FreeLibrary(module).unwrap();
        }
    });
    let (window, tid) = receiver.recv().unwrap();
    struct FixtureThread {
        thread: Option<std::thread::JoinHandle<()>>,
        tid: u32,
    }
    impl Drop for FixtureThread {
        fn drop(&mut self) {
            unsafe {
                PostThreadMessageW(self.tid, WM_QUIT, WPARAM(0), LPARAM(0)).unwrap();
            }
            self.thread.take().unwrap().join().unwrap();
        }
    }
    let _fixture = FixtureThread {
        thread: Some(thread),
        tid,
    };
    let _apartment = crate::uia_test_process::MtaApartment::initialize();
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.unwrap();
    let element: IUIAutomationElement = interface!(
        automation,
        ElementFromHandle,
        HWND(window as *mut std::ffi::c_void)
    )
    .unwrap();
    assert_eq!(
        value!(element, CurrentProcessId).unwrap(),
        unsafe { GetCurrentProcessId() } as i32
    );
    let first = runtime_id(&element).unwrap();
    assert_eq!(runtime_id(&element).unwrap(), first);
    let pattern: IUIAutomationTextPattern = interface!(
        element,
        GetCurrentPatternAs,
        UIA_TextPatternId,
        &IUIAutomationTextPattern::IID
    )
    .unwrap();
    let document: IUIAutomationTextRange = interface!(pattern, DocumentRange).unwrap();
    let text: BSTR = unsafe { document.GetText(128) }.unwrap();
    assert_eq!(
        String::from_utf16(&text).unwrap(),
        format!("{PREFIX}привет{SUFFIX}")
    );
    let selections: IUIAutomationTextRangeArray = interface!(pattern, GetSelection).unwrap();
    assert_eq!(value!(selections, Length).unwrap(), 1);
    let selection: IUIAutomationTextRange = interface!(selections, GetElement, 0).unwrap();
    let prefix: IUIAutomationTextRange = interface!(document, Clone).unwrap();
    unsafe {
        prefix.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            &selection,
            TextPatternRangeEndpoint_Start,
        )
    }
    .unwrap();
    let text = unsafe { prefix.GetText(128) }.unwrap();
    assert_eq!(
        text.to_vec(),
        format!("{PREFIX}привет").encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(unsafe { selection.GetText(1) }.unwrap().len(), 0);
    let suffix: IUIAutomationTextRange = interface!(document, Clone).unwrap();
    unsafe {
        suffix.MoveEndpointByRange(
            TextPatternRangeEndpoint_Start,
            &selection,
            TextPatternRangeEndpoint_End,
        )
    }
    .unwrap();
    let text = unsafe { suffix.GetText(128) }.unwrap();
    assert_eq!(String::from_utf16(&text).unwrap(), SUFFIX);
}
