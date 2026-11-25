use crate::diagnostics::{self, LifecycleJournal, RuntimeSnapshot, Snapshot, SnapshotV2};
use crate::diagnostics::{Cause, Counter, LifecycleCause, LifecycleKind, Stage, Unresolved};
use crate::engine::Engine;
use crate::engine::Plan;
use crate::field_contract::HostIdentity;
use crate::foreign::tsf;
use crate::foreign::{self, Call, DependencyLease, Lease};
use crate::keys::BINDINGS;
use crate::profiles::{PROFILES, SERVICE_ID};
use crate::vocabulary;
use crate::vocabulary_contract::Config;
use crate::vocabulary_contract::{Error as RuntimeError, ModuleOwner, Ready};
use keyrill_ffi::NativeEvent;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::rc::Weak;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Foundation::{E_ACCESSDENIED, E_INVALIDARG};
use windows::Win32::Foundation::{E_FAIL, RPC_E_WRONG_THREAD, S_OK};
use windows::Win32::System::Ole::CONNECT_E_NOCONNECTION;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::TextServices::*;
use windows_core::{HRESULT, IUnknown, Interface};

mod source;
pub(crate) use source::Operation as SourceTicket;

pub(crate) static OBJECTS: AtomicU32 = AtomicU32::new(0);
pub(crate) static LOCKS: AtomicU32 = AtomicU32::new(0);
pub(crate) static WORKERS: AtomicU32 = AtomicU32::new(0);

pub(crate) struct ObjectLease;

impl ObjectLease {
    pub(crate) fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for ObjectLease {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Activation {
    #[default]
    Inactive,
    Preparing,
    Cancelling,
    Active,
    Draining,
    Faulted,
}

#[derive(Default)]
struct State {
    generation: u64,
    epoch: u64,
    activation: Activation,
    activation_open: bool,
    resources: Option<Rc<Resources>>,
    diagnostic_source: Option<Lease<ITfSourceSingle>>,
    engine: Option<Engine>,
    accepted_config: Option<Config>,
    mode: u32,
    stopped: bool,
    diagnostic_depth: u32,
    diagnostics: Snapshot,
    lifecycle: LifecycleJournal,
    runtime: RuntimeSnapshot,
    owned: Option<Rc<Owned>>,
    busy: bool,
    admitting_key: bool,
    host_identity: HostIdentity,
    observations: diagnostics::ObservationJournal,
    publisher: Option<Rc<crate::observation::Publisher>>,
    metadata: crate::observation::Metadata,
    observing_host: bool,
    observed_generation: u64,
    alt_cycle: Option<Rc<AltCycle>>,
    release_permission: Option<Rc<AltCycle>>,
    release_decision: Option<Rc<Cell<bool>>>,
    worker: Option<vocabulary::Worker>,
    maintenance_window: Option<crate::runtime_window::Window>,
    pending_ready: Option<Ready>,
    applied_ready: Option<Ready>,
    application_off: bool,
    retirement_attempt: Option<(u32, crate::settings::Revision)>,
    source: source::SourceState,
    delivery: Option<Rc<Cell<bool>>>,
}

impl State {
    fn pending_restricts(&self) -> bool {
        self.pending_ready
            .as_ref()
            .zip(self.applied_ready.as_ref())
            .is_some_and(|(next, current)| next.capability() < current.capability())
    }
}

pub(crate) struct OwnedIdentity;

#[derive(Clone)]
pub(crate) struct Owned {
    pub(crate) identity: Rc<OwnedIdentity>,
    pub(crate) _context: Lease<ITfContext>,
    pub(crate) context_id: Lease<IUnknown>,
    pub(crate) composition: Lease<ITfComposition>,
    pub(crate) composition_id: Lease<IUnknown>,
    pub(crate) start: i32,
    pub(crate) verified: Vec<u16>,
    pub(crate) serial: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct Guard {
    epoch: u64,
    generation: u64,
}

pub(crate) struct Authority {
    pub(crate) manager: Lease<ITfThreadMgr>,
    pub(crate) client: u32,
    pub(crate) guard: Guard,
    pub(crate) owned: Option<Rc<Owned>>,
    pub(crate) host_identity: HostIdentity,
}

#[derive(Default)]
pub(crate) struct Execution {
    pub(crate) eaten: bool,
    completion: crate::edit::Completion,
    source: Option<Rc<source::Operation>>,
}

pub(crate) struct Sinks {
    pub(crate) thread: *mut c_void,
    pub(crate) profile: *mut c_void,
    pub(crate) keys: *mut c_void,
    pub(crate) provider: *mut c_void,
}

struct Resources {
    profiles: Option<Lease<ITfInputProcessorProfileMgr>>,
    keys: Option<Lease<ITfKeystrokeMgr>>,
    source: Option<Lease<ITfSource>>,
    manager: Option<Lease<ITfThreadMgr>>,
    owner: Weak<Service>,
    epoch: u64,
    client: u32,
    thread_cookie: Cell<u32>,
    profile_cookie: Cell<u32>,
    key_sink: Cell<bool>,
    bindings: Cell<usize>,
    uncertain: Cell<bool>,
    cleanup_error: Cell<HRESULT>,
}

impl Resources {
    fn new(owner: &Rc<Service>, epoch: u64, client: u32) -> Self {
        Self {
            profiles: None,
            keys: None,
            source: None,
            manager: None,
            owner: Rc::downgrade(owner),
            epoch,
            client,
            thread_cookie: Cell::new(TF_INVALID_COOKIE),
            profile_cookie: Cell::new(TF_INVALID_COOKIE),
            key_sink: Cell::new(false),
            bindings: Cell::new(0),
            uncertain: Cell::new(false),
            cleanup_error: Cell::new(S_OK),
        }
    }

    fn record_cleanup(&self, call: Call, absent_allowed: bool) {
        if call.exact() {
            return;
        }
        if self.cleanup_error.get() == S_OK {
            self.cleanup_error.set(if call.result.is_err() {
                call.result
            } else {
                E_FAIL
            });
        }
        if !absent_allowed {
            self.uncertain.set(true);
        }
    }

    fn drain(&self) -> HRESULT {
        if let Some(keys) = &self.keys {
            while self.bindings.get() != 0 {
                let index = self.bindings.get() - 1;
                self.bindings.set(index);
                let call = tsf::unpreserve(keys, &BINDINGS[index]);
                self.record_cleanup(call, call.result == CONNECT_E_NOCONNECTION);
            }
            if self.key_sink.replace(false) {
                self.record_cleanup(tsf::unadvise_keys(keys, self.client), false);
            }
        }
        if let Some(source) = &self.source {
            for cookie in [&self.thread_cookie, &self.profile_cookie] {
                let value = cookie.replace(TF_INVALID_COOKIE);
                if value != TF_INVALID_COOKIE {
                    self.record_cleanup(tsf::unadvise(source, value), false);
                }
            }
        }
        self.cleanup_error.get()
    }
}

impl Drop for Resources {
    fn drop(&mut self) {
        let _ = self.drain();
        // Release can reenter. Draining stays visible until every captured lease has been released.
        drop(self.profiles.take());
        drop(self.keys.take());
        drop(self.source.take());
        drop(self.manager.take());
        if let Some(owner) = self.owner.upgrade() {
            let mut state = owner.state.borrow_mut();
            if state.epoch == self.epoch && state.activation == Activation::Draining {
                state.activation = if self.uncertain.get() || owner.poisoned.get() {
                    Activation::Faulted
                } else {
                    Activation::Inactive
                };
            }
        }
    }
}

struct DiagnosticDepth<'a>(&'a Service);
impl Drop for DiagnosticDepth<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state.borrow_mut();
        state.diagnostic_depth -= 1;
        let publish = state.diagnostic_depth == 0;
        drop(state);
        if publish {
            self.0.publish_observation();
        }
    }
}

struct ActivationCall<'a>(&'a Service);
impl Drop for ActivationCall<'_> {
    fn drop(&mut self) {
        self.0.state.borrow_mut().activation_open = false;
    }
}

struct ExecutionCall<'a>(&'a Service);
impl Drop for ExecutionCall<'_> {
    fn drop(&mut self) {
        self.0.state.borrow_mut().busy = false;
    }
}

struct AdmissionCall<'a>(&'a Service);
impl Drop for AdmissionCall<'_> {
    fn drop(&mut self) {
        self.0.state.borrow_mut().admitting_key = false;
    }
}

struct HostObservationCall<'a>(&'a Service);
impl Drop for HostObservationCall<'_> {
    fn drop(&mut self) {
        self.0.state.borrow_mut().observing_host = false;
    }
}

struct AdmittedKey {
    guard: Guard,
    mode: u32,
    keyboard: [u8; 256],
    layout: windows::Win32::UI::Input::KeyboardAndMouse::HKL,
    target_layout: Option<windows::Win32::UI::Input::KeyboardAndMouse::HKL>,
}

struct AltCycle {
    ownership: Rc<OwnedIdentity>,
    guard: Guard,
    mode: u32,
    context: usize,
    composition: usize,
    field: crate::field::Identity,
}

struct ReleaseDecision<'a> {
    service: &'a Service,
    parent: Option<Rc<Cell<bool>>>,
    cancelled: Rc<Cell<bool>>,
}

impl<'a> ReleaseDecision<'a> {
    fn new(service: &'a Service) -> Self {
        let parent = service.state.borrow().release_decision.clone();
        if parent.is_some() {
            service.revoke_release();
        }
        let cancelled = Rc::new(Cell::new(false));
        service.state.borrow_mut().release_decision = Some(Rc::clone(&cancelled));
        Self {
            service,
            parent,
            cancelled,
        }
    }

    fn current(&self) -> bool {
        self.parent.is_none() && !self.cancelled.get()
    }
}

impl Drop for ReleaseDecision<'_> {
    fn drop(&mut self) {
        self.service.state.borrow_mut().release_decision = self.parent.take();
    }
}

pub(crate) struct Service {
    owner_thread: u32,
    poisoned: Cell<bool>,
    state: RefCell<State>,
    #[cfg(feature = "test-api")]
    panic_at: Cell<u32>,
    #[cfg(feature = "test-api")]
    pub(crate) test_key_translation: Cell<Option<foreign::win32::KeyTranslation>>,
    _dependencies: Option<DependencyLease>,
    _object: ObjectLease,
}

impl Service {
    pub(crate) fn new() -> Rc<Self> {
        Self::with_dependencies(None)
    }

    pub(crate) fn with_dependencies(dependencies: Option<DependencyLease>) -> Rc<Self> {
        Rc::new(Self {
            // SAFETY: GetCurrentThreadId has no pointer or lifetime preconditions.
            owner_thread: unsafe { GetCurrentThreadId() },
            poisoned: Cell::new(false),
            state: RefCell::new(State::default()),
            #[cfg(feature = "test-api")]
            panic_at: Cell::new(0),
            #[cfg(feature = "test-api")]
            test_key_translation: Cell::new(None),
            _dependencies: dependencies,
            _object: ObjectLease::new(),
        })
    }

    fn key_translation(&self) -> foreign::win32::KeyTranslation {
        #[cfg(feature = "test-api")]
        if let Some(translation) = self.test_key_translation.get() {
            return translation;
        }
        foreign::win32::KEY_TRANSLATION
    }

    pub(crate) fn maintain(self: &Rc<Self>) -> HRESULT {
        if !self.owner_thread() {
            return RPC_E_WRONG_THREAD;
        }
        let mut state = self.state.borrow_mut();
        if state.activation != Activation::Active
            || self.poisoned.get()
            || state.diagnostic_depth != 0
            || state.busy
            || state.admitting_key
        {
            return S_OK;
        }
        let guard = Guard {
            epoch: state.epoch,
            generation: state.generation,
        };
        let latest = state.worker.as_mut().and_then(vocabulary::Worker::poll);
        if let Some(latest) = latest
            && latest.activation == state.runtime.activation
            && latest.revision > state.runtime.revision
        {
            state.pending_ready = Some(latest);
        }
        let Some(ready) = state.pending_ready.clone() else {
            return S_OK;
        };
        if source::veto(&state) || !state.source.accepts_ready(&ready) {
            state.source.cancel();
        }
        state.runtime.configuration_error = ready.configuration_error;
        state.runtime.error = ready.error;
        if let Some(configuration) = &ready.configuration {
            state.runtime.requested = configuration.policy();
        }
        if !ready.model {
            state.runtime.error = ready.error;
            return S_OK;
        }
        if let Some(owned) = &state.owned {
            if !state.stopped && state.pending_restricts() {
                let revision = ready
                    .configuration
                    .as_ref()
                    .expect("changed complete configuration")
                    .settings
                    .revision;
                let attempt = (owned.serial, revision);
                if state.retirement_attempt != Some(attempt) {
                    state.retirement_attempt = Some(attempt);
                    let mode = state.mode;
                    drop(state);
                    return self.retire_configuration(guard, mode);
                }
            }
            return S_OK;
        }
        if state.source.defer_adoption() {
            return S_OK;
        }
        let _ = Self::adopt_ready(&mut state, ready);
        S_OK
    }

    fn adopt_ready(state: &mut State, ready: Ready) -> Result<(), u32> {
        if !state.source.accepts_ready(&ready) {
            state.source.cancel();
        }
        let policy = ready
            .configuration
            .as_ref()
            .map_or(if ready.model { 2 } else { 0 }, |configuration| {
                configuration.policy()
            });
        let Some(engine) = state.engine.as_mut() else {
            return Err(5);
        };
        if engine
            .policy(policy)
            .and_then(|()| engine.adopt(&ready))
            .is_err()
        {
            state.runtime.error = RuntimeError::Startup;
            state.stopped = true;
            return Err(5);
        }
        state.runtime.model_ready = ready.model as u32;
        state.runtime.error = ready.error;
        for (index, source) in ready.sources.iter().enumerate() {
            state.runtime.sources[index] = source.status;
            state.runtime.source_errors[index] = source.error;
        }
        state.application_off = ready.configuration.as_ref().is_some_and(|configuration| {
            configuration.application == crate::settings::AppPolicy::Off
        });
        state.runtime.requested = policy;
        state.runtime.effective = policy;
        state.runtime.revision = ready.revision;
        state.applied_ready = Some(ready);
        state.pending_ready = None;
        state.retirement_attempt = None;
        Ok(())
    }

    fn retire_configuration(self: &Rc<Self>, guard: Guard, mode: u32) -> HRESULT {
        let Some(authority) = self.capture_authority() else {
            return S_OK;
        };
        let Some(frame) = crate::edit::Frame::retirement(Rc::clone(self), authority, mode) else {
            return S_OK;
        };
        if !self.current(guard) || !frame.alive() {
            return S_OK;
        }
        self.state.borrow_mut().busy = true;
        let busy = ExecutionCall(self);
        let result = frame.request_editing();
        if result.effect == crate::edit::Effect::Attempted {
            self.invalidate_if(guard, true);
        }
        let retired = result.outer.exact()
            && result.callback == S_OK
            && result.completed
            && result.valid
            && self.current(guard)
            && self.owned().is_none();
        drop(frame);
        drop(busy);
        if retired { self.maintain() } else { S_OK }
    }

    fn start_runtime(self: &Rc<Self>, epoch: u64) {
        let (guard, model, activation) = {
            let state = self.state.borrow();
            let Some(config @ (Config::Model(_) | Config::Persisted)) = &state.accepted_config
            else {
                return;
            };
            (
                Guard {
                    epoch,
                    generation: state.generation,
                },
                config.clone(),
                state.runtime.activation,
            )
        };
        if !self.current(guard) {
            return;
        }
        let published = Rc::new(Cell::new(false));
        let current_published = Rc::clone(&published);
        let current_service = Rc::downgrade(self);
        let current = Rc::new(move || {
            current_service.upgrade().is_some_and(|service| {
                if current_published.get() {
                    service.runtime_current(epoch)
                } else {
                    service.current(guard)
                }
            })
        });
        let tick_service = Rc::downgrade(self);
        let tick = Rc::new(move || {
            if let Some(service) = tick_service.upgrade()
                && service.runtime_current(epoch)
            {
                let _ = service.maintain();
            }
        });
        let window = crate::runtime_window::Window::start(current, tick);
        if !self.current(guard) {
            drop(window);
            return;
        }
        let Some(window) = window else {
            self.state.borrow_mut().runtime.error = RuntimeError::Startup;
            return;
        };
        let mut worker = vocabulary::Worker::default();
        let owner = if self._dependencies.is_some() {
            ModuleOwner::Process
        } else {
            ModuleOwner::Dll
        };
        let started = match &model {
            Config::Model(model) => {
                worker.start(model, activation, owner, vocabulary::Hooks::default())
            }
            Config::Persisted => {
                worker.start_persisted(activation, owner, vocabulary::Hooks::default())
            }
            _ => false,
        };
        if !self.current(guard) {
            drop(worker);
            drop(window);
            return;
        }
        if !started {
            drop(worker);
            drop(window);
            if self.current(guard) {
                self.state.borrow_mut().runtime.error = RuntimeError::Startup;
            }
            return;
        }
        let mut state = self.state.borrow_mut();
        published.set(true);
        state.worker = Some(worker);
        state.maintenance_window = Some(window);
    }

    fn runtime_current(&self, epoch: u64) -> bool {
        let state = self.state.borrow();
        !self.poisoned.get() && state.activation == Activation::Active && state.epoch == epoch
    }

    fn stop_runtime(&self) {
        let (worker, window, pending, applied) = {
            let mut state = self.state.borrow_mut();
            state.retirement_attempt = None;
            (
                state.worker.take(),
                state.maintenance_window.take(),
                state.pending_ready.take(),
                state.applied_ready.take(),
            )
        };
        drop(worker);
        drop(pending);
        drop(applied);
        drop(window);
    }

    pub(crate) fn owner_thread(&self) -> bool {
        // SAFETY: GetCurrentThreadId has no pointer or lifetime preconditions.
        unsafe { GetCurrentThreadId() == self.owner_thread }
    }

    pub(crate) fn entry(&self, call: impl FnOnce() -> HRESULT) -> HRESULT {
        if self.poisoned.get() {
            return E_FAIL;
        }
        self.fenced(call)
    }

    fn fenced(&self, call: impl FnOnce() -> HRESULT) -> HRESULT {
        match catch_unwind(AssertUnwindSafe(call)) {
            Ok(result) => result,
            Err(_) => {
                self.poisoned.set(true);
                self.invalidate();
                E_FAIL
            }
        }
    }

    pub(crate) fn checked(&self, call: impl FnOnce() -> HRESULT) -> HRESULT {
        if !self.owner_thread() {
            return RPC_E_WRONG_THREAD;
        }
        self.entry(|| self.callback_scope(call))
    }

    fn callback_scope(&self, call: impl FnOnce() -> HRESULT) -> HRESULT {
        self.state.borrow_mut().diagnostic_depth += 1;
        let _depth = DiagnosticDepth(self);
        call()
    }

    fn cleanup(&self, call: impl FnOnce() -> HRESULT) -> HRESULT {
        if !self.owner_thread() {
            return RPC_E_WRONG_THREAD;
        }
        self.fenced(|| self.callback_scope(call))
    }

    pub(crate) fn invalidate(&self) {
        self.state.borrow_mut().source.cancel();
        self.invalidate_generation();
    }

    fn invalidate_generation(&self) {
        self.revoke_release();
        let mut state = self.state.borrow_mut();
        state.metadata = crate::observation::Metadata::default();
        if let Some(generation) = state.generation.checked_add(1) {
            state.generation = generation;
        } else {
            state.stopped = true;
        }
        if let Some(engine) = state.engine.as_mut() {
            engine.reset();
        }
    }

    pub(crate) fn invalidate_for_lifecycle(&self, cause: LifecycleCause) {
        let serial = self
            .state
            .borrow()
            .owned
            .as_ref()
            .map_or(0, |owned| owned.serial);
        self.lifecycle(
            LifecycleKind::LifecycleInvalidation,
            serial,
            cause,
            Unresolved::None,
        );
        if cause == LifecycleCause::ProfileChanged
            && matches!(self.state.borrow().source, source::SourceState::Changing(_))
        {
            self.invalidate_generation();
        } else {
            self.invalidate_word(self.owned().is_some());
        }
    }

    pub(crate) fn terminated(&self, composition: *mut c_void) -> HRESULT {
        self.cleanup(|| {
            let owned = self.owned();
            let serial = owned.as_ref().map_or(0, |owned| owned.serial);
            if composition.is_null() || owned.is_none() {
                self.lifecycle(
                    if composition.is_null() {
                        LifecycleKind::TerminationUnresolved
                    } else {
                        LifecycleKind::TerminationWithoutOwned
                    },
                    serial,
                    LifecycleCause::None,
                    if composition.is_null() {
                        Unresolved::NullArgument
                    } else {
                        Unresolved::None
                    },
                );
                return S_OK;
            }
            let Some(owned) = owned else {
                return S_OK;
            };
            let guard = {
                let state = self.state.borrow();
                Guard {
                    epoch: state.epoch,
                    generation: state.generation,
                }
            };
            let generation = guard.generation;
            // SAFETY: TSF retains its incoming composition throughout this callback.
            let identity = unsafe { foreign::query::<IUnknown>(composition) };
            let identity = match identity {
                Ok(identity) => identity,
                Err(_) => {
                    self.lifecycle(
                        LifecycleKind::TerminationUnresolved,
                        serial,
                        LifecycleCause::None,
                        Unresolved::IdentityFailure,
                    );
                    return S_OK;
                }
            };
            let current = self.owned();
            if generation != self.state.borrow().generation || current.is_none() {
                self.lifecycle(
                    LifecycleKind::TerminationUnresolved,
                    serial,
                    LifecycleCause::None,
                    Unresolved::Stale,
                );
                return S_OK;
            }
            let same_word = current
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(&current.identity, &owned.identity));
            if identity.address() == owned.composition_id.address() && same_word {
                self.lifecycle(
                    if same_word {
                        LifecycleKind::ExactTermination
                    } else {
                        LifecycleKind::TerminationUnresolved
                    },
                    serial,
                    LifecycleCause::None,
                    if same_word {
                        Unresolved::None
                    } else {
                        Unresolved::Stale
                    },
                );
                self.invalidate();
                let retired = self.state.borrow_mut().owned.take();
                drop(retired);
            } else {
                self.lifecycle(
                    if same_word {
                        LifecycleKind::DifferentTermination
                    } else {
                        LifecycleKind::TerminationUnresolved
                    },
                    serial,
                    LifecycleCause::None,
                    if same_word {
                        Unresolved::None
                    } else {
                        Unresolved::Stale
                    },
                );
            }
            S_OK
        })
    }

    fn preparing(&self, epoch: u64) -> bool {
        let state = self.state.borrow();
        !self.poisoned.get() && state.epoch == epoch && state.activation == Activation::Preparing
    }

    fn acquisition(&self, epoch: u64, call: Call) -> Result<(), HRESULT> {
        if !call.exact() {
            return Err(if call.result.is_err() {
                call.result
            } else {
                E_FAIL
            });
        }
        if !self.preparing(epoch) {
            return Err(E_FAIL);
        }
        Ok(())
    }

    fn acquire(
        self: &Rc<Self>,
        manager: *mut c_void,
        native: &mut Resources,
        sinks: &Sinks,
    ) -> Result<(), HRESULT> {
        // SAFETY: the inbound COM caller retains its manager for the complete Activate callback.
        native.manager =
            Some(unsafe { foreign::retain::<ITfThreadMgr>(manager) }.map_err(tsf::failure)?);
        if !self.preparing(native.epoch) {
            return Err(E_FAIL);
        }
        let manager = native.manager.as_ref().ok_or(E_FAIL)?;
        native.source = Some(
            manager
                .query()
                .map_err(|error| tsf::failure(error.call()))?,
        );
        if !self.preparing(native.epoch) {
            return Err(E_FAIL);
        }
        native.keys = Some(
            manager
                .query()
                .map_err(|error| tsf::failure(error.call()))?,
        );
        if !self.preparing(native.epoch) {
            return Err(E_FAIL);
        }
        let source = native.source.as_ref().ok_or(E_FAIL)?;
        for (id, sink, target) in [
            (
                ITfThreadMgrEventSink::IID,
                sinks.thread,
                &native.thread_cookie,
            ),
            (
                ITfInputProcessorProfileActivationSink::IID,
                sinks.profile,
                &native.profile_cookie,
            ),
        ] {
            let (call, cookie) = tsf::advise(source, &id, sink);

            if call.exact() {
                target.set(cookie);
                if cookie == TF_INVALID_COOKIE {
                    native.uncertain.set(true);
                }
            }
            self.acquisition(native.epoch, call)?;
            if cookie == TF_INVALID_COOKIE {
                native.uncertain.set(true);
                return Err(E_FAIL);
            }
        }
        let keys = native.keys.as_ref().ok_or(E_FAIL)?;
        let call = tsf::advise_keys(keys, native.client, sinks.keys);

        if call.exact() {
            native.key_sink.set(true);
        }
        self.acquisition(native.epoch, call)?;
        for binding in &BINDINGS {
            let call = tsf::preserve(keys, native.client, binding);

            if call.exact() {
                native.bindings.set(native.bindings.get() + 1);
            }
            self.acquisition(native.epoch, call)?;
        }
        let injected = self
            ._dependencies
            .as_ref()
            .map_or(std::ptr::null_mut(), |deps| deps.table.profiles);
        native.profiles = if injected.is_null() {
            tsf::create_profiles().ok()
        } else {
            // SAFETY: the dependency owner retains the injected SDK profile manager.
            unsafe { foreign::retain::<ITfInputProcessorProfileMgr>(injected) }.ok()
        };
        if !self.preparing(native.epoch) {
            return Err(E_FAIL);
        }
        if let Some(profiles) = &native.profiles {
            let generation = self.state.borrow().generation;
            let (call, profile) = tsf::active_profile(profiles);
            if call.exact()
                && self.preparing(native.epoch)
                && self.state.borrow().generation == generation
            {
                let _ = self.profile_changed(profile);
            }
        }
        if self.preparing(native.epoch) {
            Ok(())
        } else {
            Err(E_FAIL)
        }
    }

    pub(crate) fn activate(
        self: &Rc<Self>,
        manager: *mut c_void,
        client: u32,
        flags: u32,
        sinks: Sinks,
    ) -> HRESULT {
        self.checked(|| {
            let epoch = {
                let mut state = self.state.borrow_mut();
                if state.activation != Activation::Inactive
                    || state.activation_open
                    || manager.is_null()
                    || client == 0
                    || state.epoch == u64::MAX
                {
                    return E_INVALIDARG;
                }
                if flags & TF_TMAE_SECUREMODE != 0 {
                    return E_ACCESSDENIED;
                }
                state.activation_open = true;
                state.activation = Activation::Preparing;
                state.epoch += 1;
                state.epoch
            };
            let _call = ActivationCall(self);
            let configuration = vocabulary::capture_config();
            let invalid_configuration = configuration.is_none();
            {
                let mut state = self.state.borrow_mut();
                if let Some(config) = configuration {
                    state.accepted_config = Some(config);
                }
                if state.accepted_config.is_none() {
                    state.accepted_config = Some(Config::Off);
                }
                let policy = state.accepted_config.as_ref().map_or(0, Config::policy);
                state.application_off = matches!(state.accepted_config, Some(Config::Persisted));
                state.applied_ready = None;
                state.pending_ready = None;
                state.retirement_attempt = None;
                state.runtime = RuntimeSnapshot {
                    requested: policy,
                    configuration_error: if invalid_configuration {
                        RuntimeError::Configuration
                    } else {
                        RuntimeError::None
                    },
                    ..RuntimeSnapshot::default()
                };
                if state.engine.is_none() {
                    state.engine = Engine::new().ok();
                }
                let Some(engine) = state.engine.as_mut() else {
                    state.activation = Activation::Inactive;
                    return E_FAIL;
                };
                if engine
                    .configure(if policy == 2 { 0 } else { policy })
                    .is_err()
                {
                    state.activation = Activation::Inactive;
                    return E_FAIL;
                }
                state.stopped = false;
                state.mode = 0;
            }
            let mut resources = Resources::new(self, epoch, client);
            let result = self.acquire(manager, &mut resources, &sinks);
            if result.is_ok() && self.preparing(epoch) {
                {
                    let mut state = self.state.borrow_mut();
                    state.resources = Some(Rc::new(resources));
                    state.activation = Activation::Active;
                    let diagnostics = &mut state.diagnostics;
                    diagnostics::increment(&mut diagnostics.activation, &mut diagnostics.saturated);
                    state.runtime.activation = state.diagnostics.activation;
                    state.runtime.effective = if state.runtime.requested == 2 {
                        0
                    } else {
                        state.runtime.requested
                    };
                }
                // SAFETY: these pseudo handles and process/thread IDs refer to the current host.
                let identity = unsafe {
                    crate::observation::capture_identity(
                        windows::Win32::System::Threading::GetCurrentProcess(),
                        windows::Win32::System::Threading::GetCurrentProcessId(),
                        self.owner_thread,
                    )
                }
                .unwrap_or_default();
                if self.same_activation(epoch) {
                    let retired = self.state.borrow_mut().publisher.take();
                    drop(retired);
                    let mut publisher = crate::observation::Publisher::default();
                    if identity.kind != 0 {
                        publisher.attach(&identity);
                    }
                    if self.same_activation(epoch) {
                        let mut state = self.state.borrow_mut();
                        state.host_identity = identity;
                        state.metadata = crate::observation::Metadata::default();
                        state.observed_generation = 0;
                        state.publisher = Some(Rc::new(publisher));
                    }
                }
                self.register_diagnostics(epoch, sinks.provider);
                if self.same_activation(epoch) {
                    self.start_runtime(epoch);
                }
                return if self.same_activation(epoch) {
                    S_OK
                } else {
                    E_FAIL
                };
            }
            {
                let mut state = self.state.borrow_mut();
                state.activation = Activation::Draining;
                state.mode = 0;
            }
            self.invalidate();
            drop(resources);
            result
                .err()
                .filter(|result| result.is_err())
                .unwrap_or(E_FAIL)
        })
    }

    pub(crate) fn deactivate(&self) -> HRESULT {
        self.cleanup(|| {
            let retired = {
                let mut state = self.state.borrow_mut();
                if state.activation == Activation::Preparing {
                    state.activation = Activation::Cancelling;
                    state.mode = 0;
                    drop(state);
                    self.invalidate();
                    return S_OK;
                }
                if state.activation != Activation::Active {
                    return S_OK;
                }
                state.activation = Activation::Draining;
                state.mode = 0;
                (
                    state.resources.take(),
                    state.diagnostic_source.take(),
                    state.owned.take(),
                )
            };
            let (resources, diagnostic_source, owned) = retired;
            let serial = owned.as_ref().map_or(0, |owned| owned.serial);
            self.lifecycle(
                LifecycleKind::LifecycleInvalidation,
                serial,
                LifecycleCause::Deactivate,
                Unresolved::None,
            );
            self.invalidate_word(owned.is_some());
            if owned.is_some() {
                self.lifecycle(
                    LifecycleKind::Detached,
                    serial,
                    LifecycleCause::None,
                    Unresolved::None,
                );
            }
            self.stop_runtime();
            let result = resources
                .as_ref()
                .map_or(S_OK, |resources| resources.drain());
            if let (Some(source), Some(native)) = (&diagnostic_source, &resources) {
                tsf::unadvise_single(source, native.client);
            }
            drop(diagnostic_source);
            drop(owned);
            drop(resources);
            result
        })
    }

    fn same_activation(&self, epoch: u64) -> bool {
        let state = self.state.borrow();
        !self.poisoned.get() && state.epoch == epoch && state.activation == Activation::Active
    }

    fn register_diagnostics(&self, epoch: u64, provider: *mut c_void) {
        let mut path = [0; 32768];
        // SAFETY: the bounded output receives only the current process executable path.
        let count = foreign::win32::module_path(None, &mut path) as usize;
        let proof = count != 0
            && count < path.len()
            && path[..count]
                .iter()
                .rposition(|unit| *unit == b'\\' as u16)
                .is_some_and(|index| {
                    String::from_utf16_lossy(&path[index + 1..count])
                        .eq_ignore_ascii_case("KeyrillProofEditor.exe")
                });
        if !proof || !self.same_activation(epoch) {
            return;
        }
        let resources = self.state.borrow().resources.clone();
        let Some(resources) = resources else {
            return;
        };
        let Some(manager) = resources.manager.as_ref() else {
            return;
        };
        let Ok(source) = manager.query::<ITfSourceSingle>() else {
            return;
        };
        if !self.same_activation(epoch) {
            return;
        }
        let call = tsf::advise_single(&source, resources.client, provider);
        if call.result.is_ok() && self.same_activation(epoch) {
            let retired = self.state.borrow_mut().diagnostic_source.replace(source);
            drop(retired);
        } else {
            tsf::unadvise_single(&source, resources.client);
        }
    }

    pub(crate) fn profile_changed(&self, profile: TF_INPUTPROCESSORPROFILE) -> HRESULT {
        self.checked(|| {
            self.source_profile_observed(&profile);
            self.invalidate_for_lifecycle(LifecycleCause::ProfileChanged);
            let mut state = self.state.borrow_mut();
            state.mode = if profile.clsid == SERVICE_ID && profile.dwFlags & TF_IPP_FLAG_ACTIVE != 0
            {
                PROFILES
                    .iter()
                    .find(|entry| entry.id == profile.guidProfile)
                    .map_or(0, |entry| entry.mode)
            } else {
                0
            };
            S_OK
        })
    }

    pub(crate) fn revoke_release(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(decision) = &state.release_decision {
            decision.set(true);
        }
        state.alt_cycle = None;
        state.release_permission = None;
    }

    pub(crate) fn revoke_release_if(&self, guard: Guard) {
        if self.state.borrow().epoch == guard.epoch {
            self.revoke_release();
        }
    }

    fn same_cycle(&self, cycle: &Rc<AltCycle>) -> bool {
        let state = self.state.borrow();
        state
            .alt_cycle
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, cycle))
            && self.state_current(&state, cycle.guard)
            && state.mode == cycle.mode
            && state.owned.as_ref().is_some_and(|owned| {
                Rc::ptr_eq(&owned.identity, &cycle.ownership)
                    && owned.context_id.address() == cycle.context
                    && owned.composition_id.address() == cycle.composition
            })
    }

    unsafe fn release_authority(
        self: &Rc<Self>,
        owner: crate::com::OwnServiceLease,
        context: *mut c_void,
        ownership: &Rc<OwnedIdentity>,
        guard: Guard,
        mode: u32,
        decision: &ReleaseDecision<'_>,
    ) -> Option<crate::field::Identity> {
        if context.is_null() || !decision.current() || !self.current_owned(guard, Some(ownership)) {
            return None;
        }
        let authority = self.capture_authority()?;
        let profiles = self.state.borrow().resources.as_ref()?.profiles.clone()?;
        let (call, profile) = tsf::active_profile(&profiles);
        let exact = call.exact()
            && decision.current()
            && self.current_owned(guard, Some(ownership))
            && crate::keys::active_profile(&profile, mode);
        drop(profiles);
        if !exact || !decision.current() || !self.current_owned(guard, Some(ownership)) {
            return None;
        }
        // SAFETY: the native key callback retains context through this synchronous, read-only proof.
        let field = unsafe {
            crate::edit::Frame::release_field(
                Rc::clone(self),
                owner,
                context,
                authority,
                Rc::clone(&decision.cancelled),
            )
        };
        if decision.current() && self.current_owned(guard, Some(ownership)) {
            field
        } else {
            None
        }
    }

    unsafe fn witness_alt(
        self: &Rc<Self>,
        owner: crate::com::OwnServiceLease,
        context: *mut c_void,
        decision: &ReleaseDecision<'_>,
    ) {
        let mut state = self.state.borrow_mut();
        let stamp = Self::observation_stamp(&state);
        state.observations.observer(
            diagnostics::ObserverObservation {
                stage: diagnostics::ObserverStage::Arrival,
            },
            stamp,
        );
        drop(state);
        let admitted = (|| {
            let authority = self.capture_authority()?;
            let owned = authority.owned.as_ref()?;
            let (ownership, guard, mode, expected_context, composition) = (
                Rc::clone(&owned.identity),
                authority.guard,
                self.state.borrow().mode,
                owned.context_id.address(),
                owned.composition_id.address(),
            );
            drop(authority);
            if !decision.current() {
                return None;
            }
            let mut keyboard = [0; 256];
            // SAFETY: the complete 256-byte state is writable through the native snapshot call.
            let keyboard_result = foreign::keyboard_state(&mut keyboard);
            let left_alt = keyboard[0xa4] & 0x80 != 0;
            let right_alt = keyboard[0xa5] & 0x80 != 0;
            if !keyboard_result || !left_alt || right_alt {
                return None;
            }
            // SAFETY: the SDK caller retains context through this observer callback.
            let field = unsafe {
                self.release_authority(owner, context, &ownership, guard, mode, decision)
            }?;
            Some(Rc::new(AltCycle {
                ownership,
                guard,
                mode,
                context: expected_context,
                composition,
                field,
            }))
        })();
        let mut state = self.state.borrow_mut();
        if decision.current() {
            state.alt_cycle = admitted;
        }
        let stamp = Self::observation_stamp(&state);
        let stage = if state.alt_cycle.is_some() {
            diagnostics::ObserverStage::Admitted
        } else {
            diagnostics::ObserverStage::Refused
        };
        state
            .observations
            .observer(diagnostics::ObserverObservation { stage }, stamp);
    }

    unsafe fn dispatch_command(
        self: &Rc<Self>,
        owner: crate::com::OwnServiceLease,
        context: *mut c_void,
        event: NativeEvent,
        ticket: diagnostics::CommandTicket,
        decision: &ReleaseDecision<'_>,
    ) -> bool {
        let cycle = self.state.borrow().alt_cycle.clone();
        let witnessed = cycle.as_ref().is_some_and(|cycle| self.same_cycle(cycle));
        let permission = self.state.borrow().release_permission.clone();
        // SAFETY: the command's native callback retains context through this synchronous request.
        let execution = unsafe { self.execute(owner, context, event, false) };
        let reserved_permission = cycle.as_ref().is_some_and(|cycle| {
            permission
                .as_ref()
                .is_some_and(|permission| Rc::ptr_eq(permission, cycle))
        }) && permission.as_ref().is_some_and(|permission| {
            self.state
                .borrow()
                .release_permission
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(current, permission))
        });
        if decision.current()
            && cycle.as_ref().is_some_and(|cycle| self.same_cycle(cycle))
            && (execution.completion == crate::edit::Completion::ManualMark
                || (execution.completion == crate::edit::Completion::Reserved
                    && reserved_permission))
        {
            self.state.borrow_mut().release_permission = cycle;
        } else {
            self.revoke_release();
        }
        self.command_complete(ticket, execution.completion, witnessed);
        execution.eaten
    }

    fn observe_release(
        &self,
        delivery: diagnostics::ReleaseDelivery,
        modifier: diagnostics::ReleaseModifier,
        context: diagnostics::ReleaseContext,
        consumed: bool,
    ) {
        let mut state = self.state.borrow_mut();
        let stamp = Self::observation_stamp(&state);
        state.observations.release(
            diagnostics::ReleaseObservation {
                delivery,
                modifier,
                context,
                consumed,
            },
            stamp,
        );
    }

    pub(crate) unsafe fn release_alt(
        self: &Rc<Self>,
        owner: crate::com::OwnServiceLease,
        context: *mut c_void,
        key: usize,
        flags: isize,
        eaten: *mut windows_core::BOOL,
        delivery: diagnostics::ReleaseDelivery,
    ) -> HRESULT {
        if !self.owner_thread() {
            return S_OK;
        }
        self.checked(|| {
            if delivery == diagnostics::ReleaseDelivery::Actual {
                self.source_input();
            }
            let Some(modifier) = diagnostics::classify_relevant_release(key, flags) else {
                return S_OK;
            };
            let decision = ReleaseDecision::new(self);
            if !matches!(
                modifier,
                diagnostics::ReleaseModifier::LeftAlt
                    | diagnostics::ReleaseModifier::RightAlt
                    | diagnostics::ReleaseModifier::AltUnresolved
            ) {
                self.observe_release(
                    delivery,
                    modifier,
                    diagnostics::ReleaseContext::Unchecked,
                    false,
                );
                return S_OK;
            }
            let cycle = self.state.borrow().alt_cycle.clone();
            let mut accepted = false;
            let mut context_match = diagnostics::ReleaseContext::Unchecked;
            if modifier == diagnostics::ReleaseModifier::LeftAlt
                && decision.current()
                && let Some(cycle) = &cycle
                && self.same_cycle(cycle)
                && self
                    .state
                    .borrow()
                    .release_permission
                    .as_ref()
                    .is_some_and(|permission| Rc::ptr_eq(permission, cycle))
            {
                context_match = diagnostics::ReleaseContext::Refused;
                // SAFETY: the receiving SDK callback keeps context live through this proof.
                let field = unsafe {
                    self.release_authority(
                        owner,
                        context,
                        &cycle.ownership,
                        cycle.guard,
                        cycle.mode,
                        &decision,
                    )
                };
                accepted = field == Some(cycle.field)
                    && decision.current()
                    && self.same_cycle(cycle)
                    && self
                        .state
                        .borrow()
                        .release_permission
                        .as_ref()
                        .is_some_and(|permission| Rc::ptr_eq(permission, cycle));
                if accepted {
                    context_match = diagnostics::ReleaseContext::Matched;
                }
            }
            if delivery == diagnostics::ReleaseDelivery::Actual
                || context_match == diagnostics::ReleaseContext::Refused
            {
                self.revoke_release();
            }
            self.observe_release(delivery, modifier, context_match, accepted);
            // SAFETY: the raw boundary checked/defaulted this output and the callback owns it.
            unsafe { eaten.write(windows_core::BOOL(accepted as i32)) };
            S_OK
        })
    }

    fn current(&self, guard: Guard) -> bool {
        self.state_current(&self.state.borrow(), guard)
    }

    fn admission_call(&self, guard: Guard, call: Call, stage: Stage) -> Result<(), HRESULT> {
        if !call.exact() {
            self.refuse(stage, Cause::Native, call.result);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);

            return Err(S_OK);
        }
        if !self.current(guard) {
            self.refuse(stage, Cause::Stale, S_OK);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        }
        Ok(())
    }

    fn admit_key(
        self: &Rc<Self>,
        context: *mut c_void,
        acquired: Option<&mut diagnostics::ModifierKnowledge>,
        owner: &crate::com::OwnServiceLease,
    ) -> Result<AdmittedKey, HRESULT> {
        use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
        let state = self.state.borrow();
        if state.application_off
            || (state.owned.is_none()
                && state.pending_ready.as_ref().is_some_and(|ready| {
                    ready.configuration.as_ref().is_some_and(|configuration| {
                        configuration.application == crate::settings::AppPolicy::Off
                    })
                }))
        {
            drop(state);
            self.refuse(Stage::Key, Cause::Predicate, S_OK);
            return Err(S_OK);
        }
        if state.busy || state.admitting_key || state.observing_host {
            drop(state);
            self.refuse(Stage::Key, Cause::Reentry, S_OK);
            self.invalidate_word(self.owned().is_some());
            return Err(S_OK);
        }
        if context.is_null() {
            drop(state);
            self.refuse(Stage::Key, Cause::Missing, S_OK);
            return Err(S_OK);
        }
        let guard = Guard {
            epoch: state.epoch,
            generation: state.generation,
        };
        let mode = state.mode;
        let resources = state.resources.clone();
        drop(state);
        self.state.borrow_mut().admitting_key = true;
        let _admission = AdmissionCall(self);
        let mut keyboard = [0; 256];
        // SAFETY: keyboard is a complete writable 256-key snapshot.
        if !foreign::keyboard_state(&mut keyboard) {
            self.refuse(Stage::Keyboard, Cause::Predicate, S_OK);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        }
        if let Some(acquired) = acquired {
            *acquired = diagnostics::ModifierKnowledge::Known(diagnostics::ModifierFacts::project(
                &keyboard,
            ));
        }
        let manager = resources
            .as_ref()
            .and_then(|resources| resources.profiles.clone());
        drop(resources);
        let Some(manager) = manager else {
            let state = self.state.borrow();
            let cause = if state.activation != Activation::Active {
                Cause::Inactive
            } else if state.stopped {
                Cause::Stopped
            } else {
                Cause::Missing
            };
            drop(state);
            self.refuse(Stage::Profile, cause, S_OK);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        };
        if !self.current(guard) {
            self.refuse(Stage::Profile, Cause::Stale, S_OK);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        }
        let (call, profile) = tsf::active_profile(&manager);
        self.admission_call(guard, call, Stage::Profile)?;
        if !crate::keys::active_profile(&profile, mode) {
            let cause = if profile.dwProfileType != TF_PROFILETYPE_INPUTPROCESSOR {
                Cause::WrongType
            } else if profile.clsid != SERVICE_ID {
                Cause::WrongService
            } else if profile.dwFlags & TF_IPP_FLAG_ACTIVE == 0 {
                Cause::NotActive
            } else {
                Cause::WrongProfile
            };
            self.refuse(Stage::Layout, cause, S_OK);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        }
        let substitute = match manager.query::<ITfInputProcessorProfileSubstituteLayout>() {
            Ok(substitute) => substitute,
            Err(error) => {
                self.refuse(
                    Stage::Layout,
                    if matches!(error, foreign::InterfaceError::Missing) {
                        Cause::Missing
                    } else {
                        Cause::Native
                    },
                    error.call().result,
                );
                self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);

                return Err(S_OK);
            }
        };
        if !self.current(guard) {
            self.refuse(Stage::Layout, Cause::Stale, S_OK);
            return Err(S_OK);
        }
        let (call, actual) = tsf::substitute(&substitute, &profile);
        self.admission_call(guard, call, Stage::Layout)?;
        let layout = crate::keys::profile_layout(&profile, mode, actual);
        if layout.0.is_null() {
            self.refuse(
                Stage::Layout,
                if actual.0.is_null() {
                    Cause::Missing
                } else {
                    Cause::Substitute
                },
                S_OK,
            );
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        }
        let callback = self
            ._dependencies
            .as_ref()
            .and_then(|dependencies| dependencies.table.layouts);
        if self._dependencies.is_some() && callback.is_none() {
            self.refuse(Stage::Layout, Cause::Missing, S_OK);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        }
        let (call, count) = tsf::layouts(callback, &mut []);
        self.admission_call(guard, call, Stage::Layout)?;
        if !(1..=64).contains(&count) {
            self.refuse(Stage::Layout, Cause::Substitute, S_OK);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        }
        let mut loaded = [HKL::default(); 64];
        let (call, filled) = tsf::layouts(callback, &mut loaded);
        self.admission_call(guard, call, Stage::Layout)?;
        if filled != count {
            self.refuse(Stage::Layout, Cause::Substitute, S_OK);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        }
        let (call, after) = tsf::layouts(callback, &mut []);
        self.admission_call(guard, call, Stage::Layout)?;
        if after != count || !loaded[..count as usize].contains(&layout) {
            self.refuse(Stage::Layout, Cause::Substitute, S_OK);
            self.invalidate_lifecycle_if(guard, LifecycleCause::KeyAdmission);
            return Err(S_OK);
        }
        let host = self.state.borrow().host_identity;
        if host.kind == crate::observation::Kind::PackagedNotepad as u32
            && !crate::field::known_notepad(&host)
        {
            // SAFETY: the native key callback retains its receiving context through observation.
            unsafe { self.observe_host(owner.clone(), context) };
        }
        drop(substitute);
        drop(manager);
        if !self.current(guard) {
            self.refuse(Stage::Layout, Cause::Stale, S_OK);
            return Err(S_OK);
        }
        let target_layout = PROFILES
            .iter()
            .find(|entry| entry.mode != mode)
            .map(|entry| {
                let language = entry.language as usize;
                HKL((language << 16 | language) as *mut _)
            })
            .filter(|target| loaded[..count as usize].contains(target));
        Ok(AdmittedKey {
            guard,
            mode,
            keyboard,
            layout,
            target_layout,
        })
    }

    pub(crate) unsafe fn key_down(
        self: &Rc<Self>,
        owner: crate::com::OwnServiceLease,
        context: *mut c_void,
        key: windows::Win32::Foundation::WPARAM,
        flags: windows::Win32::Foundation::LPARAM,
        eaten: *mut windows_core::BOOL,
    ) -> HRESULT {
        self.checked(|| {
            #[cfg(feature = "test-api")]
            self.inject_panic(1);
            let delivery = self.begin_delivery();
            let decision = ReleaseDecision::new(self);
            self.count(Counter::KeyEntered);
            let admitted = match self.admit_key(context, None, &owner) {
                Ok(admitted) => admitted,
                Err(result) => return result,
            };
            let event = match crate::keys::translate(
                key,
                flags,
                admitted.layout,
                &admitted.keyboard,
                self.key_translation(),
            ) {
                Ok(crate::keys::Key::Event(event)) => event,
                _ => {
                    if matches!(key.0, 0x12 | 0xa4 | 0xa5) {
                        self.revoke_release();
                    }
                    return S_OK;
                }
            };
            if !self.current(admitted.guard)
                || self.state.borrow().mode != admitted.mode
                || event
                    .view()
                    .map_or(true, |event| event.mode != admitted.mode)
            {
                self.refuse(Stage::Mode, Cause::Stale, S_OK);
                self.invalidate_lifecycle_if(admitted.guard, LifecycleCause::KeyAdmission);
                return S_OK;
            }
            // SAFETY: inbound COM holds context and writable eaten through this synchronous call.
            let role = event.view().map_or(0, |event| event.role);
            let physical = crate::keys::PhysicalKey {
                key,
                flags,
                keyboard: admitted.keyboard,
            };
            let source_candidate = self.source_candidate(&admitted, &physical);
            if !self.current(admitted.guard) {
                return S_OK;
            }
            if matches!(role, 3..=5) {
                self.state.borrow_mut().source.cancel();
            }
            let ticket = if matches!(role, 4 | 5) {
                let facts = diagnostics::ModifierFacts::project(&admitted.keyboard);
                let intent = if role == 4 {
                    diagnostics::ManualIntent::Convert
                } else {
                    diagnostics::ManualIntent::Reverse
                };
                let ticket = self.begin_command(diagnostics::RecognizedCommand::Ordinary {
                    intent,
                    modifiers: facts,
                });
                self.command_progress(ticket, diagnostics::CommandStage::ExecutorDispatch(facts));
                ticket
            } else {
                diagnostics::CommandTicket::default()
            };
            let accepted = if matches!(role, 4 | 5) {
                // SAFETY: native callback retains context and the selected command for this request.
                unsafe { self.dispatch_command(owner, context, event, ticket, &decision) }
            } else {
                self.revoke_release();
                // SAFETY: native callback retains context through this synchronous execution.
                let mut execution =
                    unsafe { self.execute(owner.clone(), context, event, source_candidate) };
                if let Some(operation) = execution.source.take() {
                    // SAFETY: this original inbound callback still owns the exact physical delivery.
                    execution = unsafe {
                        self.finish_source_delivery(owner, context, &physical, &delivery, operation)
                    };
                }
                execution.eaten
            };
            if accepted {
                self.count(Counter::Consumed);
            }
            // SAFETY: the raw COM boundary checked and defaulted this output before admission.
            unsafe { eaten.write(windows_core::BOOL(accepted as i32)) };
            S_OK
        })
    }

    pub(crate) unsafe fn preserved(
        self: &Rc<Self>,
        owner: crate::com::OwnServiceLease,
        context: *mut c_void,
        id: windows_core::GUID,
        eaten: *mut windows_core::BOOL,
    ) -> HRESULT {
        self.checked(|| {
            self.source_input();
            let Some(binding) = BINDINGS.iter().find(|binding| binding.id == id) else {
                return S_OK;
            };
            if binding.role == 0 {
                self.revoke_release();
            }
            let decision = ReleaseDecision::new(self);
            if binding.role == 0 {
                // SAFETY: the SDK caller retains the context throughout the observer callback.
                unsafe { self.witness_alt(owner, context, &decision) };
                return S_OK;
            }
            self.state.borrow_mut().source.cancel();
            let intent = if binding.role == 4 {
                diagnostics::ManualIntent::Convert
            } else {
                diagnostics::ManualIntent::Reverse
            };
            let ticket = self.begin_command(diagnostics::RecognizedCommand::Preserved { intent });
            let mut modifiers = diagnostics::ModifierKnowledge::Unknown;
            let admitted = match self.admit_key(context, Some(&mut modifiers), &owner) {
                Ok(admitted) => admitted,
                Err(result) => {
                    self.revoke_release();
                    self.command_progress(
                        ticket,
                        if result.is_err() {
                            diagnostics::CommandStage::Exception(modifiers)
                        } else {
                            diagnostics::CommandStage::AdmissionRefused(modifiers)
                        },
                    );
                    return result;
                }
            };
            let facts = diagnostics::ModifierFacts::project(&admitted.keyboard);
            if !facts.exact() {
                self.revoke_release();
                self.command_progress(
                    ticket,
                    diagnostics::CommandStage::ExactModifiersRefused(facts),
                );
                return S_OK;
            }
            let Ok(event) = NativeEvent::new(binding.role, admitted.mode, 1, &[]) else {
                return E_FAIL;
            };
            if !self.current(admitted.guard) || self.state.borrow().mode != admitted.mode {
                self.command_progress(ticket, diagnostics::CommandStage::DispatchRefused(facts));
                self.refuse(Stage::Mode, Cause::Stale, S_OK);
                self.invalidate_lifecycle_if(admitted.guard, LifecycleCause::KeyAdmission);
                return S_OK;
            }
            self.command_progress(ticket, diagnostics::CommandStage::ExecutorDispatch(facts));
            // SAFETY: the inbound preserved-key callback retains context/output for this request.
            let accepted =
                unsafe { self.dispatch_command(owner, context, event, ticket, &decision) };
            // SAFETY: raw COM checked and defaulted this writable slot before calling here.
            unsafe { eaten.write(windows_core::BOOL(accepted as i32)) };
            S_OK
        })
    }

    pub(crate) unsafe fn execute(
        self: &Rc<Self>,
        owner: crate::com::OwnServiceLease,
        context: *mut c_void,
        event: NativeEvent,
        source_candidate: bool,
    ) -> Execution {
        use crate::edit::{Effect, Frame};
        self.count(Counter::HandleEntered);
        let authority = {
            let state = self.state.borrow();
            let cause = if state.activation != Activation::Active {
                Some(Cause::Inactive)
            } else if state.stopped {
                Some(Cause::Stopped)
            } else if state.application_off {
                Some(Cause::Predicate)
            } else if context.is_null() {
                Some(Cause::Missing)
            } else if state.busy {
                Some(Cause::Reentry)
            } else {
                None
            };
            drop(state);
            if let Some(cause) = cause {
                self.refuse(Stage::Handle, cause, S_OK);
                if cause == Cause::Reentry {
                    self.invalidate_word(self.owned().is_some());
                }
                return Execution::default();
            }
            self.capture_authority()
        };
        let Some(authority) = authority else {
            return Execution::default();
        };
        let event = {
            let state = self.state.borrow();
            if state.pending_restricts() {
                if authority.owned.is_none() {
                    return Execution::default();
                }
                let Ok(finish) = NativeEvent::new(6, state.mode, 1, &[]) else {
                    return Execution::default();
                };
                finish
            } else {
                event
            }
        };
        let guard = authority.guard;
        self.state.borrow_mut().busy = true;
        let _busy = ExecutionCall(self);
        // SAFETY: the inbound transport retains context throughout capture and the synchronous request.
        let frame =
            match unsafe { Frame::editing(Rc::clone(self), owner, context, event, authority) } {
                Ok(frame) => frame,
                Err(call) => {
                    self.refuse(Stage::Handle, Cause::Native, call.result);

                    return Execution::default();
                }
            };
        if !frame.alive() {
            self.refuse(Stage::Session, Cause::Stale, S_OK);
            return Execution::default();
        }
        frame.allow_source_handoff(source_candidate);
        self.count(Counter::Requested);
        {
            let mut state = self.state.borrow_mut();
            state.diagnostics.request_present = 0;
            state.diagnostics.session_present = 0;
            state.diagnostics.request_result = 0;
            state.diagnostics.session_result = 0;
        }
        let result = frame.request_editing();
        {
            let mut state = self.state.borrow_mut();
            state.diagnostics.request_present = 1;
            state.diagnostics.session_present = 1;
            state.diagnostics.request_result = result.outer.result.0 as u32;
            state.diagnostics.session_result = result.callback.0 as u32;
        }
        if result.outer.result.is_ok()
            && result.callback.is_ok()
            && result.entered
            && result.completed
        {
            Execution {
                eaten: frame.eaten(),
                source: if result.outer.exact()
                    && result.callback == S_OK
                    && result.valid
                    && result.effect == Effect::Verified
                    && self.current(guard)
                    && self.owned().is_none()
                {
                    frame.take_source_handoff()
                } else {
                    None
                },
                completion: if (result.outer.exact() && result.callback == S_OK && result.valid)
                    || frame.completion() == crate::edit::Completion::Unknown
                {
                    frame.completion()
                } else {
                    crate::edit::Completion::Refused
                },
            }
        } else {
            if result.outer.result.is_err() {
                self.refuse(Stage::Request, Cause::Native, result.outer.result);
            } else if result.callback.is_err() {
                self.refuse(Stage::Session, Cause::Native, result.callback);
            } else {
                self.refuse(Stage::Session, Cause::Predicate, S_OK);
            }
            let eaten = frame.unknown_disposition();
            self.invalidate_if(
                guard,
                result.effect != Effect::Unattempted || self.owned().is_some(),
            );
            Execution {
                eaten,
                source: None,
                completion: if result.effect != Effect::Unattempted {
                    crate::edit::Completion::Unknown
                } else {
                    crate::edit::Completion::Refused
                },
            }
        }
    }

    fn state_current(&self, state: &State, guard: Guard) -> bool {
        !self.poisoned.get()
            && !state.stopped
            && state.activation == Activation::Active
            && state.epoch == guard.epoch
            && state.generation == guard.generation
    }

    pub(crate) fn capture_authority(&self) -> Option<Authority> {
        if !self.owner_thread() {
            return None;
        }
        let state = self.state.borrow();
        let guard = Guard {
            epoch: state.epoch,
            generation: state.generation,
        };
        if !self.state_current(&state, guard) {
            return None;
        }
        let resources = state.resources.as_ref()?;
        Some(Authority {
            manager: resources.manager.as_ref()?.clone(),
            client: resources.client,
            guard,
            owned: state.owned.clone(),
            host_identity: state.host_identity,
        })
    }

    pub(crate) fn current_owned(&self, guard: Guard, expected: Option<&Rc<OwnedIdentity>>) -> bool {
        if !self.owner_thread() {
            return false;
        }
        let state = self.state.borrow();
        self.state_current(&state, guard)
            && match (expected, state.owned.as_ref()) {
                (None, None) => true,
                (Some(expected), Some(owned)) => Rc::ptr_eq(expected, &owned.identity),
                _ => false,
            }
    }

    pub(crate) fn owned(&self) -> Option<Rc<Owned>> {
        self.state.borrow().owned.clone()
    }
    pub(crate) fn count(&self, counter: Counter) {
        diagnostics::count(&mut self.state.borrow_mut().diagnostics, counter);
    }
    pub(crate) fn refuse(&self, stage: Stage, cause: Cause, result: HRESULT) {
        diagnostics::refuse(
            &mut self.state.borrow_mut().diagnostics,
            stage,
            cause,
            result,
        );
    }
    pub(crate) fn refusal_counts(&self) -> [u32; 26] {
        self.state.borrow().diagnostics.refusals
    }

    pub(crate) fn invalidate_if(&self, guard: Guard, stop: bool) {
        if self.state.borrow().epoch == guard.epoch {
            self.invalidate_word(stop);
        }
    }

    fn invalidate_lifecycle_if(&self, guard: Guard, cause: LifecycleCause) {
        if self.state.borrow().epoch == guard.epoch {
            self.invalidate_for_lifecycle(cause);
        }
    }

    pub(crate) fn edit_callback(&self, guard: Guard, call: impl FnOnce() -> HRESULT) -> HRESULT {
        if !self.owner_thread() {
            return RPC_E_WRONG_THREAD;
        }
        match catch_unwind(AssertUnwindSafe(|| self.callback_scope(call))) {
            Ok(result) => result,
            Err(payload) => {
                std::mem::forget(payload);
                if self.state.borrow().epoch == guard.epoch {
                    self.poisoned.set(true);
                    self.invalidate_if(guard, true);
                }
                E_FAIL
            }
        }
    }

    pub(crate) fn invalidate_word(&self, stop: bool) {
        self.state.borrow_mut().stopped |= stop;
        self.invalidate();
    }

    pub(crate) fn publish_owned(&self, guard: Guard, mut owned: Owned) -> Option<Rc<Owned>> {
        let mut state = self.state.borrow_mut();
        if !self.state_current(&state, guard) || state.owned.is_some() {
            return None;
        }
        owned.serial = state.lifecycle.acquire();
        let owned = Rc::new(owned);
        state.owned = Some(Rc::clone(&owned));
        Some(owned)
    }

    pub(crate) fn verify_owned(
        &self,
        guard: Guard,
        identity: &Rc<OwnedIdentity>,
        text: &[u16],
    ) -> bool {
        let previous = self.owned();
        let Some(previous) = previous else {
            return false;
        };
        let mut replacement = previous.as_ref().clone();
        replacement.verified = text.to_vec();
        let replacement = Rc::new(replacement);
        let retired = {
            let mut state = self.state.borrow_mut();
            if !self.state_current(&state, guard)
                || !state
                    .owned
                    .as_ref()
                    .is_some_and(|current| Rc::ptr_eq(identity, &current.identity))
            {
                return false;
            }
            state.owned.replace(replacement)
        };
        drop(retired);
        self.current_owned(guard, Some(identity))
    }

    pub(crate) fn take_owned(
        &self,
        guard: Guard,
        identity: &Rc<OwnedIdentity>,
    ) -> Option<Rc<Owned>> {
        let mut state = self.state.borrow_mut();
        if !self.state_current(&state, guard)
            || !state
                .owned
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(identity, &current.identity))
        {
            return None;
        }
        state.owned.take()
    }

    pub(crate) fn lifecycle(
        &self,
        kind: LifecycleKind,
        serial: u32,
        cause: LifecycleCause,
        unresolved: Unresolved,
    ) {
        self.state
            .borrow_mut()
            .lifecycle
            .record(kind, serial, cause, unresolved);
    }

    pub(crate) fn propose(&self, guard: Guard, event: &NativeEvent) -> Result<Plan, u32> {
        let mut state = self.state.borrow_mut();
        if !self.state_current(&state, guard) {
            return Err(5);
        }
        state.engine.as_mut().ok_or(5u32)?.plan(event)
    }

    pub(crate) fn acknowledge(&self, guard: Guard, id: u64, outcome: u32) -> Result<(), u32> {
        let mut state = self.state.borrow_mut();
        if !self.state_current(&state, guard) {
            return Err(5);
        }
        state.engine.as_mut().ok_or(5u32)?.commit(id, outcome)?;
        if outcome == 0
            && state.owned.is_none()
            && !state.pending_restricts()
            && !state.source.defer_adoption()
            && let Some(ready) = state.pending_ready.clone().filter(|ready| ready.model)
        {
            Self::adopt_ready(&mut state, ready)?;
        }
        Ok(())
    }

    pub(crate) fn capture_field(
        &self,
        window: windows::Win32::Foundation::HWND,
        host: HostIdentity,
    ) -> foreign::Returned<crate::field_contract::Facts> {
        if let Some(dependencies) = &self._dependencies
            && dependencies.table.capture.is_some()
        {
            return foreign::document::capture(&dependencies.table, window, &host);
        }
        foreign::Returned {
            call: Call { result: S_OK },
            value: crate::field::capture(window, host),
        }
    }

    unsafe fn observe_host(
        self: &Rc<Self>,
        owner: crate::com::OwnServiceLease,
        context: *mut c_void,
    ) {
        let Some(authority) = self.capture_authority() else {
            return;
        };
        let guard = authority.guard;
        let manager = authority.manager.clone();
        let client = authority.client;
        drop(authority);
        {
            let mut state = self.state.borrow_mut();
            if context.is_null()
                || state.observing_host
                || state.observed_generation == guard.generation
                || !self.state_current(&state, guard)
            {
                return;
            }
            state.observed_generation = guard.generation;
            state.observing_host = true;
        }
        let _observation = HostObservationCall(self);
        // SAFETY: the caller retains its live receiving context throughout this synchronous observation.
        let context = match unsafe { foreign::retain::<ITfContext>(context) } {
            Ok(context) => context,
            Err(call) => {
                if self.state.borrow().epoch == guard.epoch {
                    self.state.borrow_mut().metadata = crate::observation::Metadata {
                        reason: crate::observation::Reason::Native,
                        result: call.result.0 as u32,
                        ..crate::observation::Metadata::default()
                    };
                }
                return;
            }
        };
        let service = Rc::downgrade(self);
        let current = Rc::new(move || {
            service
                .upgrade()
                .is_some_and(|service| service.owner_thread() && service.current(guard))
        });
        let observed = crate::host_metadata::observe(owner, context, manager, client, current);
        if self.state.borrow().epoch != guard.epoch {
            return;
        }
        if observed.panicked {
            self.poisoned.set(true);
            self.invalidate_if(guard, true);
        }
        let metadata = if self.current(guard) || observed.panicked {
            observed.metadata
        } else {
            crate::observation::Metadata {
                reason: crate::observation::Reason::Stale,
                ..crate::observation::Metadata::default()
            }
        };
        self.state.borrow_mut().metadata = metadata;
    }

    fn observation_stamp(state: &State) -> diagnostics::ObservationStamp {
        diagnostics::ObservationStamp {
            after: diagnostics::LifecycleCursor {
                value: state.lifecycle.sequence(),
            },
            owned: diagnostics::ObservedOwnership {
                present: state.owned.is_some(),
                composition_serial: state.owned.as_ref().map_or(0, |owned| owned.serial),
            },
        }
    }

    fn begin_command(
        &self,
        recognized: diagnostics::RecognizedCommand,
    ) -> diagnostics::CommandTicket {
        let mut state = self.state.borrow_mut();
        let stamp = Self::observation_stamp(&state);
        state.observations.begin(recognized, stamp)
    }

    fn command_progress(
        &self,
        ticket: diagnostics::CommandTicket,
        stage: diagnostics::CommandStage,
    ) {
        let mut state = self.state.borrow_mut();
        let stamp = Self::observation_stamp(&state);
        state.observations.record(ticket, stage, stamp);
    }

    fn command_complete(
        &self,
        ticket: diagnostics::CommandTicket,
        completion: crate::edit::Completion,
        witnessed: bool,
    ) {
        let Some(command) = ticket.identity else {
            return;
        };
        let mut state = self.state.borrow_mut();
        let stamp = Self::observation_stamp(&state);
        let completion = match completion {
            crate::edit::Completion::Refused => diagnostics::CommandCompletion::Refused,
            crate::edit::Completion::Unknown => diagnostics::CommandCompletion::Unknown,
            crate::edit::Completion::Reserved => diagnostics::CommandCompletion::Reserved,
            crate::edit::Completion::ManualMark => diagnostics::CommandCompletion::ManualMark,
        };
        state.observations.complete(
            diagnostics::CompletionObservation {
                command,
                witnessed,
                completion,
            },
            stamp,
        );
    }

    fn publish_observation(&self) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let state = self.state.borrow();
            let Some(publisher) = state.publisher.clone() else {
                return;
            };
            if publisher.instance() == 0 {
                return;
            }
            let mut base = state.diagnostics;
            base.active = (state.activation == Activation::Active) as u32;
            base.stopped = (state.stopped || self.poisoned.get()) as u32;
            base.mode = state.mode;
            let complete = SnapshotV2 {
                base,
                lifecycle: state.lifecycle.copy(
                    state.owned.is_some(),
                    state.owned.as_ref().map_or(0, |owned| owned.serial),
                ),
            };
            let payload = crate::observation::encode(
                &state.host_identity,
                &state.metadata,
                &complete,
                &state.runtime,
                publisher.instance(),
                &state.observations.copy(),
            );
            drop(state);
            publisher.publish(&payload);
        }));
        if let Err(payload) = result {
            std::mem::forget(payload);
        }
    }

    pub(crate) fn copy_snapshot(&self) -> Result<Snapshot, HRESULT> {
        let state = self.state.borrow();
        if state.diagnostic_depth != 0 {
            return Err(HRESULT(0x8000000Au32 as i32));
        }
        let mut snapshot = state.diagnostics;
        snapshot.active = (state.activation == Activation::Active) as u32;
        snapshot.stopped = (state.stopped || self.poisoned.get()) as u32;
        snapshot.mode = state.mode;
        Ok(snapshot)
    }

    pub(crate) fn copy_snapshot_v2(&self) -> Result<SnapshotV2, HRESULT> {
        let base = self.copy_snapshot()?;
        let state = self.state.borrow();
        let lifecycle = state.lifecycle.copy(
            state.owned.is_some(),
            state.owned.as_ref().map_or(0, |owned| owned.serial),
        );
        Ok(SnapshotV2 { base, lifecycle })
    }

    pub(crate) fn copy_runtime(&self) -> Result<RuntimeSnapshot, HRESULT> {
        let state = self.state.borrow();
        if state.diagnostic_depth != 0 {
            return Err(HRESULT(0x8000000Au32 as i32));
        }
        Ok(state.runtime)
    }

    pub(crate) unsafe fn interested(&self, context: *mut c_void) -> bool {
        self.count(Counter::TestEntered);
        let mut state = self.state.borrow_mut();
        let cause = if state.activation != Activation::Active {
            Some(Cause::Inactive)
        } else if state.stopped {
            Some(Cause::Stopped)
        } else if state.application_off || state.mode == 0 {
            Some(Cause::ModeUnset)
        } else if state.busy || state.admitting_key || state.observing_host {
            Some(Cause::Reentry)
        } else if context.is_null() {
            Some(Cause::Missing)
        } else {
            None
        };
        if let Some(cause) = cause {
            drop(state);
            self.refuse(Stage::Test, cause, S_OK);
            return false;
        }
        let host = state.host_identity;
        let guard = Guard {
            epoch: state.epoch,
            generation: state.generation,
        };
        state.admitting_key = true;
        drop(state);
        let _admission = AdmissionCall(self);

        // A positive test can suppress native typing even if OnKeyDown later declines.
        // Resolve the receiving view without requesting an edit session or reading text.
        let window = (|| {
            // SAFETY: TSF retains the receiving context through OnTestKeyDown.
            let context = unsafe { foreign::retain::<ITfContext>(context) }.ok()?;
            if !self.current(guard) {
                return None;
            }
            let view = foreign::document::view(&context);
            if !view.call.exact() || !self.current(guard) {
                return None;
            }
            let window = foreign::document::window(&view.value?);
            (window.call.exact() && !window.value.is_invalid()).then_some(window.value)
        })();
        if let Some(window) = window
            && self.current(guard)
        {
            let facts = self.capture_field(window, host);
            if facts.call.exact()
                && crate::field::classify(&facts.value).is_some()
                && self.current(guard)
                && !self.state.borrow().application_off
            {
                self.count(Counter::Interested);
                return true;
            }
        }
        self.refuse(Stage::Test, Cause::Predicate, S_OK);
        false
    }

    #[cfg(feature = "test-api")]
    pub(crate) fn arm_panic(&self, at: u32) {
        self.panic_at.set(at);
    }

    #[cfg(feature = "test-api")]
    pub(crate) fn inject_panic(&self, at: u32) {
        if self.panic_at.get() == at {
            self.panic_at.set(0);
            panic!("injected adapter boundary panic");
        }
    }

    #[cfg(feature = "test-api")]
    pub(crate) fn stopped(&self) -> bool {
        self.poisoned.get() || self.state.borrow().stopped
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        // Generated IUnknown::Release invokes this destructor. No panic may leave it.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            #[cfg(feature = "test-api")]
            self.inject_panic(3);
            self.invalidate();
        }));
    }
}
