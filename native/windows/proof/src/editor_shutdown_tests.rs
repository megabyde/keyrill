use super::*;
#[path = "../tests/support/mod.rs"]
mod support;
use support::{
    document::Document,
    providers::{Diagnostic, Kind, Manager, Provider},
    schedule::HostCapsule,
};

unsafe extern "system" fn scope(_window: HWND, _scope: InputScope) -> HRESULT {
    S_OK
}

#[test]
fn shutdown_window_close_direct_destroy_and_empty_capture() {
    let instance = unsafe { GetModuleHandleW(None).unwrap() };
    let class = w!("KeyrillControlledShutdownWindow");
    let definition = WNDCLASSW {
        hInstance: instance.into(),
        lpfnWndProc: Some(window_proc),
        lpszClassName: class,
        ..Default::default()
    };
    assert_ne!(unsafe { RegisterClassW(&definition) }, 0);
    for scenario in 0..3 {
        let host = Rc::new(HostCapsule::default());
        let document = Document::new(Rc::clone(&host), 101);
        // SAFETY: independent providers return owned, stable SDK interface slots.
        let native_context = unsafe {
            ITfContext::from_raw(Provider::create(
                Rc::clone(&host),
                Kind::Document(Rc::clone(&document)),
                ITfContext::IID,
            ))
        };
        let manager = Manager::new(Rc::clone(&host), native_context.as_raw());
        let thread = unsafe {
            ITfThreadMgr::from_raw(Provider::create(
                Rc::clone(&host),
                Kind::Manager(Rc::clone(&manager)),
                ITfThreadMgr::IID,
            ))
        };
        let diagnostic = Rc::new(Diagnostic::default());
        {
            let mut words = diagnostic.snapshot.borrow_mut();
            words[0..2].copy_from_slice(&[3, 1]);
            words[55..66].copy_from_slice(&[1, 1, 0, 1, 1, 1, 1, 1, 1, 0, 0]);
        }
        let provider = unsafe {
            windows_core::IUnknown::from_raw(Provider::create(
                Rc::clone(&host),
                Kind::Diagnostic(Rc::clone(&diagnostic)),
                ITfFunctionProvider::IID,
            ))
        };
        manager.diagnostic.replace(Some(provider));
        let probe = Rc::new(shutdown::Probe::default());
        let context = Rc::new(Context {
            manager: thread,
            shutdown: Some(Rc::clone(&probe)),
            source: RefCell::new(None),
            destroyed: Cell::new(false),
            panicked: Cell::new(false),
            scope,
        });
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!(""),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .unwrap()
        };
        let transferred = Rc::into_raw(Rc::clone(&context));
        unsafe {
            set_window_long(window, GWLP_USERDATA, transferred as isize);
        }
        probe.observe(&context.manager);
        let captured_live = Rc::new(Cell::new(false));
        let captured = Rc::clone(&captured_live);
        let weak = Rc::downgrade(&context);
        *host.observer.borrow_mut() = Some(Rc::new(move |name| {
            if name == "copy" {
                captured.set(
                    unsafe { IsWindow(Some(window)) }.as_bool()
                        && !weak.upgrade().unwrap().destroyed.get(),
                );
            }
        }));
        if scenario == 2 {
            diagnostic.snapshot.borrow_mut()[59..61].fill(0);
        }
        unsafe {
            if scenario == 1 {
                DestroyWindow(window).unwrap();
            } else {
                SendMessageW(window, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
            }
        }
        host.observer.borrow_mut().take();
        assert!(!unsafe { IsWindow(Some(window)) }.as_bool());
        assert!(context.destroyed.get());
        assert!(!context.panicked.get());
        assert_eq!(captured_live.get(), scenario != 1);
        {
            let mut words = diagnostic.snapshot.borrow_mut();
            words[1] = 0;
            words[55..76].copy_from_slice(&[
                3, 1, 0, 3, 0, 0, 1, 1, 1, 0, 0, 2, 4, 1, 0, 0, 3, 8, 0, 8, 0,
            ]);
        }
        assert_eq!(
            probe.finish(S_OK) == shutdown::Verdict::Verified,
            scenario == 0
        );
        let mut message = MSG::default();
        let mut quit = false;
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            if message.message == WM_QUIT {
                quit = true;
                break;
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        assert!(quit);
        assert_eq!(Rc::strong_count(&context), 1);
        assert_eq!(document.reads.get(), 0);
        assert_eq!(document.writes.get(), 0);
        assert_eq!(document.starts.get(), 0);
        drop(context);
        drop(probe);
        drop(manager);
        drop(native_context);
        drop(document);
        host.drained();
        assert_eq!(host.live_objects.get(), 0);
        assert_eq!(
            host.releases.get(),
            host.adds.get() + host.final_releases.get()
        );
    }
    unsafe {
        UnregisterClassW(class, Some(instance.into())).unwrap();
    }
}
