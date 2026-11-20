//! Owned shutdown evidence, retaining the diagnostic endpoint through deactivation.
use crate::diagnostics::{E_PENDING, Endpoint, ServiceSnapshot};
use std::cell::RefCell;
use windows::Win32::Foundation::S_OK;
use windows::Win32::UI::TextServices::{ITfThreadMgr, TF_E_NOPROVIDER};
use windows::core::HRESULT;
/// Control ID of this fixture mode row.
pub const MODE_ID: i32 = 108;
/// Exact native title identifying this fixture mode.
pub const TITLE: &str = "Keyrill TSF owned shutdown fixture";
/// Canonical mode-identification status row.
pub const MODE_STATUS: &str = "Keyrill owned shutdown v1; mode=ProveOwnedShutdown";
#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Owned-shutdown result, using the established process exit codes.
pub enum Verdict {
    /// All required proof predicates were observed.
    Verified = 0,
    /// No watched owned mark was captured before shutdown.
    NotCaptured = 10,
    /// The requested native object or publication is absent.
    Missing = 11,
    /// The native operation or publication remains in flight.
    Busy = 12,
    /// The snapshot or lifecycle evidence failed validation.
    InvalidEvidence = 13,
    /// The active diagnostic provider changed.
    ProviderChanged = 14,
    /// The active service activation changed.
    ActivationChanged = 15,
    /// The snapshot did not establish a current owned mark.
    NoOwnedMark = 16,
    /// The retained lifecycle history did not cover the interval.
    HistoryLost = 17,
    /// The service reported an unknown editing effect.
    UnknownEffect = 18,
    /// The required terminal event before deactivation was not observed.
    NoTerminalEvent = 19,
    /// The lifecycle history reported detachment.
    Detached = 20,
    /// Thread-manager deactivation did not return `S_OK`.
    DeactivationFailed = 21,
}
/// Check active service evidence and reject unknown editing effects.
pub fn active(snapshot: &ServiceSnapshot) -> Verdict {
    if !snapshot.valid()
        || snapshot.saturated()
        || snapshot.base[1] == 0
        || snapshot.base[0] == 0
        || snapshot.base[0] == u32::MAX
        || snapshot.base[2] != 0
    {
        return Verdict::InvalidEvidence;
    }
    if snapshot.base[20] != 0 {
        Verdict::UnknownEffect
    } else {
        Verdict::Verified
    }
}
/// Require a current active service and a nonsaturated owned composition serial.
pub fn capturable(snapshot: &ServiceSnapshot) -> Verdict {
    let verdict = active(snapshot);
    if verdict != Verdict::Verified {
        return verdict;
    }
    if snapshot.lifecycle[4] != 0 && snapshot.lifecycle[5] != 0 && snapshot.lifecycle[5] != u32::MAX
    {
        Verdict::Verified
    } else {
        Verdict::NoOwnedMark
    }
}
/// Evaluate the complete retained lifecycle history for this proof outcome.
pub fn evaluate(before: &ServiceSnapshot, after: &ServiceSnapshot) -> Verdict {
    let captured = capturable(before);
    if captured != Verdict::Verified {
        return captured;
    }
    if !after.valid()
        || after.saturated()
        || after.base[1] != 0
        || after.lifecycle[4] != 0
        || after.lifecycle[5] != 0
    {
        return Verdict::InvalidEvidence;
    }
    if before.base[0] != after.base[0] {
        return Verdict::ActivationChanged;
    }
    if after.base[20] != 0 {
        return Verdict::UnknownEffect;
    }
    if before.base[9..47]
        .iter()
        .zip(&after.base[9..47])
        .any(|(a, b)| b < a)
        || before.lifecycle[1] != after.lifecycle[1]
    {
        return Verdict::InvalidEvidence;
    }
    if !crate::diagnostics::history(before, after) {
        return Verdict::HistoryLost;
    }
    let (mut requested, mut terminal, mut deactivated) = (false, false, false);
    for event in after
        .events()
        .filter(|event| event[0] > before.lifecycle[0])
    {
        match event[1] {
            1 | 7 => return Verdict::InvalidEvidence,
            9 => return Verdict::Detached,
            8 if event[3] == 8 => {
                if !deactivated && !terminal {
                    return Verdict::NoTerminalEvent;
                }
                deactivated = true;
            }
            _ => {}
        }
        if event[2] != before.lifecycle[5] || deactivated {
            continue;
        }
        if event[1] == 2 {
            requested = true;
        }
        if event[1] == 4 || (event[1] == 3 && requested) {
            terminal = true;
        }
    }
    if terminal && deactivated {
        Verdict::Verified
    } else {
        Verdict::NoTerminalEvent
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Operation {
    Binding,
    Observing,
    Capturing,
    Finishing,
}
#[expect(
    clippy::large_enum_variant,
    reason = "The proof moves fixed-size wire evidence out of state before each native callback."
)]
enum State {
    Awaiting,
    InFlight(Operation),
    Watching(Endpoint, u32),
    Closing(Endpoint, ServiceSnapshot),
    Rejected(Verdict),
    Completed(Verdict),
}
/// Reentrant fixture proof state with capabilities moved out around native calls.
pub struct Probe {
    state: RefCell<State>,
}
impl Default for Probe {
    fn default() -> Self {
        Self {
            state: RefCell::new(State::Awaiting),
        }
    }
}
impl Probe {
    fn running(&self, operation: Operation) -> bool {
        matches!(*self.state.borrow(), State::InFlight(now) if operation == now)
    }
    fn reject(&self, verdict: Verdict) {
        if matches!(
            *self.state.borrow(),
            State::Rejected(_) | State::Completed(_)
        ) {
            return;
        }
        let old = self.state.replace(State::Rejected(verdict));
        drop(old);
    }
    /// Bind or revalidate the active service while awaiting fixture closure.
    pub fn observe(&self, manager: &ITfThreadMgr) {
        if !matches!(*self.state.borrow(), State::Awaiting | State::Watching(..)) {
            return;
        }
        let binding = matches!(*self.state.borrow(), State::Awaiting);
        let operation = if binding {
            Operation::Binding
        } else {
            Operation::Observing
        };
        let old = self.state.replace(State::InFlight(operation));
        let result = match old {
            State::Watching(endpoint, activation) => {
                match endpoint.same_provider_with(manager, || self.running(operation)) {
                    Ok(true) if self.running(operation) => endpoint
                        .copy()
                        .map(|snapshot| (endpoint, activation, snapshot)),
                    Ok(true) => return,
                    Ok(false) => {
                        self.reject(Verdict::ProviderChanged);
                        return;
                    }
                    Err(error) => Err(error),
                }
            }
            _ => {
                Endpoint::discover_with(manager, || self.running(operation)).and_then(|endpoint| {
                    if self.running(operation) {
                        endpoint.copy().map(|snapshot| (endpoint, 0, snapshot))
                    } else {
                        Err(windows::Win32::Foundation::E_ABORT)
                    }
                })
            }
        };
        if !self.running(operation) {
            return;
        }
        let (endpoint, activation, snapshot) = match result {
            Ok(result) => result,
            Err(error) if binding && error == TF_E_NOPROVIDER => {
                self.state.replace(State::Awaiting);
                return;
            }
            Err(error) => {
                self.reject(if error == E_PENDING {
                    Verdict::Busy
                } else if error == TF_E_NOPROVIDER {
                    Verdict::Missing
                } else {
                    Verdict::InvalidEvidence
                });
                return;
            }
        };
        let verdict = active(&snapshot);
        if verdict != Verdict::Verified {
            self.reject(verdict);
            return;
        }
        if activation != 0 && snapshot.base[0] != activation {
            self.reject(Verdict::ActivationChanged);
            return;
        }
        self.state
            .replace(State::Watching(endpoint, snapshot.base[0]));
    }
    /// Capture the owned mark and retain its endpoint before native shutdown.
    pub fn capture_close(&self, manager: &ITfThreadMgr) {
        if matches!(
            *self.state.borrow(),
            State::Rejected(_) | State::Completed(_)
        ) {
            return;
        }
        let old = self.state.replace(State::InFlight(Operation::Capturing));
        let State::Watching(endpoint, activation) = old else {
            self.reject(Verdict::NotCaptured);
            return;
        };
        match endpoint.same_provider_with(manager, || self.running(Operation::Capturing)) {
            Ok(true) => {}
            Ok(false) => {
                self.reject(Verdict::ProviderChanged);
                return;
            }
            Err(_) => {
                self.reject(Verdict::InvalidEvidence);
                return;
            }
        }
        if !self.running(Operation::Capturing) {
            return;
        }
        let snapshot = match endpoint.copy() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.reject(if error == E_PENDING {
                    Verdict::Busy
                } else {
                    Verdict::InvalidEvidence
                });
                return;
            }
        };
        if !self.running(Operation::Capturing) {
            return;
        }
        let verdict = capturable(&snapshot);
        if verdict != Verdict::Verified {
            self.reject(verdict);
            return;
        }
        if snapshot.base[0] != activation {
            self.reject(Verdict::ActivationChanged);
            return;
        }
        self.state.replace(State::Closing(endpoint, snapshot));
    }
    /// Evaluate retained service evidence after thread-manager deactivation.
    pub fn finish(&self, deactivation: HRESULT) -> Verdict {
        if let State::Completed(verdict) = *self.state.borrow() {
            return verdict;
        }
        let old = self.state.replace(State::InFlight(Operation::Finishing));
        let mut verdict = match old {
            State::Rejected(verdict) => verdict,
            State::Closing(endpoint, before) => {
                let verdict = if deactivation != S_OK {
                    Verdict::DeactivationFailed
                } else {
                    match endpoint.copy() {
                        Ok(after) => evaluate(&before, &after),
                        Err(error) if error == E_PENDING => Verdict::Busy,
                        Err(_) => Verdict::InvalidEvidence,
                    }
                };
                drop(endpoint);
                verdict
            }
            _ => Verdict::NotCaptured,
        };
        if let State::Rejected(rejected) = *self.state.borrow() {
            verdict = rejected;
        }
        self.state.replace(State::Completed(verdict));
        verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owned() -> ServiceSnapshot {
        let mut snapshot = ServiceSnapshot::default();
        snapshot.base[0] = 7;
        snapshot.base[1] = 1;
        snapshot.lifecycle[..11].copy_from_slice(&[1, 1, 0, 1, 1, 1, 1, 1, 1, 0, 0]);
        snapshot
    }
    #[test]
    fn shutdown_requires_exact_terminal_before_deactivation() {
        let before = owned();
        let mut after = before.clone();
        after.base[1] = 0;
        after.lifecycle[..6].copy_from_slice(&[3, 1, 0, 3, 0, 0]);
        after.lifecycle[11..21].copy_from_slice(&[2, 4, 1, 0, 0, 3, 8, 0, 8, 0]);
        assert_eq!(evaluate(&before, &after), Verdict::Verified);
        after.lifecycle[12] = 9;
        assert_eq!(evaluate(&before, &after), Verdict::Detached);
        after.lifecycle[12] = 4;
        after.base[20] = 1;
        assert_eq!(evaluate(&before, &after), Verdict::UnknownEffect);
    }
}
