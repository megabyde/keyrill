use super::*;
use crate::edit::Frame;
use crate::field::Identity;
use crate::vocabulary_contract::DictionaryOwner;
use std::sync::{Arc, atomic::AtomicBool};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;

#[derive(Clone)]
struct Configuration {
    activation: u32,
    prepared: crate::vocabulary_contract::Prepared,
    dictionaries: [Option<Arc<DictionaryOwner>>; 2],
}

impl Configuration {
    fn capture(ready: &Ready) -> Option<Self> {
        let prepared = ready.configuration.as_ref()?;
        (ready.model && prepared.policy() == 2 && prepared.settings.value.switch_source).then(
            || Self {
                activation: ready.activation,
                prepared: prepared.clone(),
                dictionaries: std::array::from_fn(|index| ready.sources[index].dictionary.clone()),
            },
        )
    }

    fn matches(&self, ready: &Ready) -> bool {
        self.activation == ready.activation
            && ready.configuration.as_ref() == Some(&self.prepared)
            && self
                .dictionaries
                .iter()
                .zip(&ready.sources)
                .all(|(old, new)| match (old, &new.dictionary) {
                    (Some(old), Some(new)) => Arc::ptr_eq(old, new),
                    (None, None) => true,
                    _ => false,
                })
    }
}

pub(super) struct Intent {
    guard: Guard,
    owned: Rc<OwnedIdentity>,
    serial: u32,
    configuration: Configuration,
    from: u32,
    target: u32,
}

struct Finished {
    epoch: u64,
    field: Identity,
    configuration: Configuration,
    from: u32,
    target: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Finished,
    Preparing,
    Language,
    Activate,
    Readback,
}

pub(crate) struct Operation {
    finished: Finished,
    cancelled: Rc<Cell<bool>>,
    phase: Cell<Phase>,
    observations: Cell<u8>,
}

pub(super) enum SourceState {
    Idle(Outcome),
    Retained(Intent),
    Changing(Rc<Operation>),
}

#[derive(Clone, Copy, Default)]
pub(super) enum Outcome {
    #[default]
    None,
    Confirmed,
    Cancelled,
    Failed,
}

impl Default for SourceState {
    fn default() -> Self {
        Self::Idle(Outcome::None)
    }
}

impl SourceState {
    pub(super) fn cancel(&mut self) {
        match self {
            Self::Changing(operation) => operation.cancelled.set(true),
            Self::Idle(
                Outcome::None | Outcome::Confirmed | Outcome::Cancelled | Outcome::Failed,
            ) => return,
            Self::Retained(_) => {}
        }
        *self = Self::Idle(Outcome::Cancelled);
    }

    pub(super) fn defer_adoption(&self) -> bool {
        !matches!(self, Self::Idle(_))
    }

    pub(super) fn accepts_ready(&self, ready: &Ready) -> bool {
        match self {
            Self::Idle(_) => true,
            Self::Retained(intent) => intent.configuration.matches(ready),
            Self::Changing(operation) => operation.finished.configuration.matches(ready),
        }
    }

    #[cfg(feature = "test-api")]
    pub(super) fn status(&self) -> u32 {
        match self {
            Self::Idle(Outcome::None) => 0,
            Self::Retained(_) => 1,
            Self::Changing(_) => 2,
            Self::Idle(Outcome::Confirmed) => 3,
            Self::Idle(Outcome::Cancelled) => 4,
            Self::Idle(Outcome::Failed) => 5,
        }
    }

    fn observed(&mut self, observed: &TF_INPUTPROCESSORPROFILE) {
        let Self::Changing(operation) = self else {
            self.cancel();
            return;
        };
        let from = profile_for(operation.finished.from);
        let target = profile_for(operation.finished.target);
        let previous = operation.observations.get();
        let processor = observed.dwProfileType == TF_PROFILETYPE_INPUTPROCESSOR
            && observed.clsid == SERVICE_ID
            && observed.catid == GUID_TFCAT_TIP_KEYBOARD;
        let next = if matches!(operation.phase.get(), Phase::Language | Phase::Activate)
            && processor
            && observed.langid == from.language
            && observed.guidProfile == from.id
            && observed.dwFlags & TF_IPP_FLAG_ACTIVE == 0
            && previous == 0
        {
            Some(1)
        } else if matches!(operation.phase.get(), Phase::Language | Phase::Activate)
            && processor
            && observed.langid == target.language
            && observed.guidProfile == target.id
            && observed.dwFlags & TF_IPP_FLAG_ACTIVE != 0
            && previous <= 1
        {
            Some(2)
        } else {
            None
        };
        if let Some(next) = next {
            operation.observations.set(next);
        } else {
            self.cancel();
        }
    }
}

fn profile_for(mode: u32) -> &'static crate::profiles::Profile {
    &PROFILES[(mode == 2) as usize]
}

pub(super) fn veto(state: &State) -> bool {
    state
        .pending_ready
        .as_ref()
        .and_then(|ready| ready.configuration.as_ref())
        .is_some_and(|prepared| prepared.policy() != 2 || !prepared.settings.value.switch_source)
}

pub(super) struct Delivery<'service> {
    service: &'service Service,
    cancelled: Rc<Cell<bool>>,
}

impl Drop for Delivery<'_> {
    fn drop(&mut self) {
        let mut state = self.service.state.borrow_mut();
        if state
            .delivery
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, &self.cancelled))
        {
            state.delivery = None;
        }
        if let SourceState::Changing(operation) = &state.source
            && Rc::ptr_eq(&operation.cancelled, &self.cancelled)
        {
            state.source.cancel();
        }
    }
}

struct SourceCall<'service> {
    service: &'service Rc<Service>,
    operation: Rc<Operation>,
}

impl Drop for SourceCall<'_> {
    fn drop(&mut self) {
        let mut state = self.service.state.borrow_mut();
        if let SourceState::Changing(current) = &state.source
            && Rc::ptr_eq(current, &self.operation)
        {
            let outcome = if self.operation.cancelled.get() {
                Outcome::Cancelled
            } else {
                Outcome::Failed
            };
            self.operation.cancelled.set(true);
            state.source = SourceState::Idle(outcome);
        }
        drop(state);
        let _ = self.service.maintain();
    }
}

impl Service {
    #[cfg(feature = "test-api")]
    pub(crate) fn source_status(&self) -> u32 {
        self.state.borrow().source.status()
    }

    pub(super) fn begin_delivery(&self) -> Delivery<'_> {
        self.source_input();
        let cancelled = Rc::new(Cell::new(false));
        self.state.borrow_mut().delivery = Some(Rc::clone(&cancelled));
        Delivery {
            service: self,
            cancelled,
        }
    }

    pub(crate) fn source_input(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(delivery) = &state.delivery {
            delivery.set(true);
        }
        if matches!(state.source, SourceState::Changing(_)) {
            state.source.cancel();
        }
    }

    pub(super) fn source_profile_observed(&self, observed: &TF_INPUTPROCESSORPROFILE) {
        self.state.borrow_mut().source.observed(observed);
    }

    pub(crate) fn verified_source_edit(
        &self,
        guard: Guard,
        event: &NativeEvent,
        plan: &Plan,
        field: Identity,
        candidate: bool,
    ) -> Option<Rc<Operation>> {
        let view = event.view().ok()?;
        let mut state = self.state.borrow_mut();
        if !self.state_current(&state, guard) || veto(&state) {
            state.source.cancel();
            return None;
        }
        if plan.action == 2 {
            let SourceState::Retained(intent) = &state.source else {
                return None;
            };
            let finished = (candidate
                && plan.disposition == 2
                && state.owned.is_none()
                && intent.guard.epoch == guard.epoch
                && intent.guard.generation == guard.generation
                && state
                    .applied_ready
                    .as_ref()
                    .is_some_and(|ready| intent.configuration.matches(ready)))
            .then(|| Finished {
                epoch: guard.epoch,
                field,
                configuration: intent.configuration.clone(),
                from: intent.from,
                target: intent.target,
            });
            state.source = SourceState::Idle(Outcome::Cancelled);
            let finished = finished?;
            let cancelled = state.delivery.as_ref()?.clone();
            if cancelled.get() {
                return None;
            }
            let operation = Rc::new(Operation {
                finished,
                cancelled,
                phase: Cell::new(Phase::Finished),
                observations: Cell::new(0),
            });
            state.source = SourceState::Changing(Rc::clone(&operation));
            return Some(operation);
        }
        if plan.action == 1
            && view.role == 1
            && view.units == [b' ' as u16]
            && plan.mark.last() == Some(&(b' ' as u16))
            && plan.mark[..plan.mark.len() - 1] != plan.expected
        {
            let configuration = state
                .applied_ready
                .as_ref()
                .and_then(Configuration::capture);
            if let (Some(configuration), Some(owned)) = (configuration, &state.owned) {
                state.source = SourceState::Retained(Intent {
                    guard,
                    owned: Rc::clone(&owned.identity),
                    serial: owned.serial,
                    configuration,
                    from: view.mode,
                    target: if view.mode == 1 { 2 } else { 1 },
                });
            }
        } else if matches!(state.source, SourceState::Retained(_)) {
            state.source.cancel();
        }
        None
    }

    pub(super) fn source_candidate(
        &self,
        admitted: &AdmittedKey,
        physical: &crate::keys::PhysicalKey,
    ) -> bool {
        let state = self.state.borrow();
        let SourceState::Retained(intent) = &state.source else {
            return false;
        };
        if veto(&state)
            || intent.from != admitted.mode
            || !self.state_current(&state, intent.guard)
            || !state.owned.as_ref().is_some_and(|owned| {
                Rc::ptr_eq(&intent.owned, &owned.identity) && owned.serial == intent.serial
            })
        {
            return false;
        }
        let Some(layout) = admitted.target_layout else {
            return false;
        };
        drop(state);
        crate::keys::handoff_letter(physical, layout, &admitted.keyboard, self.key_translation())
            .is_some()
            && self.current(admitted.guard)
    }

    fn source_current(&self, operation: &Rc<Operation>) -> bool {
        let mut state = self.state.borrow_mut();
        let latest = state.worker.as_mut().and_then(vocabulary::Worker::poll);
        if let Some(latest) = latest
            && latest.activation == state.runtime.activation
            && latest.revision > state.runtime.revision
        {
            state.pending_ready = Some(latest);
        }
        let valid = !self.poisoned.get()
            && !operation.cancelled.get()
            && !state.stopped
            && state.activation == Activation::Active
            && state.epoch == operation.finished.epoch
            && state.owned.is_none()
            && !veto(&state)
            && state
                .pending_ready
                .as_ref()
                .is_none_or(|ready| operation.finished.configuration.matches(ready))
            && state
                .applied_ready
                .as_ref()
                .is_some_and(|ready| operation.finished.configuration.matches(ready))
            && matches!(&state.source, SourceState::Changing(current) if Rc::ptr_eq(current, operation));
        if !valid {
            if matches!(&state.source, SourceState::Changing(current) if Rc::ptr_eq(current, operation))
            {
                state.source.cancel();
            }
            operation.cancelled.set(true);
        }
        valid
    }

    pub(super) unsafe fn finish_source_delivery(
        self: &Rc<Self>,
        owner: crate::com::OwnServiceLease,
        context: *mut c_void,
        physical: &crate::keys::PhysicalKey,
        delivery: &Delivery<'_>,
        operation: Rc<Operation>,
    ) -> Execution {
        let original = &physical.keyboard;
        if delivery.cancelled.get()
            || operation.phase.get() != Phase::Finished
            || !Rc::ptr_eq(&operation.cancelled, &delivery.cancelled)
            || !self.source_current(&operation)
        {
            return Execution::default();
        }
        operation.phase.set(Phase::Preparing);
        let _source = SourceCall {
            service: self,
            operation: Rc::clone(&operation),
        };
        let resources = self.state.borrow().resources.clone();
        let Some(resources) = resources else {
            return Execution::default();
        };
        // SAFETY: the original native key callback retains its exact receiving context.
        let Ok(context_lease) = (unsafe { foreign::retain::<ITfContext>(context) }) else {
            return Execution::default();
        };
        let Some(authority) = self.capture_authority() else {
            return Execution::default();
        };
        let manager = authority.manager.clone();
        drop(authority);
        let revoked = Arc::new(AtomicBool::new(false));
        let mut watch = crate::admission_watch::Watch::new(Arc::clone(&revoked));
        if !watch.install(&context_lease, &manager) {
            return Execution::default();
        }
        let current = || {
            if revoked.load(Ordering::Acquire) {
                operation.cancelled.set(true);
                return false;
            }
            self.source_current(&operation)
        };
        let eligible = || {
            if !current() {
                return false;
            }
            // SAFETY: this metadata-only check remains within the original native callback.
            let field = unsafe { Frame::source_field(Rc::clone(self), owner.clone(), context) };
            if field != Some(operation.finished.field) || !current() {
                return false;
            }
            let mut keyboard = [0; 256];
            if !foreign::keyboard_state(&mut keyboard) || &keyboard != original {
                operation.cancelled.set(true);
                return false;
            }
            current()
        };
        if !eligible() {
            return Execution::default();
        }
        let profiles = resources.profiles.clone();
        let Some(profiles) = profiles else {
            return Execution::default();
        };
        let Ok(languages) = profiles.query::<ITfInputProcessorProfiles>() else {
            return Execution::default();
        };
        if !eligible() {
            return Execution::default();
        }
        let Ok(substitute) = profiles.query::<ITfInputProcessorProfileSubstituteLayout>() else {
            return Execution::default();
        };
        if !eligible() {
            return Execution::default();
        }
        let from = profile_for(operation.finished.from);
        let target = profile_for(operation.finished.target);
        let (call, available) = tsf::target_profile(&profiles, target);
        if !call.exact()
            || !eligible()
            || available.clsid != SERVICE_ID
            || available.guidProfile != target.id
            || available.langid != target.language
            || available.dwProfileType != TF_PROFILETYPE_INPUTPROCESSOR
            || available.catid != GUID_TFCAT_TIP_KEYBOARD
        {
            return Execution::default();
        }
        let (call, target_layout) = tsf::substitute(&substitute, &available);
        let language = target.language as usize;
        let standard = HKL((language << 16 | language) as *mut _);
        if !call.exact()
            || target_layout != standard
            || available.hklSubstitute != target_layout
            || !eligible()
        {
            return Execution::default();
        }
        let callback = self
            ._dependencies
            .as_ref()
            .and_then(|dependencies| dependencies.table.layouts);
        if self._dependencies.is_some() && callback.is_none() {
            return Execution::default();
        }
        let (call, count) = tsf::layouts(callback, &mut []);
        if !call.exact() || !(1..=64).contains(&count) || !eligible() {
            return Execution::default();
        }
        let mut loaded = [HKL::default(); 64];
        let (call, filled) = tsf::layouts(callback, &mut loaded);
        if !call.exact() || filled != count || !eligible() {
            return Execution::default();
        }
        let (call, after) = tsf::layouts(callback, &mut []);
        if !call.exact()
            || after != count
            || !loaded[..count as usize].contains(&target_layout)
            || !eligible()
        {
            return Execution::default();
        }
        let (call, enabled) = tsf::profile_enabled(&languages, target);
        if !call.exact() || !enabled.as_bool() || !eligible() {
            return Execution::default();
        }
        let (call, language) = tsf::current_language(&languages);
        if !call.exact() || language != from.language || !eligible() {
            return Execution::default();
        }
        let (call, active) = tsf::active_profile(&profiles);
        if !call.exact()
            || !crate::keys::active_profile(&active, from.mode)
            || active.catid != GUID_TFCAT_TIP_KEYBOARD
            || !eligible()
        {
            return Execution::default();
        }
        let (call, layout) = tsf::substitute(&substitute, &active);
        if !call.exact()
            || crate::keys::profile_layout(&active, from.mode, layout)
                .0
                .is_null()
            || !eligible()
        {
            return Execution::default();
        }
        let mut fresh = [0; 256];
        if !foreign::keyboard_state(&mut fresh) || &fresh != original || !eligible() {
            return Execution::default();
        }
        operation.phase.set(Phase::Language);
        let call = tsf::change_language(&languages, target.language);
        operation.phase.set(Phase::Readback);
        if !call.exact() || !eligible() {
            return Execution::default();
        }
        let (call, language) = tsf::current_language(&languages);
        if !call.exact() || language != target.language || !eligible() {
            return Execution::default();
        }
        let (call, active) = tsf::active_profile(&profiles);
        if !call.exact() || !eligible() {
            return Execution::default();
        }
        if !crate::keys::active_profile(&active, target.mode)
            || active.catid != GUID_TFCAT_TIP_KEYBOARD
        {
            operation.phase.set(Phase::Activate);
            let call = tsf::activate_profile(&profiles, target);
            operation.phase.set(Phase::Readback);
            if !call.exact() || !eligible() {
                return Execution::default();
            }
        }
        let (call, language) = tsf::current_language(&languages);
        if !call.exact() || language != target.language || !eligible() {
            return Execution::default();
        }
        let (call, active) = tsf::active_profile(&profiles);
        if !call.exact()
            || !crate::keys::active_profile(&active, target.mode)
            || active.catid != GUID_TFCAT_TIP_KEYBOARD
            || !eligible()
        {
            return Execution::default();
        }
        let (call, layout) = tsf::substitute(&substitute, &active);
        if !call.exact()
            || crate::keys::profile_layout(&active, target.mode, layout)
                .0
                .is_null()
            || !eligible()
        {
            return Execution::default();
        }
        drop(substitute);
        drop(languages);
        drop(profiles);
        drop(manager);
        watch.drain();
        if !eligible() {
            return Execution::default();
        }
        self.invalidate_generation();
        self.state.borrow_mut().mode = target.mode;
        self.state.borrow_mut().source = SourceState::Idle(Outcome::Confirmed);
        let admitted = match self.admit_key(context, None, &owner) {
            Ok(admitted) => admitted,
            Err(_) => return Execution::default(),
        };
        if operation.cancelled.get() || admitted.mode != target.mode {
            return Execution::default();
        }
        let Some(event) = crate::keys::handoff_letter(
            physical,
            admitted.layout,
            &admitted.keyboard,
            self.key_translation(),
        ) else {
            return Execution::default();
        };
        if !self.current(admitted.guard) || operation.cancelled.get() {
            return Execution::default();
        }
        // SAFETY: the live original callback still owns this key and context through its one continuation.
        unsafe { self.execute(owner, context, event, false) }
    }
}
