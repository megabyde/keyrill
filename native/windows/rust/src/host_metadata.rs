//! Read-only TSF metadata for hosts outside the editing admission contract.

use crate::com::OwnServiceLease;
use crate::foreign::document as native;
use crate::foreign::{self, Call, InterfaceError, Lease, Returned};
use crate::observation::{Focus, Metadata, Reason, Scope};
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use windows::Win32::Foundation::{E_FAIL, E_UNEXPECTED, HWND, RPC_E_WRONG_THREAD, S_FALSE, S_OK};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, IsWindowEnabled};
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::{
    ES_PASSWORD, ES_READONLY, GWL_STYLE, GetWindowLongPtrW,
};
use windows_core::{GUID, HRESULT, IUnknown, Interface, Result, implement};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Ready,
    Entered,
    Expired,
}

struct Inputs {
    context: Lease<ITfContext>,
    manager: Lease<ITfThreadMgr>,
    identity: Option<Lease<IUnknown>>,
}

struct Frame {
    inputs: RefCell<Option<Inputs>>,
    current: Rc<dyn Fn() -> bool>,
    entry: Cell<Entry>,
    value: Cell<Metadata>,
    panicked: Cell<bool>,
    // The actual generated service allocation outlives every captured host reference.
    _owner: OwnServiceLease,
}

impl Frame {
    fn update(&self, update: impl FnOnce(&mut Metadata)) {
        let mut value = self.value.get();
        update(&mut value);
        self.value.set(value);
    }

    fn reason(&self, reason: Reason) {
        self.update(|value| value.reason = reason);
    }

    fn alive(&self) -> bool {
        if self.value.get().reason == Reason::Exception {
            return false;
        }
        if self.entry.get() == Entry::Expired || !(self.current)() {
            self.reason(Reason::Stale);
            return false;
        }
        true
    }

    fn good(&self, call: Call) -> bool {
        if !self.alive() {
            return false;
        }
        if call.result != S_OK {
            self.update(|value| {
                value.reason = Reason::Native;
                value.result = call.result.0 as u32;
            });
            return false;
        }
        true
    }

    fn required<T: Interface>(
        &self,
        query: impl FnOnce() -> Returned<Option<Lease<T>>>,
    ) -> Option<Lease<T>> {
        if !self.alive() {
            return None;
        }
        let output = query();
        if !self.good(output.call) {
            drop(output.value);
            return None;
        }
        output.value
    }

    fn query<T: Interface, U: Interface>(&self, object: &Lease<T>) -> Option<Lease<U>> {
        if !self.alive() {
            return None;
        }
        let output = object.query();
        self.queried(output)
    }

    fn queried<T: Interface>(
        &self,
        output: std::result::Result<Lease<T>, InterfaceError>,
    ) -> Option<Lease<T>> {
        match output {
            Ok(lease) => self.alive().then_some(lease),
            Err(error) => {
                self.good(error.call());
                None
            }
        }
    }

    fn same<T: Interface>(&self, object: &Lease<T>, expected: usize) -> bool {
        let Some(identity) = self.query::<_, IUnknown>(object) else {
            return false;
        };
        let same = identity.address() == expected;
        drop(identity);
        if !self.alive() {
            return false;
        }
        if !same {
            self.reason(Reason::Context);
        }
        same
    }

    fn enabled<T: Interface>(&self, object: &Lease<T>, id: &GUID) -> bool {
        let Some(manager) = self.query::<_, ITfCompartmentMgr>(object) else {
            return false;
        };
        let Some(compartment) = self.required(|| native::compartment(&manager, id)) else {
            return false;
        };
        if !self.alive() {
            return false;
        }
        let output = native::compartment_value(&compartment);
        let mut call = output.call;
        if call.result == S_FALSE && output.value.empty() {
            call.result = S_OK;
        }
        let queried = self.good(call);
        let enabled = output.value.empty() || output.value.zero_integer();
        let cleared = self.good(output.value.clear());
        if !queried || !cleared {
            return false;
        }
        if !enabled {
            self.reason(Reason::Disabled);
        }
        enabled
    }

    fn window_policy(&self, window: HWND) -> bool {
        // SAFETY: these Win32 metadata queries accept borrowed HWND values and return scalars.
        let focus = unsafe { GetFocus() };
        if !self.alive() {
            return false;
        }
        self.update(|value| {
            value.focus = if window.is_invalid() {
                Focus::Windowless
            } else if window == focus {
                Focus::Exact
            } else {
                Focus::Different
            };
        });
        let mut styles = 0;
        let mut disabled = false;
        for target in [window, focus] {
            if target.is_invalid() {
                continue;
            }
            // SAFETY: GetWindowLongPtrW reads the borrowed window's metadata without text access.
            styles |= unsafe { GetWindowLongPtrW(target, GWL_STYLE) } as u32;
            if !self.alive() {
                return false;
            }
            // SAFETY: IsWindowEnabled queries only the borrowed window's enabled flag.
            disabled |= !unsafe { IsWindowEnabled(target) }.as_bool();
            if !self.alive() {
                return false;
            }
        }
        let reason = if styles & ES_PASSWORD as u32 != 0 {
            Some(Reason::Secure)
        } else if disabled {
            Some(Reason::Disabled)
        } else if styles & ES_READONLY as u32 != 0 {
            Some(Reason::ReadOnly)
        } else {
            None
        };
        if let Some(reason) = reason {
            self.reason(reason);
            return false;
        }
        true
    }

    fn observe(&self, cookie: u32) -> Option<()> {
        self.reason(Reason::Native);
        let (context, manager, identity) = {
            let inputs = self.inputs.borrow();
            let inputs = inputs.as_ref()?;
            (
                inputs.context.clone(),
                inputs.manager.clone(),
                inputs.identity.as_ref()?.address(),
            )
        };
        let document = self.required(|| native::focus(&manager))?;
        let top = self.required(|| native::top(&document))?;
        if !self.same(&top, identity) {
            return None;
        }
        let view = self.required(|| native::view(&context))?;
        if !self.alive() {
            return None;
        }
        let window = native::window(&view);
        if !self.good(window.call) || !self.window_policy(window.value) {
            return None;
        }
        let status = native::status(&context);
        if !self.good(status.call) {
            return None;
        }
        if status.value.dwDynamicFlags & TF_SD_READONLY != 0 {
            self.reason(Reason::ReadOnly);
            return None;
        }
        if !self.enabled(&context, &GUID_COMPARTMENT_KEYBOARD_DISABLED)
            || !self.enabled(&context, &GUID_COMPARTMENT_EMPTYCONTEXT)
            || !self.enabled(&manager, &GUID_COMPARTMENT_KEYBOARD_DISABLED)
        {
            return None;
        }
        if !self.alive() {
            return None;
        }
        let selected = native::selection(&context, cookie);
        if !self.good(selected.call) {
            return None;
        }
        let range = selected.value.range;
        if selected.value.fetched != 1
            || range.is_none()
            || selected.value.style.fInterimChar.as_bool()
        {
            self.reason(Reason::Selection);
            return None;
        }
        let range = range?;
        let range_context = self.required(|| native::range_context(&range))?;
        if !self.same(&range_context, identity) {
            return None;
        }
        self.update(|value| {
            value.selection = 1;
            value.scope = Scope::Missing;
            value.reason = Reason::MissingScope;
        });
        let property = self.required(|| native::property(&context))?;
        if !self.alive() {
            return None;
        }
        let output = native::property_value(&property, cookie, &range);
        let queried = self.good(output.call);
        let scope = if queried && output.value.has_unknown() {
            self.queried(output.value.input_scope())
        } else {
            None
        };
        let cleared = self.good(output.value.clear());
        if !cleared || !self.alive() {
            return None;
        }
        let Some(scope) = scope else {
            self.reason(Reason::MissingScope);
            return None;
        };
        let scopes = native::scopes(&scope);
        if !self.good(scopes.call) {
            return None;
        }
        let scope = match scopes.value.single() {
            Some(IS_DEFAULT) => Scope::Default,
            Some(IS_TEXT) => Scope::Text,
            Some(_) => Scope::Excluded,
            None => Scope::Ambiguous,
        };
        self.update(|value| value.scope = scope);
        if scope != Scope::Default && scope != Scope::Text {
            self.reason(Reason::Scope);
            return None;
        }
        self.reason(Reason::Unsupported);
        Some(())
    }

    fn panic(&self) {
        self.panicked.set(true);
        self.reason(Reason::Exception);
    }

    fn request(self: &Rc<Self>, client: u32) {
        self.reason(Reason::Native);
        if !self.alive() {
            return;
        }
        let context = {
            let inputs = self.inputs.borrow();
            let Some(inputs) = inputs.as_ref() else {
                return;
            };
            inputs.context.clone()
        };
        let Some(identity) = self.query::<_, IUnknown>(&context) else {
            return;
        };
        let mut identity = Some(identity);
        {
            let mut inputs = self.inputs.borrow_mut();
            if let Some(inputs) = inputs.as_mut() {
                std::mem::swap(&mut inputs.identity, &mut identity);
            }
        }
        drop(identity);
        // SAFETY: GetCurrentThreadId has no memory preconditions.
        let thread = unsafe { GetCurrentThreadId() };
        let session: ITfEditSession = MetadataSession {
            thread,
            frame: Rc::clone(self),
        }
        .into();
        let (request, callback) =
            foreign::request(&context, client, &session, TF_ES_SYNC.0 | TF_ES_READ.0);
        self.alive();
        self.entry.set(Entry::Expired);
        self.update(|value| {
            value.request = request.result.0 as u32;
            value.session = callback.0 as u32;
            if self.panicked.get() {
                value.reason = Reason::Exception;
            } else if request.result != S_OK {
                value.reason = Reason::Request;
            } else if callback != S_OK {
                value.reason = Reason::Session;
            } else if value.completed == 0 {
                value.reason = Reason::Absent;
            }
        });
    }

    fn run(&self, cookie: u32) -> HRESULT {
        if self.entry.get() != Entry::Ready {
            return E_FAIL;
        }
        self.entry.set(Entry::Entered);
        self.update(|value| value.entered = 1);
        let _ = self.observe(cookie);
        self.alive();
        self.update(|value| value.completed = 1);
        S_OK
    }
}

#[implement(ITfEditSession, Agile = false)]
struct MetadataSession {
    thread: u32,
    frame: Rc<Frame>,
}

impl ITfEditSession_Impl for MetadataSession_Impl {
    fn DoEditSession(&self, cookie: u32) -> Result<()> {
        // SAFETY: GetCurrentThreadId has no memory preconditions.
        if unsafe { GetCurrentThreadId() } != self.thread {
            return RPC_E_WRONG_THREAD.ok();
        }
        catch_unwind(AssertUnwindSafe(|| self.frame.run(cookie)))
            .unwrap_or_else(|_| {
                self.frame.panic();
                self.frame.update(|value| value.completed = 1);
                E_UNEXPECTED
            })
            .ok()
    }
}

pub(crate) struct Observed {
    pub(crate) metadata: Metadata,
    pub(crate) panicked: bool,
}

pub(crate) fn observe(
    owner: OwnServiceLease,
    context: Lease<ITfContext>,
    manager: Lease<ITfThreadMgr>,
    client: u32,
    current: Rc<dyn Fn() -> bool>,
) -> Observed {
    let frame = Rc::new(Frame {
        inputs: RefCell::new(Some(Inputs {
            context,
            manager,
            identity: None,
        })),
        current,
        entry: Cell::new(Entry::Ready),
        value: Cell::new(Metadata::default()),
        panicked: Cell::new(false),
        _owner: owner,
    });
    if catch_unwind(AssertUnwindSafe(|| frame.request(client))).is_err() {
        frame.panic();
    }
    frame.entry.set(Entry::Expired);
    let inputs = frame.inputs.borrow_mut().take();
    drop(inputs);
    if catch_unwind(AssertUnwindSafe(|| {
        if frame.value.get().reason != Reason::Exception && !(frame.current)() {
            frame.reason(Reason::Stale);
        }
    }))
    .is_err()
    {
        frame.panic();
    }
    Observed {
        metadata: frame.value.get(),
        panicked: frame.panicked.get(),
    }
}

#[cfg(test)]
#[path = "host_metadata_tests.rs"]
mod tests;
