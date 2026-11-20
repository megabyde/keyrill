//! Independent diagnostic COM schema and canonical fixture publications.
use crate::protocol::{valid_lifecycle, valid_service};
use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use windows::Win32::Foundation::*;
use windows::Win32::UI::TextServices::*;
use windows::core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface};

/// Native pending result used by the diagnostic contract.
pub const E_PENDING: HRESULT = HRESULT(0x8000000au32 as i32);
/// The service CLSID from the external diagnostic contract.
pub const SERVICE_ID: GUID = GUID::from_u128(0x1a746212_84db_4acf_88b3_caaa26352774);
/// IID of the external version-two service snapshot reader.
pub const SNAPSHOT_IID: GUID = GUID::from_u128(0x4f89429e_a955_4885_bb33_c78ad1982481);
/// IID of the external runtime snapshot reader.
pub const RUNTIME_IID: GUID = GUID::from_u128(0xcda9ed25_1dbc_4fb6_b2a5_bc132945e082);
/// Canonical diagnostic stage labels in wire enumeration order.
pub const STAGES: [&str; 26] = [
    "Test",
    "Key",
    "Keyboard",
    "Profile",
    "Layout",
    "Mode",
    "Handle",
    "Request",
    "Session",
    "Selection",
    "Focus",
    "Context",
    "ProofWindow",
    "Status",
    "ContextDisabled",
    "EmptyContext",
    "ThreadDisabled",
    "AppProperty",
    "AppValue",
    "ScopeInterface",
    "Scopes",
    "Foreign",
    "Boundary",
    "Plan",
    "Apply",
    "Acknowledge",
];
/// Canonical refusal-cause labels in wire enumeration order.
pub const CAUSES: [&str; 15] = [
    "None",
    "Native",
    "Predicate",
    "Inactive",
    "Stopped",
    "ModeUnset",
    "Reentry",
    "Stale",
    "Missing",
    "WrongType",
    "WrongService",
    "NotActive",
    "WrongProfile",
    "Substitute",
    "Exception",
];
/// Canonical service-counter labels in wire enumeration order.
pub const COUNTERS: [&str; 12] = [
    "TestEntered",
    "Interested",
    "KeyEntered",
    "Consumed",
    "HandleEntered",
    "Requested",
    "SessionEntered",
    "SessionCompleted",
    "Attempted",
    "Verified",
    "Refused",
    "Unknown",
];
/// Canonical service-publication availability labels.
pub const AVAILABILITY: [&str; 6] = [
    "Missing",
    "Error",
    "Inactive",
    "Busy",
    "Available",
    "UnsupportedVersion",
];
/// Canonical service-publication continuity labels.
pub const CONTINUITY: [&str; 4] = ["None", "Reset", "Same", "Indeterminate"];
#[repr(C)]
#[derive(Clone, Debug, Eq, PartialEq)]
/// Independent C-layout service snapshot with 55 base and 86 lifecycle DWORDs.
pub struct ServiceSnapshot {
    /// 55 base diagnostic DWORDs matching the frozen service ABI.
    pub base: [u32; 55],
    /// Lifecycle header followed by sixteen five-DWORD event rows.
    pub lifecycle: [u32; 86],
}
impl Default for ServiceSnapshot {
    fn default() -> Self {
        Self {
            base: [0; 55],
            lifecycle: [0; 86],
        }
    }
}
const _: () = assert!(std::mem::size_of::<ServiceSnapshot>() == 564);
const _: () = assert!(std::mem::offset_of!(ServiceSnapshot, lifecycle) == 220);
impl ServiceSnapshot {
    /// Validate every field and reserved slot in this independent representation.
    pub fn valid(&self) -> bool {
        valid_service(&self.base) && valid_lifecycle(&self.lifecycle)
    }
    /// Report exhausted publication or lifecycle counters.
    pub fn saturated(&self) -> bool {
        self.base[4] != 0 || self.lifecycle[2] != 0
    }
    /// Iterate the retained lifecycle events in chronological order.
    pub fn events(&self) -> impl Iterator<Item = &[u32; 5]> {
        self.lifecycle[6..6 + self.lifecycle[3].min(16) as usize * 5]
            .as_chunks::<5>()
            .0
            .iter()
    }
}
#[repr(C)]
struct ReaderVtable {
    base: IUnknown_Vtbl,
    copy: unsafe extern "system" fn(*mut c_void, u32, u32, *mut c_void) -> HRESULT,
}
/// Canonical COM provider identity and its retained version-two snapshot reader.
pub struct Endpoint {
    /// Canonical retained provider or process identity.
    pub identity: IUnknown,
    reader: IUnknown,
}
impl Endpoint {
    /// Bind the diagnostic reader to its canonical service identity.
    ///
    /// # Errors
    /// Returns the first failed native operation or evidence predicate.
    pub fn discover(manager: &ITfThreadMgr) -> Result<Self, HRESULT> {
        Self::discover_with(manager, || true)
    }
    /// Bind the diagnostic reader while rechecking caller liveness after native calls.
    ///
    /// # Errors
    /// Returns the first failed native operation or evidence predicate.
    pub fn discover_with(
        manager: &ITfThreadMgr,
        alive: impl Fn() -> bool,
    ) -> Result<Self, HRESULT> {
        let mut raw = std::ptr::null_mut();
        let result = unsafe {
            (manager.vtable().GetFunctionProvider)(manager.as_raw(), &SERVICE_ID, &mut raw)
        };
        let provider = take_interface::<ITfFunctionProvider>(result, raw)?;
        if !alive() {
            return Err(E_ABORT);
        }
        let identity: IUnknown = query_exact(&provider)?;
        if !alive() {
            return Err(E_ABORT);
        }
        let mut raw = std::ptr::null_mut();
        let result = unsafe {
            (provider.vtable().GetFunction)(
                provider.as_raw(),
                &GUID::zeroed(),
                &SNAPSHOT_IID,
                &mut raw,
            )
        };
        let function = take_interface::<IUnknown>(result, raw)?;
        if !alive() {
            return Err(E_ABORT);
        }
        let mut raw = std::ptr::null_mut();
        let result = unsafe {
            (function.vtable().QueryInterface)(function.as_raw(), &SNAPSHOT_IID, &mut raw)
        };
        let reader = take_interface::<IUnknown>(result, raw)?;
        if !alive() {
            return Err(E_ABORT);
        }
        let reader_identity: IUnknown = query_exact(&reader)?;
        if !alive() {
            return Err(E_ABORT);
        }
        if identity.as_raw() != reader_identity.as_raw() {
            return Err(E_UNEXPECTED);
        }
        Ok(Self { identity, reader })
    }
    /// Compare the live native function-provider identity with the retained endpoint.
    ///
    /// # Errors
    /// Returns the first failed native operation or evidence predicate.
    pub fn same_provider(&self, manager: &ITfThreadMgr) -> Result<bool, HRESULT> {
        self.same_provider_with(manager, || true)
    }
    /// Recheck caller liveness around native provider-identity acquisition.
    ///
    /// # Errors
    /// Returns the first failed native operation or evidence predicate.
    pub fn same_provider_with(
        &self,
        manager: &ITfThreadMgr,
        alive: impl Fn() -> bool,
    ) -> Result<bool, HRESULT> {
        let mut raw = std::ptr::null_mut();
        let result = unsafe {
            (manager.vtable().GetFunctionProvider)(manager.as_raw(), &SERVICE_ID, &mut raw)
        };
        let provider = take_interface::<ITfFunctionProvider>(result, raw)?;
        if !alive() {
            return Err(E_ABORT);
        }
        let identity: IUnknown = query_exact(&provider)?;
        if !alive() {
            return Err(E_ABORT);
        }
        Ok(identity.as_raw() == self.identity.as_raw())
    }
    /// Read and independently validate the exact service snapshot ABI.
    ///
    /// # Errors
    /// Returns the first failed native operation or evidence predicate.
    pub fn copy(&self) -> Result<ServiceSnapshot, HRESULT> {
        let mut output = ServiceSnapshot::default();
        let vtable = unsafe { &**self.reader.as_raw().cast::<*const ReaderVtable>() };
        let result = unsafe {
            (vtable.copy)(
                self.reader.as_raw(),
                std::mem::size_of::<ServiceSnapshot>() as u32,
                2,
                (&mut output as *mut ServiceSnapshot).cast(),
            )
        };
        if result != S_OK {
            return Err(result);
        }
        if !output.valid() {
            return Err(E_UNEXPECTED);
        }
        Ok(output)
    }
}
fn query_exact<T: Interface>(object: &impl Interface) -> Result<T, HRESULT> {
    let mut raw = std::ptr::null_mut();
    let result = unsafe { object.query(&T::IID, &mut raw) };
    take_interface(result, raw)
}
fn take_interface<T: Interface>(result: HRESULT, raw: *mut c_void) -> Result<T, HRESULT> {
    if raw.is_null() {
        return Err(result);
    }
    let value = unsafe { T::from_raw(raw) };
    if result == S_OK {
        Ok(value)
    } else {
        Err(result)
    }
}
#[repr(C, align(8))]
struct Runtime([u32; 12]);
/// Read the external 48-byte runtime snapshot through its versioned interface.
///
/// The returned observation order interleaves each language's source state and
/// error in words 8 through 11. The native ABI groups both states before both errors.
///
/// # Errors
/// Returns the first failed native operation or evidence predicate.
pub fn read_runtime(manager: &ITfThreadMgr) -> Result<[u32; 12], HRESULT> {
    let mut raw = std::ptr::null_mut();
    let result =
        unsafe { (manager.vtable().GetFunctionProvider)(manager.as_raw(), &SERVICE_ID, &mut raw) };
    let provider = take_interface::<ITfFunctionProvider>(result, raw)?;
    let mut raw = std::ptr::null_mut();
    let result = unsafe {
        (provider.vtable().GetFunction)(provider.as_raw(), &GUID::zeroed(), &RUNTIME_IID, &mut raw)
    };
    let function = take_interface::<IUnknown>(result, raw)?;
    let mut raw = std::ptr::null_mut();
    let result =
        unsafe { (function.vtable().QueryInterface)(function.as_raw(), &RUNTIME_IID, &mut raw) };
    let reader = take_interface::<IUnknown>(result, raw)?;
    let vtable = unsafe { &**reader.as_raw().cast::<*const ReaderVtable>() };
    let mut value = Runtime([0; 12]);
    let result =
        unsafe { (vtable.copy)(reader.as_raw(), 48, 1, (&mut value as *mut Runtime).cast()) };
    if result == S_OK {
        value.0.swap(9, 10);
        Ok(value.0)
    } else {
        Err(result)
    }
}
/// Format the established canonical runtime status row.
pub fn format_runtime(value: &[u32; 12]) -> String {
    format!(
        "Keyrill runtime v1; activation={}; requested={}; effective={}; model_ready={}; revision={}; error={}; configuration_error={}; en_state={}; en_error={}; ru_state={}; ru_error={}",
        value[0],
        value[1],
        value[2],
        value[3],
        u64::from(value[4]) | (u64::from(value[5]) << 32),
        value[6],
        value[7],
        value[8],
        value[9],
        value[10],
        value[11]
    )
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Canonical service status row and its independently validated snapshot.
pub struct Publication {
    /// Monotonic host publication number.
    pub publication: u32,
    /// Whether a publication or event counter exhausted its range.
    pub saturated: u32,
    /// Monotonic host source identifier.
    pub source: u32,
    /// Service-reader availability enumeration value.
    pub availability: u32,
    /// Service-source continuity enumeration value.
    pub continuity: u32,
    /// Exact native HRESULT represented as a DWORD.
    pub result: u32,
    /// Independent complete service snapshot.
    pub snapshot: ServiceSnapshot,
}
impl Publication {
    /// Validate every field and reserved slot in this independent representation.
    pub fn valid(&self) -> bool {
        if self.availability >= 6
            || self.continuity >= 4
            || self.saturated > 1
            || ((self.publication == u32::MAX || self.source == u32::MAX) && self.saturated == 0)
            || !self.snapshot.valid()
            || (self.availability == 5 && self.result != E_NOINTERFACE.0 as u32)
        {
            return false;
        }
        match self.availability {
            4 => {
                self.snapshot.base[1] != 0
                    && self.result == 0
                    && self.source != 0
                    && self.continuity != 0
                    && ((self.saturated != 0 || self.snapshot.saturated())
                        == (self.continuity == 3))
            }
            2 => self.snapshot.base[1] == 0 && self.result == 0 && self.continuity == 0,
            _ => self.continuity == 0 && self.snapshot == ServiceSnapshot::default(),
        }
    }
    /// Format the established canonical status row.
    pub fn format(&self) -> Option<String> {
        if !self.valid() {
            return None;
        }
        let mut result = format!(
            "Keyrill service v2; state={}; continuity={}",
            AVAILABILITY[self.availability as usize], CONTINUITY[self.continuity as usize]
        );
        let header = [
            "publication",
            "host_saturated",
            "source",
            "result",
            "activation",
            "active",
            "stopped",
            "mode",
            "saturated",
            "request_present",
            "request_result",
            "session_present",
            "session_result",
        ];
        let values = [self.publication, self.saturated, self.source, self.result]
            .into_iter()
            .chain(self.snapshot.base[..9].iter().copied());
        for (label, value) in header.into_iter().zip(values) {
            result.push_str(&format!("; {label}={value}"));
        }
        for (label, value) in COUNTERS
            .into_iter()
            .chain(STAGES)
            .zip(self.snapshot.base[9..47].iter())
        {
            result.push_str(&format!("; {label}={value}"));
        }
        for (label, refusal) in ["first", "latest"]
            .into_iter()
            .zip(self.snapshot.base[47..].as_chunks::<4>().0.iter())
        {
            result.push_str(&format!(
                "; {label}={}/{}/{}/{}",
                refusal[0], STAGES[refusal[1] as usize], CAUSES[refusal[2] as usize], refusal[3]
            ));
        }
        for (label, value) in [
            "event_sequence",
            "mark_allocated",
            "lifecycle_saturated",
            "event_count",
            "owned_present",
            "owned_serial",
        ]
        .into_iter()
        .zip(self.snapshot.lifecycle)
        {
            result.push_str(&format!("; {label}={value}"));
        }
        for event in self.snapshot.lifecycle[6..].as_chunks::<5>().0.iter() {
            result.push_str(&format!(
                "; event={}/{}/{}/{}/{}",
                event[0], event[1], event[2], event[3], event[4]
            ));
        }
        Some(result)
    }
    /// Parse an exact canonical command, case name, or status representation.
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() >= 8192 {
            return None;
        }
        let fields: Vec<_> = text.split("; ").collect();
        if fields.len() != 78 || fields[0] != "Keyrill service v2" {
            return None;
        }
        let availability = AVAILABILITY
            .iter()
            .position(|label| fields[1].strip_prefix("state=") == Some(label))?
            as u32;
        let continuity = CONTINUITY
            .iter()
            .position(|label| fields[2].strip_prefix("continuity=") == Some(label))?
            as u32;
        let mut result = Self {
            availability,
            continuity,
            ..Self::default()
        };
        let values: Option<Vec<u32>> = fields[3..54]
            .iter()
            .map(|field| field.split_once('=')?.1.parse().ok())
            .collect();
        let values = values?;
        result.publication = values[0];
        result.saturated = values[1];
        result.source = values[2];
        result.result = values[3];
        result.snapshot.base[..47].copy_from_slice(&values[4..]);
        for (index, field) in fields[54..56].iter().enumerate() {
            let parts: Vec<_> = field.split_once('=')?.1.split('/').collect();
            if parts.len() != 4 {
                return None;
            }
            result.snapshot.base[47 + index * 4..51 + index * 4].copy_from_slice(&[
                parts[0].parse().ok()?,
                STAGES.iter().position(|&label| label == parts[1])? as u32,
                CAUSES.iter().position(|&label| label == parts[2])? as u32,
                parts[3].parse().ok()?,
            ]);
        }
        for (index, field) in fields[56..62].iter().enumerate() {
            result.snapshot.lifecycle[index] = field.split_once('=')?.1.parse().ok()?;
        }
        for (index, field) in fields[62..].iter().enumerate() {
            let parts: Option<Vec<u32>> = field
                .split_once('=')?
                .1
                .split('/')
                .map(|part| part.parse().ok())
                .collect();
            let parts = parts?;
            if parts.len() != 5 {
                return None;
            }
            result.snapshot.lifecycle[6 + index * 5..11 + index * 5].copy_from_slice(&parts);
        }
        (result.format().as_deref() == Some(text)).then_some(result)
    }
}
/// Require fresh publications from one unchanged diagnostic source.
pub fn comparable(before: &Publication, after: &Publication) -> bool {
    if !before.valid()
        || !after.valid()
        || before.availability != 4
        || after.availability != 4
        || before.publication == 0
        || after.publication <= before.publication
        || before.source != after.source
        || before.snapshot.base[0] == 0
        || before.snapshot.base[0] != after.snapshot.base[0]
        || after.continuity != 2
        || before.saturated != 0
        || after.saturated != 0
        || before.snapshot.saturated()
        || after.snapshot.saturated()
    {
        return false;
    }
    history(&before.snapshot, &after.snapshot)
}
/// Require monotonic counts and retained overlapping lifecycle history.
pub fn history(before: &ServiceSnapshot, after: &ServiceSnapshot) -> bool {
    let a = &before.lifecycle;
    let b = &after.lifecycle;
    if b[0] < a[0] || b[1] < a[1] || b[0] - a[0] > b[3] {
        return false;
    }
    let oldest = b[0] - b[3];
    before
        .events()
        .filter(|event| event[0] > oldest)
        .all(|event| {
            let offset = 6 + (event[0] - oldest - 1) as usize * 5;
            event == &b[offset..offset + 5]
        })
}
#[derive(Default)]
/// Fixture status publisher retaining source identity across native polls.
pub struct Poller {
    value: RefCell<Publication>,
    baseline: RefCell<Option<Baseline>>,
    epoch: RefCell<Rc<()>>,
}
struct Baseline {
    identity: IUnknown,
    activation: u32,
}
impl Poller {
    /// Copy the most recently completed publication without entering native code.
    pub fn value(&self) -> Publication {
        self.value.borrow().clone()
    }
    fn current(&self, epoch: &Rc<()>) -> bool {
        Rc::ptr_eq(&self.epoch.borrow(), epoch)
    }
    /// Seed the host counter for controlled saturation schedules.
    #[cfg(feature = "controlled-tests")]
    pub fn set_publication_number_for_test(&self, publication: u32) {
        self.value.borrow_mut().publication = publication;
    }
    /// Read the native service and advance the fixture publication.
    pub fn poll(&self, manager: &ITfThreadMgr) {
        let epoch = Rc::new(());
        self.epoch.replace(Rc::clone(&epoch));
        let baseline = self.baseline.borrow_mut().take();
        let mut value = self.value();
        value.publication = value.publication.saturating_add(1);
        value.saturated |= u32::from(value.publication == u32::MAX);
        value.snapshot = ServiceSnapshot::default();
        value.continuity = 0;
        let result = Endpoint::discover_with(manager, || self.current(&epoch))
            .and_then(|endpoint| endpoint.copy().map(|snapshot| (endpoint, snapshot)));
        if !self.current(&epoch) {
            return;
        }
        let mut next = None;
        let mut reader = None;
        match result {
            Ok((endpoint, snapshot)) => {
                value.result = 0;
                value.snapshot = snapshot;
                let Endpoint {
                    identity,
                    reader: retained,
                } = endpoint;
                reader = Some(retained);
                if value.snapshot.base[1] == 0 {
                    value.availability = 2;
                    // Keep the native identity alive until the completed state is published.
                    next = Some(Baseline {
                        identity,
                        activation: 0,
                    });
                } else {
                    let same = baseline.as_ref().is_some_and(|old| {
                        old.identity.as_raw() == identity.as_raw()
                            && old.activation == value.snapshot.base[0]
                    });
                    if !same {
                        value.source = value.source.saturating_add(1);
                        value.saturated |= u32::from(value.source == u32::MAX);
                    }
                    value.availability = 4;
                    value.continuity = if value.saturated != 0 || value.snapshot.saturated() {
                        3
                    } else if same {
                        2
                    } else {
                        1
                    };
                    next = Some(Baseline {
                        identity,
                        activation: value.snapshot.base[0],
                    });
                }
            }
            Err(error) => {
                value.result = error.0 as u32;
                value.availability = if error == TF_E_NOPROVIDER {
                    0
                } else if error == E_PENDING {
                    3
                } else if error == E_NOINTERFACE {
                    5
                } else {
                    1
                };
            }
        }
        let inactive = if value.availability == 2 {
            next.take()
        } else {
            None
        };
        let retired = self.baseline.replace(next);
        self.value.replace(value);
        drop(retired);
        drop(reader);
        drop(inactive);
        drop(baseline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_service_publication_roundtrip_and_negative_fields() {
        let value = Publication::default();
        let text = value.format().unwrap();
        assert_eq!(Publication::parse(&text), Some(value));
        assert!(Publication::parse(&(text.clone() + " ")).is_none());
        assert!(Publication::parse(&text.replace("active=0", "active=2")).is_none());
        assert!(Publication::parse(&text.replace("publication=0", "publication=00")).is_none());
    }
}
