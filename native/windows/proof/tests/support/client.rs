use super::{com_slots, dll::*, document::*, providers::*, schedule::HostCapsule};
use std::{ffi::c_void, rc::Rc};
use windows::Win32::{
    Foundation::*,
    System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
    UI::TextServices::*,
};
use windows_core::{HRESULT, IUnknown, Interface};
struct FixturePolicy;
impl FixturePolicy {
    fn set() -> Self {
        // Serial test constructors select the fixture explicitly and restore the absent override.
        unsafe {
            std::env::set_var("KEYRILL_WINDOWS_POLICY", "fixture");
        }
        Self
    }
}
impl Drop for FixturePolicy {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("KEYRILL_WINDOWS_POLICY");
        }
    }
}

pub struct Client {
    pub host: Rc<HostCapsule>,
    pub a: Rc<Document>,
    pub b: Rc<Document>,
    pub manager: Rc<Manager>,
    pub context_a: ITfContext,
    pub context_b: ITfContext,
    pub thread: ITfThreadMgr,
    pub profiles: IUnknown,
    pub profile_state: Rc<Profile>,
    pub service: ITfTextInputProcessorEx,
    pub handle: *mut c_void,
    pub api: Api,
    _policy: Option<FixturePolicy>,
}
impl Client {
    pub fn new() -> Self {
        Self::with_layout_callback(true)
    }
    pub fn without_layouts() -> Self {
        Self::with_layout_callback(false)
    }
    fn with_layout_callback(with_layout: bool) -> Self {
        let policy = (std::env::var_os("KEYRILL_WINDOWS_POLICY").is_none()
            && std::env::var_os("KEYRILL_WINDOWS_TEST_SETTINGS_NAMESPACE").is_none())
        .then(FixturePolicy::set);
        let api = Api::load();
        let host = Rc::new(HostCapsule::default());
        let a = Document::new(Rc::clone(&host), 101);
        let b = Document::new(Rc::clone(&host), 102);
        unsafe {
            let context_a = ITfContext::from_raw(Provider::create(
                Rc::clone(&host),
                Kind::Document(Rc::clone(&a)),
                ITfContext::IID,
            ));
            a.context.set(context_a.as_raw());
            let context_b = ITfContext::from_raw(Provider::create(
                Rc::clone(&host),
                Kind::Document(Rc::clone(&b)),
                ITfContext::IID,
            ));
            b.context.set(context_b.as_raw());
            let manager = Manager::new(Rc::clone(&host), context_a.as_raw());
            let thread = ITfThreadMgr::from_raw(Provider::create(
                Rc::clone(&host),
                Kind::Manager(Rc::clone(&manager)),
                ITfThreadMgr::IID,
            ));
            let profile_state = Rc::new(Profile::default());
            let profiles = IUnknown::from_raw(Provider::create(
                Rc::clone(&host),
                Kind::Profiles(Rc::clone(&profile_state)),
                IUnknown::IID,
            ));
            let dependencies = Dependencies {
                size: size_of::<Dependencies>() as u32,
                version: VERSION,
                context: Rc::as_ptr(&host).cast_mut().cast(),
                retain: Some(retain),
                release: Some(release),
                profiles: profiles.as_raw(),
                layouts: if with_layout {
                    Some(layout_callback(&host))
                } else {
                    None
                },
                capture: Some(capture),
            };
            let mut handle = std::ptr::null_mut();
            let mut service = std::ptr::null_mut();
            assert_eq!(
                (api.create)(VERSION, &dependencies, &mut handle, &mut service),
                S_OK
            );
            let service = ITfTextInputProcessorEx::from_raw(service);
            let hr = (service.vtable().ActivateEx)(service.as_raw(), thread.as_raw(), 42, 0);
            assert_eq!(hr, S_OK, "activate actual DLL");
            host.check();
            Self {
                host,
                a,
                b,
                manager,
                context_a,
                context_b,
                thread,
                profiles,
                profile_state,
                service,
                handle,
                api,
                _policy: policy,
            }
        }
    }
    pub fn captured_facts(&self) -> FieldFacts {
        let mut facts = FieldFacts::default();
        unsafe {
            assert_eq!(
                capture(
                    Rc::as_ptr(&self.host).cast_mut().cast(),
                    self.a.window,
                    &HostIdentity::default(),
                    &mut facts
                ),
                S_OK
            );
        }
        facts
    }
    pub fn ordinary(class: &str) -> Self {
        let c = Self::new();
        let mut facts = c.captured_facts();
        facts.proof_process = 0;
        facts.host.process = facts.process;
        facts.host.thread = facts.thread;
        facts.host.started = 1;
        facts.field.class_name.fill(0);
        for (to, from) in facts.field.class_name.iter_mut().zip(class.encode_utf16()) {
            *to = from;
        }
        facts.proof_root.class_name.fill(0);
        facts.proof_root.class_name[0] = b'R' as u16;
        c.host.field_facts.replace(Some(facts));
        c
    }
    pub fn notepad() -> Self {
        Self::notepad_version([11, 2607, 14, 0])
    }
    pub fn notepad_version(version: [u32; 4]) -> Self {
        let c = Self::new();
        unsafe {
            let process = GetCurrentProcessId();
            let thread = GetCurrentThreadId();
            let node = |handle: usize, class: &str| {
                let mut w = FieldWindow {
                    handle: HWND(handle as *mut _),
                    process,
                    thread,
                    enabled: 1,
                    complete: 1,
                    ..Default::default()
                };
                for (to, from) in w.class_name.iter_mut().zip(class.encode_utf16()) {
                    *to = from;
                }
                w
            };
            let mut field = node(101, "RichEditD2DPT");
            field.style = 0x40000004;
            c.host.field_facts.replace(Some(FieldFacts {
                host: HostIdentity {
                    process,
                    thread,
                    started: 1,
                    kind: 2,
                    version,
                },
                process,
                thread,
                view: c.a.window,
                focus: c.a.window,
                foreground: HWND(3usize as *mut _),
                root: HWND(3usize as *mut _),
                field,
                parent: node(2, "NotepadTextBox"),
                grandparent: node(3, "Notepad"),
                proof_root: node(3, "Notepad"),
                ..Default::default()
            }));
        }
        c
    }
    pub fn event(&self, event: Event) -> bool {
        let mut eaten = -1;
        unsafe {
            assert_eq!(
                (self.api.event)(self.handle, self.context_a.as_raw(), &event, &mut eaten),
                S_OK
            );
        }
        self.host.check();
        assert!(matches!(eaten, 0 | 1));
        eaten != 0
    }
    pub fn key(&self, unit: u16, pass: bool) -> bool {
        let eaten = self.event(Event::text(unit));
        if !eaten && pass {
            self.a.native_text(&[unit]);
        }
        eaten
    }
    pub fn type_text(&self, text: &str) {
        for unit in text.encode_utf16() {
            assert!(self.key(unit, true), "owned letter {unit}");
        }
    }
    pub fn delayed(&self) -> HRESULT {
        let pointer = self
            .a
            .retained
            .borrow()
            .as_ref()
            .expect("retained request")
            .as_raw();
        unsafe {
            let session = ITfEditSession::from_raw(com_slots::retain(pointer));
            (session.vtable().DoEditSession)(session.as_raw(), 123)
        }
    }
    pub fn deactivate(&self) -> HRESULT {
        unsafe { (self.service.vtable().base__.Deactivate)(self.service.as_raw()) }
    }
    pub fn reactivate(&self) -> HRESULT {
        unsafe {
            (self.service.vtable().ActivateEx)(self.service.as_raw(), self.thread.as_raw(), 42, 0)
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.host.drained();
        }
        let observer = self.host.observer.take();
        drop(observer);
        let actions = self.host.actions.take();
        drop(actions);
        let _ = self.deactivate();
        self.a.drain_retained();
        self.b.drain_retained();
        unsafe {
            assert_eq!((self.api.release)(self.handle), S_OK);
        }
        if !std::thread::panicking() {
            self.host.check();
        }
    }
}
pub unsafe extern "system" fn retain(context: *mut c_void) {
    let host = unsafe { &*context.cast::<HostCapsule>() };
    com_slots::fence(host, (), || {
        host.dependency_retains
            .set(host.dependency_retains.get() + 1);
        unsafe {
            Rc::increment_strong_count(context.cast::<HostCapsule>());
        }
    });
}
pub unsafe extern "system" fn release(context: *mut c_void) {
    let owner = unsafe { Rc::from_raw(context.cast::<HostCapsule>()) };
    com_slots::fence(&owner, (), || {
        owner
            .dependency_releases
            .set(owner.dependency_releases.get() + 1);
    });
}
thread_local! {static LAYOUT_HOSTS:std::cell::RefCell<Vec<std::rc::Weak<HostCapsule>>>=const{std::cell::RefCell::new(Vec::new())};}
fn layout_callback(host: &Rc<HostCapsule>) -> unsafe extern "system" fn(i32, *mut isize) -> i32 {
    let index = LAYOUT_HOSTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        if let Some(index) = slots.iter().position(|s| s.strong_count() == 0) {
            slots[index] = Rc::downgrade(host);
            index
        } else {
            let index = slots.len();
            assert!(index < 32, "at most32 simultaneous dependency capsules");
            slots.push(Rc::downgrade(host));
            index
        }
    });
    const CALLBACKS: [unsafe extern "system" fn(i32, *mut isize) -> i32; 32] = [
        layouts::<0>,
        layouts::<1>,
        layouts::<2>,
        layouts::<3>,
        layouts::<4>,
        layouts::<5>,
        layouts::<6>,
        layouts::<7>,
        layouts::<8>,
        layouts::<9>,
        layouts::<10>,
        layouts::<11>,
        layouts::<12>,
        layouts::<13>,
        layouts::<14>,
        layouts::<15>,
        layouts::<16>,
        layouts::<17>,
        layouts::<18>,
        layouts::<19>,
        layouts::<20>,
        layouts::<21>,
        layouts::<22>,
        layouts::<23>,
        layouts::<24>,
        layouts::<25>,
        layouts::<26>,
        layouts::<27>,
        layouts::<28>,
        layouts::<29>,
        layouts::<30>,
        layouts::<31>,
    ];
    CALLBACKS[index]
}
static LAYOUT_PANICS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub fn layout_panics() -> usize {
    LAYOUT_PANICS.load(std::sync::atomic::Ordering::Relaxed)
}
unsafe extern "system" fn layouts<const INDEX: usize>(capacity: i32, out: *mut isize) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        layout_call::<INDEX>(capacity, out)
    })) {
        Ok(result) => result,
        Err(_) => {
            LAYOUT_PANICS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            0
        }
    }
}
unsafe fn layout_call<const INDEX: usize>(capacity: i32, out: *mut isize) -> i32 {
    let host = LAYOUT_HOSTS
        .with(|slots| slots.borrow()[INDEX].upgrade())
        .expect("callback retains dependency capsule");
    com_slots::fence(&host, 0, || {
        host.layout_calls.set(host.layout_calls.get() + 1);
        host.hit("Layouts");
        let result = {
            let mut results = host.layout_results.borrow_mut();
            if results.is_empty() {
                host.layouts.borrow().len() as i32
            } else {
                results.remove(0)
            }
        };
        if capacity > 0 && !out.is_null() {
            for (index, value) in host
                .layouts
                .borrow()
                .iter()
                .take(capacity as usize)
                .enumerate()
            {
                unsafe {
                    out.add(index).write(*value);
                }
            }
        }
        result
    })
}
unsafe extern "system" fn capture(
    context: *mut c_void,
    view: HWND,
    host: *const HostIdentity,
    out: *mut FieldFacts,
) -> HRESULT {
    let owner = unsafe { &*context.cast::<HostCapsule>() };
    com_slots::fence(owner, E_UNEXPECTED, || unsafe {
        owner.hit("capture");
        let process = GetCurrentProcessId();
        let thread = GetCurrentThreadId();
        let window = |handle: HWND, class: &str, control: i32| {
            let mut value = FieldWindow {
                handle,
                process,
                thread,
                control,
                enabled: 1,
                complete: 1,
                ..Default::default()
            };
            for (to, from) in value.class_name.iter_mut().zip(class.encode_utf16()) {
                *to = from;
            }
            value
        };
        let root = HWND(std::ptr::dangling_mut());
        let facts = FieldFacts {
            host: *host,
            process,
            thread,
            view,
            focus: view,
            foreground: root,
            root,
            field: window(view, "RICHEDIT50W", view.0 as i32),
            proof_root: window(root, "KeyrillProofEditor", 0),
            proof_process: 1,
            ..Default::default()
        };
        let mut facts = owner
            .field_facts
            .borrow()
            .as_ref()
            .copied()
            .unwrap_or(facts);
        if facts.view != view {
            facts.focus = HWND::default();
        }
        out.write(facts);
        S_OK
    })
}
