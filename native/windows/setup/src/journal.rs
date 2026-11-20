//! Durable before/after receipts, with no compensating writes after unknown effects.
use crate::Result;
use serde_json::{Value, json};

/// One exact owned resource and its observed and desired values.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// Closed resource identifier interpreted by the native backend.
    pub resource: String,
    /// Freshly observed value before any mutation.
    pub before: Value,
    /// Exact intended readback after the mutation.
    pub after: Value,
}

/// A persisted operation whose cursor records verified acknowledgements only.
#[derive(Clone, Debug, PartialEq)]
pub struct Journal {
    /// Installed package identity, or `absent` for uninstall.
    pub desired: String,
    /// Previous installed package identity, when known.
    pub previous: Option<String>,
    /// Ordered changes with immutable before/after observations.
    pub changes: Vec<Change>,
    /// Number of freshly verified changes.
    pub completed: usize,
}

/// Foreign boundaries used by recovery and by isolated failure schedules.
pub trait Backend {
    /// Read one exact resource without mutating it.
    ///
    /// # Errors
    /// Returns an error when current state cannot be established.
    fn read(&mut self, resource: &str) -> Result<Value>;
    /// Attempt one mutation; a failure may have effects.
    ///
    /// # Errors
    /// The caller always reads back, including after an error.
    fn write(&mut self, resource: &str, value: &Value) -> Result<()>;
    /// Durably checkpoint the complete journal before advancing.
    ///
    /// # Errors
    /// Failure stops all further mutation.
    fn persist(&mut self, journal: &Journal) -> Result<()>;
}

impl Journal {
    /// Serialize an exact, versioned receipt.
    #[must_use]
    pub fn value(&self) -> Value {
        json!({"schema":1,"scope":"machine","desired":self.desired,"previous":self.previous,
            "completed":self.completed,"changes":self.changes.iter().map(|c|
                json!({"resource":c.resource,"before":c.before,"after":c.after})).collect::<Vec<_>>()})
    }

    /// Decode bounded journal state. Resource authorization remains the native backend's duty.
    ///
    /// # Errors
    /// Returns an error for unknown schema, malformed or excessive state.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("installation receipt too large".into());
        }
        let value: Value = serde_json::from_slice(bytes)?;
        if value["schema"] != 1 || value["scope"] != "machine" {
            return Err("unsupported installation receipt".into());
        }
        let desired = value["desired"]
            .as_str()
            .ok_or("receipt desired state absent")?
            .to_owned();
        if desired != "absent" && !keyrill_windows_build::artifact::is_digest(&desired) {
            return Err("receipt package invalid".into());
        }
        let previous = if value["previous"].is_null() {
            None
        } else {
            Some(
                value["previous"]
                    .as_str()
                    .filter(|v| keyrill_windows_build::artifact::is_digest(v))
                    .ok_or("receipt previous package invalid")?
                    .to_owned(),
            )
        };
        let records = value["changes"]
            .as_array()
            .filter(|v| v.len() <= 128)
            .ok_or("receipt changes invalid")?;
        let mut names = std::collections::BTreeSet::new();
        let mut changes = Vec::new();
        for record in records {
            let resource = record["resource"]
                .as_str()
                .filter(|v| v.len() <= 128)
                .ok_or("receipt resource invalid")?
                .to_owned();
            if !names.insert(resource.clone())
                || record.get("before").is_none()
                || record.get("after").is_none()
            {
                return Err("receipt duplicate or incomplete resource".into());
            }
            changes.push(Change {
                resource,
                before: record["before"].clone(),
                after: record["after"].clone(),
            });
        }
        let completed = usize::try_from(
            value["completed"]
                .as_u64()
                .ok_or("receipt cursor invalid")?,
        )?;
        if completed > changes.len() {
            return Err("receipt cursor exceeds changes".into());
        }
        Ok(Self {
            desired,
            previous,
            changes,
            completed,
        })
    }

    /// Reconcile fresh before/after observations. Never infer a write from an API result.
    ///
    /// # Errors
    /// Stops on drift, unavailable readback, failed persistence or a known no-effect failure.
    /// The persisted receipt remains suitable for an explicit retry.
    pub fn reconcile(&mut self, backend: &mut impl Backend) -> Result<()> {
        for (index, change) in self.changes.iter().enumerate() {
            let observed = backend.read(&change.resource)?;
            if observed != change.after && (index < self.completed || observed != change.before) {
                return Err(
                    "installation conflict: resource differs from both receipt states".into(),
                );
            }
        }
        backend.persist(self)?;
        while self.completed < self.changes.len() {
            let change = &self.changes[self.completed];
            let observed = backend.read(&change.resource)?;
            if observed != change.after {
                if observed != change.before {
                    return Err("installation conflict before mutation".into());
                }
                let attempted = backend.write(&change.resource, &change.after);
                let actual = backend.read(&change.resource).map_err(
                    |_| "installation unknown effect: readback unavailable; receipt retained",
                )?;
                if actual != change.after {
                    return Err(if actual == change.before {
                        attempted.err().unwrap_or_else(|| {
                            "installation mutation had no verified effect".into()
                        })
                    } else {
                        "installation unknown effect: readback differs from both states; receipt retained".into()
                    });
                }
            }
            self.completed += 1;
            backend.persist(self)?;
        }
        for change in &self.changes {
            if backend.read(&change.resource)? != change.after {
                return Err("installation changed during final readback".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Fixture {
        state: BTreeMap<String, Value>,
        saved: Option<Journal>,
        calls: usize,
        fail: usize,
        after_effect: bool,
        writes: usize,
    }
    impl Fixture {
        fn tick(&mut self) -> Result<()> {
            self.calls += 1;
            if self.calls == self.fail {
                Err("injected interruption".into())
            } else {
                Ok(())
            }
        }
    }
    impl Backend for Fixture {
        fn read(&mut self, name: &str) -> Result<Value> {
            self.tick()?;
            Ok(self.state.get(name).cloned().unwrap_or(Value::Null))
        }
        fn write(&mut self, name: &str, value: &Value) -> Result<()> {
            if !self.after_effect {
                self.tick()?;
            }
            self.state.insert(name.into(), value.clone());
            self.writes += 1;
            if self.after_effect {
                self.tick()?;
            }
            Ok(())
        }
        fn persist(&mut self, journal: &Journal) -> Result<()> {
            self.tick()?;
            self.saved = Some(journal.clone());
            Ok(())
        }
    }
    fn journal() -> Journal {
        Journal {
            desired: "a".repeat(64),
            previous: None,
            completed: 0,
            changes: (0..4)
                .map(|i| Change {
                    resource: format!("owned-{i}"),
                    before: Value::Null,
                    after: json!(i),
                })
                .collect(),
        }
    }
    #[test]
    fn recovery_at_every_boundary_never_replays_a_completed_effect() {
        for after_effect in [false, true] {
            for fail in 1..=28 {
                let mut backend = Fixture {
                    state: BTreeMap::from([("unrelated".into(), json!(42))]),
                    saved: None,
                    calls: 0,
                    fail,
                    after_effect,
                    writes: 0,
                };
                let mut operation = journal();
                let _ = operation.reconcile(&mut backend);
                backend.fail = usize::MAX;
                let mut recovered = backend.saved.clone().unwrap_or_else(journal);
                recovered.reconcile(&mut backend).unwrap();
                assert_eq!(
                    backend.writes, 4,
                    "failure {fail}, after effect {after_effect}"
                );
                assert_eq!(backend.state["unrelated"], json!(42));
                let before = backend.writes;
                recovered.reconcile(&mut backend).unwrap();
                assert_eq!(backend.writes, before);
                assert_eq!(
                    Journal::parse(&serde_json::to_vec(&recovered.value()).unwrap()).unwrap(),
                    recovered
                );
            }
        }
    }
    #[test]
    fn drift_refuses_all_writes_and_preserves_unrelated_state() {
        let mut backend = Fixture {
            state: BTreeMap::from([("owned-3".into(), json!("foreign"))]),
            saved: None,
            calls: 0,
            fail: usize::MAX,
            after_effect: false,
            writes: 0,
        };
        assert!(journal().reconcile(&mut backend).is_err());
        assert_eq!(backend.writes, 0);
        assert_eq!(backend.state["owned-3"], json!("foreign"));
    }
}
