//! Thread-owned maintenance window and its callback lifetime.

use crate::foreign::win32::{set_window_long, window_long};
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HINSTANCE, HMODULE, HWND, LPARAM,
    LRESULT, SetLastError, WPARAM,
};
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleHandleExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClassInfoW,
    HWND_MESSAGE, IsWindow, KillTimer, RegisterClassW, SetTimer, UnregisterClassW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_NCCREATE, WM_NCDESTROY, WM_TIMER, WNDCLASSW,
};
use windows_core::{PCWSTR, w};

const CLASS_NAME: PCWSTR = w!("KeyrillSnapshotMaintenance");
const TIMER: usize = 1;

struct Callbacks {
    current: Rc<dyn Fn() -> bool>,
    tick: Rc<dyn Fn()>,
    window: Cell<HWND>,
}

pub(crate) struct Window {
    callbacks: Option<Rc<Callbacks>>,
    module: Option<HINSTANCE>,
}

impl Window {
    pub(crate) fn start(current: Rc<dyn Fn() -> bool>, tick: Rc<dyn Fn()>) -> Option<Self> {
        fenced(|| {
            let mut window = Self {
                callbacks: Some(Rc::new(Callbacks {
                    current,
                    tick,
                    window: Cell::new(HWND::default()),
                })),
                module: None,
            };
            if !window.acquire() {
                window.close();
                return None;
            }
            Some(window)
        })
        .flatten()
    }

    fn acquire(&mut self) -> bool {
        let callbacks = self.callbacks.as_ref().expect("starting window").clone();
        if !(callbacks.current)() {
            return false;
        }
        let mut module = HMODULE::default();
        // SAFETY: FROM_ADDRESS interprets this procedure address as module code.
        let acquired = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                PCWSTR(procedure as *const () as *const u16),
                &mut module,
            )
        };
        if acquired.is_err() || !(callbacks.current)() {
            return false;
        }
        let module = HINSTANCE(module.0);
        let class = WNDCLASSW {
            hInstance: module,
            lpszClassName: CLASS_NAME,
            lpfnWndProc: Some(procedure),
            ..Default::default()
        };
        // SAFETY: the class name and procedure remain valid for the module lifetime.
        if unsafe { RegisterClassW(&class) } == 0 {
            // SAFETY: GetLastError immediately follows the failed registration.
            if unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS {
                return false;
            }
            let mut existing = WNDCLASSW::default();
            // SAFETY: existing is a writable class descriptor, scoped to this module.
            if unsafe { GetClassInfoW(Some(module), CLASS_NAME, &mut existing) }.is_err()
                || existing.hInstance != module
                || existing.lpfnWndProc.map(|call| call as usize)
                    != Some(procedure as *const () as usize)
            {
                return false;
            }
        }
        self.module = Some(module);
        if !(callbacks.current)() {
            return false;
        }
        // SAFETY: callbacks stays alive across synchronous creation. WM_NCCREATE retains
        // a separate Rc before publishing its address in the native userdata slot.
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS_NAME,
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(module),
                Some(Rc::as_ptr(&callbacks).cast()),
            )
        };
        if !(callbacks.current)() {
            return false;
        }
        let Ok(window) = created else {
            return false;
        };
        if callbacks.window.get() != window {
            return false;
        }
        // SAFETY: the captured window belongs to this thread and callback state.
        let timer = unsafe { SetTimer(Some(window), TIMER, 100, None) };
        timer != 0 && (callbacks.current)() && callbacks.window.get() == window
    }

    pub(crate) fn close(&mut self) {
        let callbacks = self.callbacks.take();
        let module = self.module.take();
        let _ = fenced(move || {
            if let Some(callbacks) = &callbacks {
                let window = callbacks.window.replace(HWND::default());
                if !window.is_invalid() {
                    // SAFETY: this thread owns the window. Retain callbacks locally while
                    // detaching its native owner, before DestroyWindow can reenter.
                    unsafe {
                        detach(window);
                        let _ = KillTimer(Some(window), TIMER);
                        if DestroyWindow(window).is_err() && IsWindow(Some(window)).as_bool() {
                            // A surviving HWND can still dispatch our module's procedure.
                            crate::foreign::pin_current_module_for_process_lifetime();
                        }
                    }
                }
            }
            if let Some(module) = module {
                // SAFETY: this is the captured class/module pair. Another live instance
                // can keep the class registered until its own final retirement.
                unsafe {
                    let _ = UnregisterClassW(CLASS_NAME, Some(module));
                }
            }
            drop(callbacks);
        });
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.close();
    }
}

fn fenced<T>(call: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(value) => Some(value),
        Err(payload) => {
            // A user-provided panic payload can itself panic when dropped.
            std::mem::forget(payload);
            None
        }
    }
}

unsafe fn detach(window: HWND) {
    // SAFETY: this native slot contains exactly one Rc retained by WM_NCCREATE.
    let pointer = unsafe { set_window_long(window, GWLP_USERDATA, 0) } as *const Callbacks;
    if !pointer.is_null() {
        // SAFETY: clearing the slot transfers its one owner back to this scope.
        drop(unsafe { Rc::from_raw(pointer) });
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    fenced(|| {
        if message == WM_NCCREATE {
            // SAFETY: Windows supplies CREATESTRUCTW for WM_NCCREATE, and start supplies
            // a live Rc allocation as its creation parameter.
            unsafe {
                let create = &*(l.0 as *const CREATESTRUCTW);
                let pointer = create.lpCreateParams.cast::<Callbacks>();
                Rc::increment_strong_count(pointer);
                SetLastError(ERROR_SUCCESS);
                set_window_long(window, GWLP_USERDATA, pointer as isize);
                if GetLastError() != ERROR_SUCCESS {
                    drop(Rc::from_raw(pointer));
                    return LRESULT(0);
                }
                (*pointer).window.set(window);
            }
        }
        // SAFETY: the userdata owner is live until detach clears the slot, on this thread.
        let pointer = unsafe { window_long(window, GWLP_USERDATA) } as *const Callbacks;
        let callbacks = if pointer.is_null() {
            None
        } else {
            // SAFETY: the native Rc owner keeps this allocation live while it is cloned.
            unsafe {
                Rc::increment_strong_count(pointer);
                Some(Rc::from_raw(pointer))
            }
        };
        if message == WM_NCDESTROY {
            if let Some(callbacks) = &callbacks {
                callbacks.window.set(HWND::default());
            }
            // SAFETY: the local Rc retains state through detach and DefWindowProcW.
            unsafe { detach(window) };
        }
        if message == WM_TIMER {
            if let Some(callbacks) = callbacks {
                let current = fenced(|| (callbacks.current)()).unwrap_or(false);
                if current && callbacks.window.get() == window {
                    // Keep the state owner outside the callback's unwind scope, even
                    // when the callback closes its window before panicking.
                    let _ = fenced(|| (callbacks.tick)());
                }
            }
            return LRESULT(0);
        }
        // SAFETY: the original native message parameters are forwarded unchanged.
        unsafe { DefWindowProcW(window, message, w, l) }
    })
    .unwrap_or(LRESULT(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, HCBT_CREATEWND, HCBT_DESTROYWND, MSG,
        MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_TIMER,
        SendMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_CBT,
    };

    thread_local! {
        static CURRENT: Cell<bool> = const { Cell::new(true) };
        static STALE_ON_CREATE: Cell<bool> = const { Cell::new(false) };
        static REJECT_ON_CREATE: Cell<bool> = const { Cell::new(false) };
        static DESTROYED: Cell<usize> = const { Cell::new(0) };
    }

    unsafe extern "system" fn hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        fenced(|| {
            if code == HCBT_CREATEWND as i32 && REJECT_ON_CREATE.get() {
                return LRESULT(1);
            }
            if code == HCBT_CREATEWND as i32 && STALE_ON_CREATE.get() {
                CURRENT.set(false);
            }
            if code == HCBT_DESTROYWND as i32 {
                DESTROYED.set(DESTROYED.get() + 1);
                // SAFETY: CBT supplies a thread-owned HWND before its destruction.
                unsafe {
                    SendMessageW(
                        HWND(w.0 as *mut _),
                        WM_TIMER,
                        Some(WPARAM(TIMER)),
                        Some(LPARAM(0)),
                    )
                };
            }
            // SAFETY: forward the hook's original parameters on the owning thread.
            unsafe { CallNextHookEx(None, code, w, l) }
        })
        .unwrap_or(LRESULT(0))
    }

    fn module() -> HINSTANCE {
        let mut module = HMODULE::default();
        // SAFETY: FROM_ADDRESS identifies the module containing this procedure.
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                PCWSTR(procedure as *const () as *const u16),
                &mut module,
            )
            .expect("maintenance module");
        }
        HINSTANCE(module.0)
    }

    fn registered() -> bool {
        // SAFETY: the output is writable and the class name is static.
        unsafe { GetClassInfoW(Some(module()), CLASS_NAME, &mut WNDCLASSW::default()) }.is_ok()
    }

    fn send_timer(window: HWND) {
        // SAFETY: each probe uses its own thread-owned maintenance HWND.
        unsafe { SendMessageW(window, WM_TIMER, Some(WPARAM(TIMER)), Some(LPARAM(0))) };
    }

    fn dispatch_timer(window: HWND) {
        // SAFETY: wait only for this test thread's timer queue, with a bounded timeout.
        let ready =
            unsafe { MsgWaitForMultipleObjectsEx(None, 5000, QS_TIMER, MWMO_INPUTAVAILABLE) };
        assert_eq!(ready, windows::Win32::Foundation::WAIT_OBJECT_0);
        let mut message = MSG::default();
        // SAFETY: the output is writable and the filter names the captured test HWND.
        assert!(unsafe {
            PeekMessageW(&mut message, Some(window), WM_TIMER, WM_TIMER, PM_REMOVE).as_bool()
        });
        assert_eq!(message.wParam, WPARAM(TIMER));
        // SAFETY: PeekMessageW supplied this message to its thread-owned procedure.
        unsafe { DispatchMessageW(&message) };
    }

    unsafe extern "system" fn conflicting(
        window: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
    ) -> LRESULT {
        fenced(|| {
            // SAFETY: the original native message is forwarded to its default handler.
            unsafe { DefWindowProcW(window, message, w, l) }
        })
        .unwrap_or(LRESULT(0))
    }

    #[test]
    fn native_window_schedules() {
        assert!(!registered());
        assert!(Window::start(Rc::new(|| false), Rc::new(|| {})).is_none());
        assert!(Window::start(Rc::new(|| panic!("current callback")), Rc::new(|| {})).is_none());
        assert!(!registered());

        STALE_ON_CREATE.set(true);
        CURRENT.set(true);
        DESTROYED.set(0);
        // SAFETY: the hook belongs only to this test thread and is removed below.
        let installed = unsafe {
            SetWindowsHookExW(WH_CBT, Some(hook), None, GetCurrentThreadId())
                .expect("thread-local maintenance hook")
        };
        assert!(Window::start(Rc::new(|| CURRENT.get()), Rc::new(|| {})).is_none());
        assert!(!CURRENT.get());
        assert_eq!(DESTROYED.get(), 1);
        assert!(!registered());
        STALE_ON_CREATE.set(false);
        REJECT_ON_CREATE.set(true);
        assert!(Window::start(Rc::new(|| true), Rc::new(|| {})).is_none());
        assert!(!registered());
        REJECT_ON_CREATE.set(false);

        let holder = Rc::new(RefCell::new(None::<Window>));
        let callback_holder = Rc::downgrade(&holder);
        let ticks = Rc::new(Cell::new(0));
        let callback_ticks = ticks.clone();
        let window = Window::start(
            Rc::new(|| true),
            Rc::new(move || {
                callback_ticks.set(callback_ticks.get() + 1);
                let holder = callback_holder.upgrade().expect("callback owner");
                let mut retired = holder.borrow_mut().take().expect("published window");
                retired.close();
                retired.close();
                assert!(!registered());
            }),
        )
        .expect("maintenance creation");
        let callbacks = Rc::downgrade(window.callbacks.as_ref().expect("callback state"));
        let hwnd = window.callbacks.as_ref().unwrap().window.get();
        *holder.borrow_mut() = Some(window);
        dispatch_timer(hwnd);
        assert_eq!(ticks.get(), 1);
        assert!(holder.borrow().is_none());
        assert!(callbacks.upgrade().is_none());
        assert_eq!(DESTROYED.get(), 2);
        // SAFETY: this removes the exact hook installed on the current test thread.
        unsafe { UnhookWindowsHookEx(installed).expect("remove maintenance hook") };

        for stale_at in 2..=5 {
            let checks = Rc::new(Cell::new(0));
            assert!(
                Window::start(
                    Rc::new(move || {
                        checks.set(checks.get() + 1);
                        checks.get() != stale_at
                    }),
                    Rc::new(|| {}),
                )
                .is_none()
            );
            assert!(!registered());
        }

        let holder = Rc::new(RefCell::new(None::<Window>));
        let current_holder = Rc::downgrade(&holder);
        let retire = Rc::new(Cell::new(false));
        let current_retire = retire.clone();
        let ticks = Rc::new(Cell::new(0));
        let callback_ticks = ticks.clone();
        let window = Window::start(
            Rc::new(move || {
                if current_retire.get() {
                    let holder = current_holder.upgrade().expect("current callback owner");
                    let retired = holder.borrow_mut().take();
                    drop(retired);
                }
                true
            }),
            Rc::new(move || callback_ticks.set(callback_ticks.get() + 1)),
        )
        .expect("current callback reentry instance");
        let hwnd = window.callbacks.as_ref().unwrap().window.get();
        *holder.borrow_mut() = Some(window);
        retire.set(true);
        send_timer(hwnd);
        assert_eq!(ticks.get(), 0);
        assert!(holder.borrow().is_none());
        assert!(!registered());

        let active = Rc::new(Cell::new(true));
        let current = active.clone();
        let ticks = Rc::new(Cell::new(0));
        let callback_ticks = ticks.clone();
        let first = Window::start(
            Rc::new(move || current.get()),
            Rc::new(move || callback_ticks.set(callback_ticks.get() + 1)),
        )
        .expect("first shared-class instance");
        let second = Window::start(Rc::new(|| true), Rc::new(|| panic!("tick callback")))
            .expect("second shared-class instance");
        let first_hwnd = first.callbacks.as_ref().unwrap().window.get();
        let second_hwnd = second.callbacks.as_ref().unwrap().window.get();
        send_timer(first_hwnd);
        assert_eq!(ticks.get(), 1);
        active.set(false);
        send_timer(first_hwnd);
        assert_eq!(ticks.get(), 1);
        send_timer(second_hwnd);
        drop(first);
        assert!(registered());
        drop(second);
        assert!(!registered());

        let window =
            Window::start(Rc::new(|| true), Rc::new(|| {})).expect("externally destroyed instance");
        let callbacks = Rc::downgrade(window.callbacks.as_ref().unwrap());
        let hwnd = window.callbacks.as_ref().unwrap().window.get();
        // SAFETY: this is the captured window created on this test thread.
        unsafe { DestroyWindow(hwnd).expect("external destruction") };
        assert!(window.callbacks.as_ref().unwrap().window.get().is_invalid());
        drop(window);
        assert!(callbacks.upgrade().is_none());
        assert!(!registered());

        struct PanicDrop;
        impl Drop for PanicDrop {
            fn drop(&mut self) {
                panic!("callback capture destructor");
            }
        }
        let capture = PanicDrop;
        let window = Window::start(
            Rc::new(|| true),
            Rc::new(move || {
                let _ = &capture;
            }),
        )
        .expect("destructor panic instance");
        drop(window);
        assert!(!registered());

        let holder = Rc::new(RefCell::new(None::<Window>));
        let callback_holder = Rc::downgrade(&holder);
        let capture = PanicDrop;
        let window = Window::start(
            Rc::new(|| true),
            Rc::new(move || {
                let _ = &capture;
                let holder = callback_holder.upgrade().expect("panicking callback owner");
                let retired = holder.borrow_mut().take();
                drop(retired);
                panic!("callback after retirement");
            }),
        )
        .expect("retiring panic instance");
        let callbacks = Rc::downgrade(window.callbacks.as_ref().unwrap());
        let hwnd = window.callbacks.as_ref().unwrap().window.get();
        *holder.borrow_mut() = Some(window);
        send_timer(hwnd);
        assert!(holder.borrow().is_none());
        assert!(callbacks.upgrade().is_none());
        assert!(!registered());

        let class = WNDCLASSW {
            hInstance: module(),
            lpfnWndProc: Some(conflicting),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        // SAFETY: this test retains the static procedure until unregistering below.
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        assert!(Window::start(Rc::new(|| true), Rc::new(|| {})).is_none());
        assert!(registered());
        // SAFETY: this exact class was registered above and has no live windows.
        unsafe { UnregisterClassW(CLASS_NAME, Some(module())).expect("retire conflicting class") };
        assert!(!registered());
    }
}
