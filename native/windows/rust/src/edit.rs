use crate::com::OwnServiceLease;
use crate::diagnostics::{Cause, Counter, LifecycleCause, LifecycleKind, Stage, Unresolved};
use crate::engine::{Action, Disposition, Plan};
use crate::field::{self, CandidateField, Contract, Identity};
use crate::foreign::document as native;
use crate::foreign::{self, Call, InterfaceError, Lease, Returned};
use crate::service::{Authority, Owned, OwnedIdentity, Service, SourceTicket};
use keyrill_ffi::NativeEvent;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use windows::Win32::Foundation::{E_FAIL, RPC_E_WRONG_THREAD, S_FALSE, S_OK};
use windows::Win32::UI::TextServices::*;
use windows_core::{BOOL, HRESULT, IUnknown, Interface, Result, implement};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Effect {
    #[default]
    Unattempted,
    CompositionAttempted,
    Attempted,
    Verified,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Completion {
    #[default]
    Refused,
    Unknown,
    Reserved,
    ManualMark,
}

struct Editing {
    context: Lease<ITfContext>,
    identity: Option<Lease<IUnknown>>,
    event: NativeEvent,
    authority: Authority,
}

enum Work {
    #[cfg(feature = "test-api")]
    Probe,
    Editing(Editing),
}

enum FrameOwner {
    Incoming(OwnServiceLease),
    Retirement {
        _service: Rc<Service>,
        _module: crate::service::ObjectLease,
    },
}

pub(crate) struct Frame {
    service: Rc<Service>,
    work: Work,
    entered: Cell<bool>,
    completed: Cell<bool>,
    expired: Cell<bool>,
    admission_revoked: Arc<AtomicBool>,
    effect: Cell<Effect>,
    stage: Cell<Stage>,
    field: Cell<Option<CandidateField>>,
    ownership: RefCell<Option<Rc<OwnedIdentity>>>,
    eaten: Cell<bool>,
    completion: Cell<Completion>,
    last_action: Cell<Action>,
    last_disposition: Cell<Disposition>,
    outcome_recorded: Cell<bool>,
    diagnose: bool,
    decision: Option<Rc<Cell<bool>>>,
    source_candidate: Cell<bool>,
    source_handoff: RefCell<Option<Rc<SourceTicket>>>,
    // Retain the service and module through native reference retirement above.
    owner: FrameOwner,
}

struct Observe<'a> {
    frame: &'a Frame,
    previous: Stage,
}
impl Drop for Observe<'_> {
    fn drop(&mut self) {
        self.frame.stage.set(self.previous);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SelectionValue {
    start: i32,
    length: i32,
    active_end: TfActiveSelEnd,
    interim: BOOL,
}
struct SelectionObservation {
    range: Lease<ITfRange>,
    value: SelectionValue,
}
struct RangePermit<'range> {
    range: &'range Lease<ITfRange>,
    _scope: ScopeEvidence,
}

struct ScopeEvidence {
    // Release can reenter the host, so these leases outlive the permitted native operation.
    _property: Lease<ITfReadOnlyProperty>,
    _scope: Option<Lease<ITfInputScope>>,
    _values: Option<native::Scopes>,
}

struct FinishSelection {
    original: SelectionValue,
    carrier: Lease<ITfRange>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnedOperation {
    Mark,
    ReplaceAndEnd,
    EndVerifiedUnchanged,
}

impl Frame {
    fn with_work(
        service: Rc<Service>,
        owner: FrameOwner,
        work: Work,
        ownership: Option<Rc<OwnedIdentity>>,
        diagnose: bool,
        decision: Option<Rc<Cell<bool>>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            service,
            work,
            entered: Cell::new(false),
            completed: Cell::new(false),
            expired: Cell::new(false),
            admission_revoked: Arc::new(AtomicBool::new(false)),
            effect: Cell::new(Effect::Unattempted),
            stage: Cell::new(Stage::Session),
            field: Cell::new(None),
            ownership: RefCell::new(ownership),
            eaten: Cell::new(false),
            completion: Cell::new(Completion::Refused),
            last_action: Cell::new(Action::Pass),
            last_disposition: Cell::new(Disposition::Pass),
            outcome_recorded: Cell::new(false),
            diagnose,
            decision,
            source_candidate: Cell::new(false),
            source_handoff: RefCell::new(None),
            owner,
        })
    }

    #[cfg(feature = "test-api")]
    pub(crate) fn new(service: Rc<Service>, owner: OwnServiceLease) -> Rc<Self> {
        Self::with_work(
            service,
            FrameOwner::Incoming(owner),
            Work::Probe,
            None,
            true,
            None,
        )
    }

    pub(crate) fn allow_source_handoff(&self, allowed: bool) {
        self.source_candidate.set(allowed);
    }

    pub(crate) fn take_source_handoff(&self) -> Option<Rc<SourceTicket>> {
        self.source_handoff.borrow_mut().take()
    }

    pub(crate) unsafe fn source_field(
        service: Rc<Service>,
        owner: OwnServiceLease,
        context: *mut std::ffi::c_void,
    ) -> Option<Identity> {
        let authority = service.capture_authority()?;
        if authority.owned.is_some() {
            return None;
        }
        let guard = authority.guard;
        let event = NativeEvent::new(6, 1, 1, &[]).ok()?;
        // SAFETY: the source-only caller retains the native receiving context for this metadata query.
        let frame = unsafe {
            Self::capture(
                Rc::clone(&service),
                owner,
                context,
                event,
                authority,
                false,
                None,
            )
        }
        .ok()?;
        let editing = frame.editing_work()?;
        let field = frame.fresh_field(editing)?;
        if !frame.writable(editing) || frame.no_foreign(editing).is_none() || !frame.alive() {
            return None;
        }
        drop(frame);
        service.current_owned(guard, None).then_some(field.identity)
    }

    pub(crate) fn retirement(
        service: Rc<Service>,
        authority: Authority,
        mode: u32,
    ) -> Option<Rc<Self>> {
        let owned = authority.owned.as_ref()?;
        let event = NativeEvent::new(6, mode, 1, &[]).ok()?;
        let context = owned._context.clone();
        let identity = Some(owned.context_id.clone());
        let ownership = Some(Rc::clone(&owned.identity));
        let owner = FrameOwner::Retirement {
            _service: Rc::clone(&service),
            _module: crate::service::ObjectLease::new(),
        };
        Some(Self::with_work(
            service,
            owner,
            Work::Editing(Editing {
                context,
                identity,
                event,
                authority,
            }),
            ownership,
            true,
            None,
        ))
    }

    pub(crate) unsafe fn editing(
        service: Rc<Service>,
        owner: OwnServiceLease,
        context: *mut std::ffi::c_void,
        event: NativeEvent,
        authority: Authority,
    ) -> std::result::Result<Rc<Self>, Call> {
        // SAFETY: forwards the inbound context lifetime required by this constructor.
        unsafe { Self::capture(service, owner, context, event, authority, true, None) }
    }

    unsafe fn capture(
        service: Rc<Service>,
        owner: OwnServiceLease,
        context: *mut std::ffi::c_void,
        event: NativeEvent,
        authority: Authority,
        diagnose: bool,
        decision: Option<Rc<Cell<bool>>>,
    ) -> std::result::Result<Rc<Self>, Call> {
        // SAFETY: Execute's inbound caller retains this context throughout capture and request.
        let context = unsafe { foreign::retain::<ITfContext>(context) }?;
        let ownership = authority
            .owned
            .as_ref()
            .map(|owned| Rc::clone(&owned.identity));
        let identity = match context.query::<IUnknown>() {
            Ok(identity) => Some(identity),
            Err(error) => {
                let call = error.call();

                if diagnose {
                    service.refuse(
                        Stage::Session,
                        if matches!(error, InterfaceError::Missing) {
                            Cause::Missing
                        } else {
                            Cause::Native
                        },
                        if matches!(error, InterfaceError::Missing) {
                            S_OK
                        } else {
                            call.result
                        },
                    );
                }
                None
            }
        };
        let frame = Self::with_work(
            service,
            FrameOwner::Incoming(owner),
            Work::Editing(Editing {
                context,
                identity,
                event,
                authority,
            }),
            ownership,
            diagnose,
            decision,
        );
        Ok(frame)
    }

    pub(crate) unsafe fn release_field(
        service: Rc<Service>,
        owner: OwnServiceLease,
        context: *mut std::ffi::c_void,
        authority: Authority,
        decision: Rc<Cell<bool>>,
    ) -> Option<Identity> {
        let expected = authority.owned.as_ref()?.context_id.address();
        let event = NativeEvent::new(6, 1, 1, &[]).ok()?;
        // SAFETY: the synchronous callback retains context through this read-only proof.
        let frame = unsafe {
            Self::capture(
                service,
                owner,
                context,
                event,
                authority,
                false,
                Some(decision),
            )
        }
        .ok()?;
        let editing = frame.editing_work()?;
        if editing.identity.as_ref()?.address() != expected || !frame.alive() {
            return None;
        }
        let field = frame.fresh_field(editing)?;
        if !frame.writable(editing) {
            return None;
        }
        let result = frame.alive().then_some(field.identity);
        let service = Rc::clone(&frame.service);
        let guard = editing.authority.guard;
        let ownership = frame.ownership.borrow().clone();
        let decision = frame.decision.clone();
        drop(frame);
        result.filter(|_| {
            decision.as_ref().is_none_or(|decision| !decision.get())
                && service.current_owned(guard, ownership.as_ref())
        })
    }

    fn editing_work(&self) -> Option<&Editing> {
        match &self.work {
            Work::Editing(editing) => Some(editing),
            #[cfg(feature = "test-api")]
            Work::Probe => None,
        }
    }

    fn observe(&self, stage: Stage) -> Observe<'_> {
        Observe {
            frame: self,
            previous: self.stage.replace(stage),
        }
    }

    fn invalidate(&self, stop: bool) {
        if !self.diagnose {
            return;
        }
        if let Some(editing) = self.editing_work() {
            self.service.invalidate_if(editing.authority.guard, stop);
        }
    }

    fn refuse(&self, stage: Stage, cause: Cause, result: HRESULT) {
        if self.diagnose {
            self.service.refuse(stage, cause, result);
        }
    }

    pub(crate) fn alive(&self) -> bool {
        if !self.service.owner_thread()
            || self.expired.get()
            || self.admission_revoked.load(Ordering::Acquire)
            || self
                .decision
                .as_ref()
                .is_some_and(|decision| decision.get())
        {
            return false;
        }
        if let Some(editing) = self.editing_work() {
            editing.identity.is_some()
                && self
                    .service
                    .current_owned(editing.authority.guard, self.ownership.borrow().as_ref())
        } else {
            true
        }
    }

    fn check(&self, valid: bool, cause: Cause) -> bool {
        if !valid {
            self.refuse(self.stage.get(), cause, S_OK);
        }
        valid
    }

    fn status(&self, call: Call, expected: HRESULT) -> bool {
        if call.result != expected {
            self.refuse(
                self.stage.get(),
                if call.result == S_OK {
                    Cause::Predicate
                } else {
                    Cause::Native
                },
                call.result,
            );
            return false;
        }
        self.check(self.alive(), Cause::Stale)
    }

    fn exact(&self, call: Call) -> bool {
        self.status(call, S_OK)
    }

    fn required<T: Interface>(&self, output: Returned<Option<Lease<T>>>) -> Option<Lease<T>> {
        if !self.exact(output.call) {
            drop(output.value);
            return None;
        }
        if output.value.is_none() {
            self.check(false, Cause::Missing);
        }
        output.value
    }

    fn queried<T: Interface>(
        &self,
        query: std::result::Result<Lease<T>, InterfaceError>,
    ) -> Option<Lease<T>> {
        match query {
            Ok(lease) => Some(lease),
            Err(error) => {
                if matches!(error, InterfaceError::Missing) {
                    self.check(false, Cause::Missing);
                } else {
                    self.exact(error.call());
                }
                None
            }
        }
    }

    fn identity<T: Interface>(&self, object: &Lease<T>, expected: usize) -> bool {
        let _observe = self.observe(Stage::Context);
        if !self.alive() {
            return false;
        }
        let Some(identity) = self.queried(object.query::<IUnknown>()) else {
            return false;
        };
        let matches = self.check(identity.address() == expected, Cause::Predicate);
        drop(identity);
        matches && self.alive()
    }

    fn fresh_field(&self, editing: &Editing) -> Option<CandidateField> {
        let _observe = self.observe(Stage::Focus);
        if !self.check(self.alive(), Cause::Stale) {
            return None;
        }
        let document = self.required(native::focus(&editing.authority.manager))?;
        let top = self.required(native::top(&document))?;
        if !self.identity(&top, editing.identity.as_ref()?.address()) {
            return None;
        }
        let view = self.required(native::view(&editing.context))?;
        let window = native::window(&view);
        if !self.exact(window.call) {
            return None;
        }
        self.stage.set(Stage::ProofWindow);
        let facts = self
            .service
            .capture_field(window.value, editing.authority.host_identity);
        if !self.exact(facts.call) {
            return None;
        }
        let field = field::classify(&facts.value);
        self.check(field.is_some() && self.alive(), Cause::Predicate)
            .then_some(field)
            .flatten()
    }

    fn extent(&self, editing: &Editing, range: &Lease<ITfRange>) -> Option<(i32, i32)> {
        let _observe = self.observe(Stage::Context);
        if !self.alive() {
            return None;
        }
        let context = self.required(native::range_context(range))?;
        if !self.identity(&context, editing.identity.as_ref()?.address()) {
            return None;
        }
        let acp = self.queried(range.query::<ITfRangeACP>())?;
        let extent = native::extent(&acp);
        let (start, length) = extent.value;
        if !self.exact(extent.call)
            || !self.check(
                start >= 0 && length >= 0 && start.checked_add(length).is_some(),
                Cause::Predicate,
            )
        {
            return None;
        }
        Some((start, length))
    }

    fn disabled<T: Interface>(
        &self,
        target: &Lease<T>,
        id: &windows_core::GUID,
        stage: Stage,
    ) -> bool {
        let _observe = self.observe(stage);
        if !self.alive() {
            return true;
        }
        let Some(manager) = self.queried(target.query::<ITfCompartmentMgr>()) else {
            return true;
        };
        let Some(compartment) = self.required(native::compartment(&manager, id)) else {
            return true;
        };
        let output = native::compartment_value(&compartment);
        let unset = output.call.result == S_FALSE && output.value.empty();
        let allowed = (if unset {
            self.status(output.call, S_FALSE)
        } else {
            self.exact(output.call)
        }) && self.check(
            output.value.empty() || output.value.zero_integer(),
            Cause::Predicate,
        );
        let _ = output.value.clear();

        !allowed || !self.check(self.alive(), Cause::Stale)
    }

    fn writable(&self, editing: &Editing) -> bool {
        let _observe = self.observe(Stage::Status);
        if !self.alive() {
            return false;
        }
        let status = native::status(&editing.context);
        self.exact(status.call)
            && self.check(
                status.value.dwDynamicFlags & TF_SD_READONLY == 0,
                Cause::Predicate,
            )
            && !self.disabled(
                &editing.context,
                &GUID_COMPARTMENT_KEYBOARD_DISABLED,
                Stage::ContextDisabled,
            )
            && !self.disabled(
                &editing.context,
                &GUID_COMPARTMENT_EMPTYCONTEXT,
                Stage::EmptyContext,
            )
            && !self.disabled(
                &editing.authority.manager,
                &GUID_COMPARTMENT_KEYBOARD_DISABLED,
                Stage::ThreadDisabled,
            )
    }

    fn ordinary_scope(
        &self,
        editing: &Editing,
        cookie: u32,
        range: &Lease<ITfRange>,
        field: CandidateField,
    ) -> Option<ScopeEvidence> {
        let _observe = self.observe(Stage::AppProperty);
        if !self.alive() {
            return None;
        }
        let property = self.required(native::property(&editing.context))?;
        self.stage.set(Stage::AppValue);
        let output = native::property_value(&property, cookie, range);
        if output.call.result == E_FAIL
            && output.value.empty()
            && field.contract == Contract::NotepadBounded
        {
            let alive = self.check(self.alive(), Cause::Stale);
            let _ = output.value.clear();
            return (alive && self.alive()).then_some(ScopeEvidence {
                _property: property,
                _scope: None,
                _values: None,
            });
        }
        let valid = self.exact(output.call)
            && self.check(output.value.unknown(), Cause::WrongType)
            && self.check(output.value.has_unknown(), Cause::Missing);
        if !valid {
            let _ = output.value.clear();

            return None;
        }
        self.stage.set(Stage::ScopeInterface);
        let scope = output.value.input_scope();
        let _ = output.value.clear();
        if !self.alive() {
            return None;
        }
        let scope = self.queried(scope)?;
        self.stage.set(Stage::Scopes);
        let scopes = native::scopes(&scope);
        let valid = self.exact(scopes.call)
            && self.check(scopes.value.ordinary(), Cause::Predicate)
            && self.check(self.alive(), Cause::Stale);
        valid.then_some(ScopeEvidence {
            _property: property,
            _scope: Some(scope),
            _values: Some(scopes.value),
        })
    }

    fn permit_range<'range>(
        &self,
        editing: &Editing,
        cookie: u32,
        range: &'range Lease<ITfRange>,
        expected: CandidateField,
    ) -> Option<RangePermit<'range>> {
        let field = self.fresh_field(editing)?;
        if !self.check(field == expected, Cause::Predicate) {
            return None;
        }
        let context = self.required(native::range_context(range))?;
        if !self.identity(
            &context,
            editing.identity.as_ref().map_or(0, Lease::address),
        ) {
            return None;
        }
        drop(context);
        if !self.writable(editing) {
            return None;
        }
        let scope = self.ordinary_scope(editing, cookie, range, expected)?;
        let facts = self
            .service
            .capture_field(expected.identity.field, editing.authority.host_identity);
        let permitted = self.exact(facts.call)
            && self.check(
                field::classify(&facts.value) == Some(field) && self.alive(),
                Cause::Stale,
            );
        permitted.then_some(RangePermit {
            range,
            _scope: scope,
        })
    }

    fn selection(
        &self,
        editing: &Editing,
        cookie: u32,
        collapsed: bool,
    ) -> Option<SelectionObservation> {
        let _observe = self.observe(Stage::Focus);
        let field = self.fresh_field(editing)?;
        if self.field.get().is_some_and(|previous| previous != field) || !self.writable(editing) {
            return None;
        }
        self.stage.set(Stage::Selection);
        let selection = native::selection(&editing.context, cookie);
        if !self.exact(selection.call) {
            return None;
        }
        let valid_style = selection.value.style.fInterimChar == BOOL(0)
            && [TF_AE_NONE, TF_AE_START, TF_AE_END].contains(&selection.value.style.ase);
        if !self.check(
            selection.value.fetched == 1 && selection.value.range.is_some() && valid_style,
            Cause::Predicate,
        ) {
            return None;
        }
        let range = selection.value.range?;
        self.permit_range(editing, cookie, &range, field)?;
        let (start, length) = self.extent(editing, &range)?;
        if !self.check(!collapsed || length == 0, Cause::Predicate)
            || self.permit_range(editing, cookie, &range, field).is_none()
            || !self.check(
                self.fresh_field(editing) == Some(field) && self.alive(),
                Cause::Stale,
            )
        {
            return None;
        }
        self.field.set(Some(field));
        Some(SelectionObservation {
            range,
            value: SelectionValue {
                start,
                length,
                active_end: selection.value.style.ase,
                interim: selection.value.style.fInterimChar,
            },
        })
    }

    fn selection_matches(&self, editing: &Editing, cookie: u32, expected: SelectionValue) -> bool {
        self.selection(editing, cookie, false)
            .is_some_and(|actual| self.check(actual.value == expected, Cause::Predicate))
    }

    fn no_foreign(&self, editing: &Editing) -> Option<Lease<ITfContextComposition>> {
        let _observe = self.observe(Stage::Foreign);
        if !self.alive() {
            return None;
        }
        let context = self.queried(editing.context.query::<ITfContextComposition>())?;
        let entries = self.required(native::compositions(&context))?;
        let next = native::next(&entries);
        if !self.status(next.call, S_FALSE)
            || !self.check(
                next.value.fetched == 0 && next.value.view.is_none(),
                Cause::Predicate,
            )
        {
            return None;
        }
        Some(context)
    }

    fn active_composition(&self, editing: &Editing, expected: usize) -> bool {
        if !self.alive() {
            return false;
        }
        let Some(context) = self.queried(editing.context.query::<ITfContextComposition>()) else {
            return false;
        };
        let Some(entries) = self.required(native::compositions(&context)) else {
            return false;
        };
        let first = native::next(&entries);
        if !self.exact(first.call) || first.value.fetched != 1 {
            return false;
        }
        let Some(view) = first.value.view else {
            return false;
        };
        if !self.identity(&view, expected) {
            return false;
        }
        drop(view);
        if !self.alive() {
            return false;
        }
        let last = native::next(&entries);
        self.status(last.call, S_FALSE) && last.value.fetched == 0 && last.value.view.is_none()
    }

    fn read(
        &self,
        editing: &Editing,
        cookie: u32,
        range: &Lease<ITfRange>,
        expected: &[u16],
    ) -> bool {
        let Some(field) = self.field.get() else {
            return false;
        };
        let Some(permit) = self.permit_range(editing, cookie, range, field) else {
            return false;
        };
        let text = native::text(permit.range, cookie, 129);
        self.exact(text.call)
            && text.value.count as usize == expected.len()
            && text.value.units == expected
    }

    fn owned_range(
        &self,
        editing: &Editing,
        cookie: u32,
        expected: &[u16],
        caret: bool,
    ) -> Option<Lease<ITfRange>> {
        let owned = self.service.owned()?;
        if owned.context_id.address() != editing.identity.as_ref()?.address()
            || !self.alive()
            || !self.identity(&owned.composition, owned.composition_id.address())
            || !self.active_composition(editing, owned.composition_id.address())
        {
            return None;
        }
        let range = self.required(native::composition_range(&owned.composition))?;
        let (start, length) = self.extent(editing, &range)?;
        if start != owned.start
            || length as usize != expected.len()
            || !self.read(editing, cookie, &range, expected)
        {
            return None;
        }
        if caret {
            let selection = self.selection(editing, cookie, true)?;
            if selection.value.start != start + length {
                return None;
            }
        }
        self.alive().then_some(range)
    }

    fn boundary(&self, editing: &Editing, cookie: u32, selection: &SelectionObservation) -> bool {
        let _observe = self.observe(Stage::Boundary);
        if selection.value.start == 0 {
            return true;
        }
        if !self.alive() {
            return false;
        }
        let Some(prefix) = self.required(native::clone_range(&selection.range)) else {
            return false;
        };
        let shifted = native::shift_start(&prefix, cookie, -1);
        if !self.exact(shifted.call) || !self.check(shifted.value == -1, Cause::Predicate) {
            return false;
        }
        if self.extent(editing, &prefix) != Some((selection.value.start - 1, 1)) {
            return false;
        }
        let Some(field) = self.field.get() else {
            return false;
        };
        let Some(permit) = self.permit_range(editing, cookie, &prefix, field) else {
            return false;
        };
        let text = native::text(permit.range, cookie, 2);
        if !self.exact(text.call) || text.value.count != 1 {
            return false;
        }
        let Some(unit) = text.value.units.first() else {
            return false;
        };
        let whitespace = native::whitespace(*unit);
        self.exact(whitespace.call) && self.check(whitespace.value, Cause::Predicate)
    }

    fn capture_finish_selection(&self, editing: &Editing, cookie: u32) -> Option<FinishSelection> {
        let selection = self.selection(editing, cookie, false)?;
        let carrier = self.required(native::clone_range(&selection.range))?;
        if self.extent(editing, &carrier) != Some((selection.value.start, selection.value.length)) {
            return None;
        }
        Some(FinishSelection {
            original: selection.value,
            carrier,
        })
    }

    fn preserve_finish_selection(
        &self,
        editing: &Editing,
        cookie: u32,
        saved: &FinishSelection,
    ) -> bool {
        let Some(selection) = self.selection(editing, cookie, false) else {
            return false;
        };
        if selection.value == saved.original {
            return true;
        }
        let Some((start, length)) = self.extent(editing, &saved.carrier) else {
            return false;
        };
        let Some(field) = self.field.get() else {
            return false;
        };
        if self
            .permit_range(editing, cookie, &saved.carrier, field)
            .is_none()
        {
            return false;
        }
        let end = saved.original.start + saved.original.length;
        let amount = end - (start + length);
        if amount != 0 {
            let shifted = native::shift_end(&saved.carrier, cookie, amount);
            if !self.exact(shifted.call) || !self.check(shifted.value == amount, Cause::Predicate) {
                return false;
            }
        }
        let Some((start, length)) = self.extent(editing, &saved.carrier) else {
            return false;
        };
        if start + length != end
            || self
                .permit_range(editing, cookie, &saved.carrier, field)
                .is_none()
        {
            return false;
        }
        let amount = saved.original.start - start;
        if amount != 0 {
            let shifted = native::shift_start(&saved.carrier, cookie, amount);
            if !self.exact(shifted.call) || !self.check(shifted.value == amount, Cause::Predicate) {
                return false;
            }
        }
        if self.extent(editing, &saved.carrier)
            != Some((saved.original.start, saved.original.length))
            || self.no_foreign(editing).is_none()
        {
            return false;
        }
        let Some(permit) = self.permit_range(editing, cookie, &saved.carrier, field) else {
            return false;
        };
        self.exact(native::set_selection(
            &editing.context,
            cookie,
            permit.range,
            TF_SELECTIONSTYLE {
                ase: saved.original.active_end,
                fInterimChar: saved.original.interim,
            },
        )) && self.selection_matches(editing, cookie, saved.original)
    }

    fn apply(&self, editing: &Editing, cookie: u32, plan: &Plan) -> bool {
        let _observe = self.observe(Stage::Apply);
        let operation = match (plan.action, plan.mark == plan.expected, &self.owner) {
            (Action::Mark, _, _) => OwnedOperation::Mark,
            (Action::Finish, true, FrameOwner::Incoming(_)) => OwnedOperation::EndVerifiedUnchanged,
            _ => OwnedOperation::ReplaceAndEnd,
        };
        if plan.action == Action::Finish {
            self.service.revoke_release_if(editing.authority.guard);
        }
        let range = if let Some(owned) = self.service.owned() {
            if plan.expected != owned.verified {
                return false;
            }
            self.owned_range(
                editing,
                cookie,
                &plan.expected,
                plan.action != Action::Finish || plan.mark != plan.expected,
            )
        } else {
            if plan.action != Action::Mark || !plan.expected.is_empty() {
                return false;
            }
            let Some(selection) = self.selection(editing, cookie, true) else {
                return false;
            };
            let Some(context) = self.no_foreign(editing) else {
                return false;
            };
            let Some(field) = self.field.get() else {
                return false;
            };
            let Some(permit) = self.permit_range(editing, cookie, &selection.range, field) else {
                return false;
            };
            let FrameOwner::Incoming(owner) = &self.owner else {
                return false;
            };
            self.effect.set(Effect::CompositionAttempted);
            let started = native::start_composition(
                &context,
                cookie,
                &selection.range,
                owner.composition_sink(),
            );
            if started.call.exact() && started.value.is_none() {
                // S_OK plus null is TSF's clean rejection only while the field is still
                // current and independently confirms no composition or selection change.
                if self.alive()
                    && self.no_foreign(editing).is_some()
                    && self.selection_matches(editing, cookie, selection.value)
                {
                    self.effect.set(Effect::Unattempted);
                }
                self.check(false, Cause::Missing);
                return false;
            }
            let Some(composition) = self.required(started) else {
                return false;
            };
            drop(permit);
            if !self.alive() {
                return false;
            }
            let Some(composition_id) = self.queried(composition.query::<IUnknown>()) else {
                return false;
            };
            if !self.alive() {
                return false;
            }
            let Some(context_id) = &editing.identity else {
                return false;
            };
            let owned = Owned {
                identity: Rc::new(OwnedIdentity),
                _context: editing.context.clone(),
                context_id: context_id.clone(),
                composition,
                composition_id,
                start: selection.value.start,
                verified: Vec::new(),
                serial: 0,
            };
            let Some(owned) = self.service.publish_owned(editing.authority.guard, owned) else {
                return false;
            };
            self.ownership.replace(Some(Rc::clone(&owned.identity)));
            drop(selection);
            if !self.alive() {
                return false;
            }
            self.owned_range(editing, cookie, &plan.expected, true)
        };
        let Some(range) = range else {
            return false;
        };
        let original = if plan.action == Action::Finish && plan.mark == plan.expected {
            let Some(saved) = self.capture_finish_selection(editing, cookie) else {
                return false;
            };
            if self
                .owned_range(editing, cookie, &plan.expected, false)
                .is_none()
            {
                return false;
            }
            Some(saved)
        } else {
            None
        };
        if original
            .as_ref()
            .is_some_and(|saved| !self.selection_matches(editing, cookie, saved.original))
        {
            return false;
        }
        let Some(field) = self.field.get() else {
            return false;
        };
        if operation != OwnedOperation::EndVerifiedUnchanged {
            let Some(permit) = self.permit_range(editing, cookie, &range, field) else {
                return false;
            };
            self.service.count(Counter::Attempted);
            self.effect.set(Effect::Attempted);
            if !self.exact(native::set_text(permit.range, cookie, &plan.mark)) {
                return false;
            }
        }
        let Some((start, length)) = self.extent(editing, &range) else {
            return false;
        };
        let Some(owned) = self.service.owned() else {
            return false;
        };
        if start != owned.start
            || length as usize != plan.mark.len()
            || !self.read(editing, cookie, &range, &plan.mark)
        {
            return false;
        }
        let composition = owned.composition.clone();
        let Some(actual) = self.required(native::composition_range(&composition)) else {
            return false;
        };
        if self.extent(editing, &actual) != Some((start, length))
            || !self.read(editing, cookie, &actual, &plan.mark)
        {
            return false;
        }
        if plan.action == Action::Mark || plan.mark != plan.expected {
            let Some(caret) = self.required(native::clone_range(&actual)) else {
                return false;
            };
            if !self.exact(native::collapse(&caret, cookie, TF_ANCHOR_END)) {
                return false;
            }
            let Some(permit) = self.permit_range(editing, cookie, &caret, field) else {
                return false;
            };
            if !self.exact(native::set_selection(
                &editing.context,
                cookie,
                permit.range,
                TF_SELECTIONSTYLE {
                    ase: TF_AE_NONE,
                    fInterimChar: BOOL(0),
                },
            )) {
                return false;
            }
            let Some(selection) = self.selection(editing, cookie, true) else {
                return false;
            };
            if selection.value.start != start + length
                || self
                    .owned_range(editing, cookie, &plan.mark, true)
                    .is_none()
            {
                return false;
            }
        }
        if plan.action == Action::Mark {
            self.service
                .verify_owned(editing.authority.guard, &owned.identity, &plan.mark)
        } else {
            if operation == OwnedOperation::EndVerifiedUnchanged
                && original
                    .as_ref()
                    .is_none_or(|saved| !self.selection_matches(editing, cookie, saved.original))
            {
                return false;
            }
            let Some(_permit) = self.permit_range(editing, cookie, &actual, field) else {
                return false;
            };
            self.service.count(Counter::Attempted);
            self.service.lifecycle(
                LifecycleKind::EndRequested,
                owned.serial,
                LifecycleCause::None,
                Unresolved::None,
            );
            // Termination is the first native effect for an unchanged ordinary finish.
            self.effect.set(Effect::Attempted);
            if !self.exact(native::end_composition(&composition, cookie))
                || self.extent(editing, &actual) != Some((start, length))
                || !self.read(editing, cookie, &actual, &plan.mark)
                || self.no_foreign(editing).is_none()
            {
                return false;
            }
            if plan.mark != plan.expected {
                let Some(selection) = self.selection(editing, cookie, true) else {
                    return false;
                };
                if selection.value.start != start + length {
                    return false;
                }
            }
            if let Some(saved) = &original
                && (!self.preserve_finish_selection(editing, cookie, saved)
                    || self.extent(editing, &actual) != Some((start, length))
                    || !self.read(editing, cookie, &actual, &plan.mark)
                    || self.no_foreign(editing).is_none()
                    || !self.selection_matches(editing, cookie, saved.original))
            {
                return false;
            }
            let retired = self
                .service
                .take_owned(editing.authority.guard, &owned.identity);
            if retired.is_none() {
                return false;
            }
            self.ownership.replace(None);
            drop(retired);
            if !self.alive() {
                return false;
            }
            self.service.lifecycle(
                LifecycleKind::FinishVerified,
                owned.serial,
                LifecycleCause::None,
                Unresolved::None,
            );
            true
        }
    }

    fn run_editing(&self, editing: &Editing, cookie: u32) {
        if !self.check(self.alive(), Cause::Stale) {
            return;
        }
        let Some(mut selection) = self.selection(editing, cookie, self.service.owned().is_none())
        else {
            return;
        };
        if self.service.owned().is_some_and(|owned| {
            Some(owned.context_id.address()) != editing.identity.as_ref().map(Lease::address)
        }) {
            self.refuse(Stage::Context, Cause::Predicate, S_OK);
            self.invalidate(true);
            return;
        }
        let mut context = 1;
        if self.service.owned().is_none() {
            if self.no_foreign(editing).is_none() || !self.boundary(editing, cookie, &selection) {
                return;
            }
            context = 0;
        }
        for _ in 0..2 {
            if !self.alive() {
                return;
            }
            self.stage.set(Stage::Plan);
            let Ok(view) = editing.event.view() else {
                self.invalidate(true);
                return;
            };
            let Ok(event) = NativeEvent::new(view.role, view.mode, context, view.units) else {
                return;
            };
            let Ok(plan) = self.service.propose(editing.authority.guard, &event) else {
                self.refuse(Stage::Plan, Cause::Predicate, S_OK);
                self.invalidate(true);
                return;
            };
            if matches!(self.owner, FrameOwner::Retirement { .. })
                && (plan.action != Action::Finish || plan.mark != plan.expected)
            {
                self.invalidate(true);
                return;
            }
            self.last_action.set(plan.action);
            self.last_disposition.set(plan.disposition);
            if matches!(plan.action, Action::Pass | Action::Unavailable) {
                if !self.alive()
                    || self
                        .service
                        .acknowledge(editing.authority.guard, plan.id, 0)
                        .is_err()
                {
                    self.refuse(Stage::Acknowledge, Cause::Predicate, S_OK);
                    self.invalidate(true);
                    return;
                }
                self.eaten.set(plan.disposition == Disposition::Consumed);
                self.completion.set(if self.eaten.get() {
                    Completion::Reserved
                } else {
                    Completion::Refused
                });
                return;
            }
            self.effect.set(Effect::Unattempted);
            self.outcome_recorded.set(false);
            let previous = self.service.refusal_counts();
            let mut applied = self.apply(editing, cookie, &plan);
            let attempted = self.effect.get() != Effect::Unattempted;
            self.service.count(if applied {
                Counter::Verified
            } else if attempted {
                Counter::Unknown
            } else {
                Counter::Refused
            });
            self.outcome_recorded.set(true);
            if !applied && previous == self.service.refusal_counts() {
                self.refuse(Stage::Apply, Cause::Predicate, S_OK);
            }
            let current = self.alive();
            let outcome = if applied {
                0
            } else if attempted {
                2
            } else {
                1
            };
            if current
                && self
                    .service
                    .acknowledge(editing.authority.guard, plan.id, outcome)
                    .is_err()
            {
                self.refuse(Stage::Acknowledge, Cause::Predicate, S_OK);
                applied = false;
            }
            if !applied || !current {
                self.eaten.set(self.unknown_disposition());
                self.completion.set(if attempted {
                    Completion::Unknown
                } else {
                    Completion::Refused
                });
                self.invalidate(attempted || self.service.owned().is_some());
                return;
            }
            self.effect.set(Effect::Verified);
            self.eaten.set(plan.disposition == Disposition::Consumed);
            let source = self.field.get().and_then(|field| {
                self.service.verified_source_edit(
                    editing.authority.guard,
                    &event,
                    &plan,
                    field.identity,
                    self.source_candidate.get(),
                )
            });
            if self.source_candidate.get()
                && plan.action == Action::Finish
                && plan.disposition == Disposition::Replan
            {
                *self.source_handoff.borrow_mut() = source;
                return;
            }
            if plan.action == Action::Mark && (view.role == 4 || view.role == 5) {
                self.completion.set(Completion::ManualMark);
            }
            if plan.disposition != Disposition::Replan {
                return;
            }
            let Some(next) = self.selection(editing, cookie, true) else {
                return;
            };
            selection = next;
            if self.no_foreign(editing).is_none() || !self.boundary(editing, cookie, &selection) {
                return;
            }
            context = 0;
        }
    }

    pub(crate) fn unknown_disposition(&self) -> bool {
        matches!(self.effect.get(), Effect::Attempted | Effect::Verified)
            && (self.last_action.get() == Action::Mark
                || self.last_disposition.get() == Disposition::Consumed)
    }

    pub(crate) fn completion(&self) -> Completion {
        self.completion.get()
    }

    pub(crate) fn eaten(&self) -> bool {
        self.eaten.get()
    }

    fn run(&self, cookie: u32) -> HRESULT {
        if !self.service.owner_thread() {
            return RPC_E_WRONG_THREAD;
        }
        if self.entered.get() || !self.alive() {
            self.refuse(
                Stage::Session,
                if self.entered.get() {
                    Cause::Reentry
                } else {
                    Cause::Stale
                },
                S_OK,
            );
            return E_FAIL;
        }
        self.entered.set(true);
        self.service.count(Counter::SessionEntered);
        let body = || {
            match &self.work {
                Work::Editing(editing) => self.run_editing(editing, cookie),
                #[cfg(feature = "test-api")]
                Work::Probe => {
                    self.service.inject_panic(1);
                    self.effect.set(Effect::Attempted);
                    self.service.inject_panic(2);
                    self.effect.set(Effect::Verified);
                }
            }
            S_OK
        };
        let result = if let Some(editing) = self.editing_work() {
            self.service.edit_callback(editing.authority.guard, body)
        } else {
            self.service.checked(body)
        };
        if result.is_err() && self.editing_work().is_some() {
            if self.effect.get() != Effect::Unattempted && !self.outcome_recorded.replace(true) {
                self.service.count(Counter::Unknown);
            }
            self.eaten.set(self.unknown_disposition());
            self.completion
                .set(if self.effect.get() != Effect::Unattempted {
                    Completion::Unknown
                } else {
                    Completion::Refused
                });
        }
        self.completed.set(true);
        self.service.count(Counter::SessionCompleted);
        result
    }

    pub(crate) fn request(self: &Rc<Self>, context: &Lease<ITfContext>, client: u32) -> Request {
        let mut watch = crate::admission_watch::Watch::new(Arc::clone(&self.admission_revoked));
        if let Some(editing) = self.editing_work()
            && !watch.install(context, &editing.authority.manager)
        {
            self.admission_revoked.store(true, Ordering::Release);
        }
        let session: ITfEditSession = EditSession {
            frame: Rc::clone(self),
        }
        .into();
        let (outer, callback) = if self.alive() {
            foreign::request(context, client, &session, TF_ES_SYNC.0 | TF_ES_READWRITE.0)
        } else {
            (Call { result: E_FAIL }, E_FAIL)
        };
        watch.drain();
        let valid = self.alive();
        if self.admission_revoked.load(Ordering::Acquire) {
            self.invalidate(
                watch.uncertain
                    || self.effect.get() != Effect::Unattempted
                    || self.service.owned().is_some(),
            );
        }
        self.expired.set(true);
        Request {
            outer,
            callback,
            entered: self.entered.get(),
            completed: self.completed.get(),
            effect: self.effect.get(),
            valid,
        }
    }

    pub(crate) fn request_editing(self: &Rc<Self>) -> Request {
        let editing = self.editing_work().expect("editing frame request");
        self.request(&editing.context, editing.authority.client)
    }
}

pub(crate) struct Request {
    pub(crate) outer: Call,
    pub(crate) callback: HRESULT,
    pub(crate) entered: bool,
    pub(crate) completed: bool,
    pub(crate) effect: Effect,
    pub(crate) valid: bool,
}

#[implement(ITfEditSession, Agile = false)]
struct EditSession {
    frame: Rc<Frame>,
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, cookie: u32) -> Result<()> {
        self.frame.run(cookie).ok()
    }
}
