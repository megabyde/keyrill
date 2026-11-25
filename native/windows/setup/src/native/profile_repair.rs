use super::*;
use keyrill_windows_build::Digest;
use std::collections::BTreeSet;

const LIMIT: usize = 2 * 1024 * 1024;
const HISTORY: &str = "profile-repairs.json";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Profile {
    English,
    Russian,
}
impl Profile {
    fn parse(resource: &str) -> Result<Self> {
        match resource {
            "profile/en" => Ok(Self::English),
            "profile/ru" => Ok(Self::Russian),
            _ => Err("repair is restricted to the next pending EN or RU profile".into()),
        }
    }
    fn index(self) -> usize {
        match self {
            Self::English => 0,
            Self::Russian => 1,
        }
    }
    fn resource(self) -> &'static str {
        match self {
            Self::English => "profile/en",
            Self::Russian => "profile/ru",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Prepared,
    Attempted,
    Stopped,
    Verified,
}
impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Attempted => "attempted",
            Self::Stopped => "stopped",
            Self::Verified => "verified",
        }
    }
    fn parse(value: &Value) -> Result<Self> {
        match value.as_str() {
            Some("prepared") => Ok(Self::Prepared),
            Some("attempted") => Ok(Self::Attempted),
            Some("stopped") => Ok(Self::Stopped),
            Some("verified") => Ok(Self::Verified),
            _ => Err("invalid profile repair phase".into()),
        }
    }
}

struct Approval {
    value: Value,
    digest: String,
    original: Journal,
    profile: Profile,
}

fn digest(bytes: &[u8]) -> String {
    Digest::of_bytes(bytes).to_string()
}
fn closed(value: &Value, keys: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or("repair record must be an object")?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err("repair record has missing or unknown fields".into());
    }
    Ok(())
}
fn identity<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .filter(|text| is_digest(text))
        .ok_or_else(|| "invalid repair identity".into())
}
fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    files::ordinary_ancestors(path)?;
    let mut file = keyrill_windows_build::artifact::retain(path, false)?;
    let mut bytes = Vec::new();
    use std::io::Read;
    file.by_ref()
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err("profile repair evidence exceeds bound".into());
    }
    Ok(bytes)
}
fn raw_destination(value: &Value) -> Result<Value> {
    let description = value["description"]
        .as_str()
        .ok_or("profile description absent")?;
    let icon = value["icon"].as_str().ok_or("profile icon absent")?;
    if value["index"] != 0 {
        return Err("profile destination icon index differs".into());
    }
    let raw = json!({"Description":registry::string(description),
        "IconFile":registry::string(icon),"IconIndex":registry::dword(0)});
    Ok(json!([raw, raw]))
}

impl Approval {
    fn parse(bytes: &[u8], expected: &str, original: &[u8]) -> Result<Self> {
        if bytes.len() > LIMIT || !is_digest(expected) || digest(bytes) != expected {
            return Err("profile repair approval digest differs".into());
        }
        let value: Value = serde_json::from_slice(bytes)?;
        closed(
            &value,
            &[
                "schema",
                "original",
                "desired",
                "previous",
                "cursor",
                "count",
                "profile",
                "source",
                "protected",
                "session_observation",
                "tool",
                "predecessor",
            ],
        )?;
        if value["schema"] != 1 || identity(&value, "original")? != digest(original) {
            return Err("profile repair original receipt differs".into());
        }
        let journal = Journal::parse(original)?;
        let change = journal
            .changes
            .get(journal.completed)
            .ok_or("no pending profile")?;
        let profile = Profile::parse(&change.resource)?;
        if value["profile"] != profile.resource()
            || value["desired"] != journal.desired
            || value["previous"] != json!(journal.previous)
            || value["cursor"] != journal.completed
            || value["count"] != journal.changes.len()
            || journal.desired == "absent"
        {
            return Err("profile repair does not identify the exact pending change".into());
        }
        if !value["predecessor"].is_null() {
            identity(&value, "predecessor")?;
        }
        closed(&value["tool"], &["package", "executable", "source"])?;
        for name in ["package", "executable", "source"] {
            identity(&value["tool"], name)?;
        }
        if value["tool"]["package"] == journal.desired {
            return Err("repair requires a separately verified corrected package".into());
        }
        let source = value["source"]
            .as_array()
            .filter(|v| v.len() == 2)
            .ok_or("repair requires two raw profile views")?;
        if source[0] != source[1]
            || value["source"] == raw_destination(&change.after)?
            || (!change.before.is_null() && value["source"] == raw_destination(&change.before)?)
        {
            return Err("repair source must be one exact agreed third state".into());
        }
        for view in source {
            closed(view, &["Description", "IconFile", "IconIndex"])?;
            for raw in view.as_object().ok_or("profile view missing")?.values() {
                closed(raw, &["type", "data"])?;
                if raw["type"].as_u64().is_none_or(|v| v > u32::MAX.into())
                    || raw["data"].as_array().is_none_or(|v| {
                        v.len() > 65536 || v.iter().any(|b| b.as_u64().is_none_or(|b| b > 255))
                    })
                {
                    return Err("invalid raw profile evidence".into());
                }
            }
        }
        closed(&value["protected"], &["resources", "native", "installed"])?;
        if !value["protected"]["installed"].is_null() {
            identity(&value["protected"], "installed")?;
        }
        let resources = value["protected"]["resources"]
            .as_object()
            .ok_or("repair baseline missing")?;
        let expected_resources: BTreeSet<_> = Installation::resources()
            .into_iter()
            .filter(|name| name != profile.resource())
            .collect();
        if resources.keys().cloned().collect::<BTreeSet<_>>() != expected_resources {
            return Err("repair baseline does not cover the owned resource set".into());
        }
        for (index, change) in journal.changes.iter().enumerate() {
            if index == journal.completed {
                continue;
            }
            let observed = resources
                .get(&change.resource)
                .ok_or("repair resource not owned")?;
            if *observed != change.after
                && (index < journal.completed || *observed != change.before)
            {
                return Err("another original resource conflicts with the receipt".into());
            }
        }
        Ok(Self {
            value,
            digest: expected.into(),
            original: journal,
            profile,
        })
    }
    fn destination(&self) -> &Value {
        &self.original.changes[self.original.completed].after
    }
    fn matches(&self, snapshot: &Value, desired: bool) -> Result<bool> {
        Ok(snapshot["protected"] == self.value["protected"]
            && snapshot["source"]
                == if desired {
                    raw_destination(self.destination())?
                } else {
                    self.value["source"].clone()
                })
    }
}

trait RepairIo {
    fn snapshot(&mut self, profile: Profile) -> Result<Value>;
    fn checkpoint(&mut self, phase: Phase, outcome: &Value) -> Result<()>;
    fn write_profile(&mut self, profile: Profile, destination: &Value) -> Result<()>;
}

fn execute(approval: &Approval, phase: Phase, io: &mut impl RepairIo) -> Result<()> {
    if phase == Phase::Stopped {
        return Err(
            "profile repair stopped on observed failure; inspect and approve a linked successor"
                .into(),
        );
    }
    let before = io.snapshot(approval.profile)?;
    if phase != Phase::Prepared {
        if approval.matches(&before, true)? {
            if io.snapshot(approval.profile)? != before {
                return Err("repair readback changed before verification checkpoint".into());
            }
            if phase == Phase::Verified {
                return Ok(());
            }
            io.checkpoint(
                Phase::Verified,
                &json!({"snapshot":before,"native":"not-called",
                "session_verification":"unavailable-after-restart"}),
            )?;
            return Ok(());
        }
        return Err(
            "profile repair already attempted or stopped; inspect and approve a linked successor"
                .into(),
        );
    }
    if !approval.matches(&before, false)? {
        return Err("profile repair approved baseline changed; no write permitted".into());
    }
    io.checkpoint(
        Phase::Attempted,
        &json!({"session_before":before["session"],
        "session_verification":"pending","native":"not-called"}),
    )?;
    let immediate = io.snapshot(approval.profile)?;
    if !approval.matches(&immediate, false)? || immediate["session"] != before["session"] {
        return Err("profile repair changed after the attempt marker; no write permitted".into());
    }
    let native = match io.write_profile(approval.profile, approval.destination()) {
        Ok(()) => json!({"result":"returned-success"}),
        Err(error) => json!({"result":"returned-error","error":error.to_string()}),
    };
    let after = io.snapshot(approval.profile);
    let verified = after.as_ref().is_ok_and(|snapshot| {
        approval.matches(snapshot, true).unwrap_or(false)
            && snapshot["session"] == before["session"]
    });
    let readback_error = after.as_ref().err().map(ToString::to_string);
    let session_verification = if after
        .as_ref()
        .is_ok_and(|snapshot| snapshot["session"] == before["session"])
    {
        "matched-within-call"
    } else {
        "mismatched-or-unavailable"
    };
    let outcome = json!({"native":native,"snapshot":after.unwrap_or(Value::Null),"readback_error":readback_error,
        "session_before":before["session"],"session_verification":session_verification});
    io.checkpoint(
        if verified {
            Phase::Verified
        } else {
            Phase::Stopped
        },
        &outcome,
    )?;
    if verified {
        Ok(())
    } else {
        Err("profile repair effect or protected state unknown; no replay permitted".into())
    }
}

struct Entry {
    approval: Approval,
    phase: Phase,
    outcome: Value,
}
struct Repair<'a> {
    installation: &'a mut Installation,
    entries: Vec<Entry>,
}
impl RepairIo for Repair<'_> {
    fn snapshot(&mut self, profile: Profile) -> Result<Value> {
        let approval = &self.entries.last().ok_or("repair history empty")?.approval;
        if digest(&self.installation.repair_original()?) != approval.value["original"]
            || tool_identity()? != approval.value["tool"]
        {
            return Err("repair receipt or executing tool changed".into());
        }
        self.installation.repair_snapshot(profile)
    }
    fn checkpoint(&mut self, phase: Phase, outcome: &Value) -> Result<()> {
        let last = self.entries.last_mut().ok_or("repair history empty")?;
        last.phase = phase;
        last.outcome = retained_outcome(&last.outcome, outcome);
        let records: Vec<_> = self.entries.iter().map(|entry| json!({
            "approval":entry.approval.digest,"phase":entry.phase.name(),"outcome":entry.outcome
        })).collect();
        let bytes = serde_json::to_vec_pretty(&json!({"schema":1,"records":records}))?;
        if bytes.len() > LIMIT {
            return Err("profile repair history exceeds publication bound".into());
        }
        files::atomic(&self.installation.root.join(HISTORY), &bytes)
    }
    fn write_profile(&mut self, profile: Profile, destination: &Value) -> Result<()> {
        tsf::profile_write(profile.index(), destination)
    }
}

impl Installation {
    fn repair_history(&self) -> Result<Vec<Entry>> {
        let path = self.root.join(HISTORY);
        if !path.try_exists()? {
            return Ok(Vec::new());
        }
        let value: Value = serde_json::from_slice(&read_bytes(&path)?)?;
        closed(&value, &["schema", "records"])?;
        if value["schema"] != 1 {
            return Err("unknown profile repair history".into());
        }
        let records = value["records"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 16)
            .ok_or("invalid bounded repair history")?;
        let mut entries: Vec<Entry> = Vec::new();
        for record in records {
            closed(record, &["approval", "phase", "outcome"])?;
            let id = identity(record, "approval")?;
            let bytes = read_bytes(&self.root.join(format!("profile-approval-{id}.json")))?;
            let approval_value: Value = serde_json::from_slice(&bytes)?;
            let original = identity(&approval_value, "original")?;
            let original =
                read_bytes(&self.root.join(format!("profile-original-{original}.json")))?;
            let approval = Approval::parse(&bytes, id, &original)?;
            validate_successor(&entries, &approval)?;
            let phase = Phase::parse(&record["phase"])?;
            if phase == Phase::Verified
                && !approval.matches(&record["outcome"]["snapshot"], true)?
            {
                return Err("verified repair history lacks exact destination evidence".into());
            }
            entries.push(Entry {
                approval,
                phase,
                outcome: record["outcome"].clone(),
            });
        }
        Ok(entries)
    }
    pub(super) fn repair_packages(&self) -> Result<BTreeSet<String>> {
        Ok(self
            .repair_history()?
            .into_iter()
            .map(|e| e.approval.original.desired)
            .collect())
    }
    pub(super) fn repair_gate(&mut self, selection: Option<&str>) -> Result<()> {
        let entries = self.repair_history()?;
        let Some(last) = entries.last() else {
            return Ok(());
        };
        if last.phase != Phase::Verified {
            return Err(
                "profile repair unresolved; use explicit repair-profile or inspect-profile-repair"
                    .into(),
            );
        }
        let operation = self.receipt("operation.json")?;
        if let Some(operation) = &operation {
            let original = &last.approval.original;
            if operation.desired == original.desired {
                if operation.changes != original.changes
                    || operation.previous != original.previous
                    || operation.completed < original.completed
                {
                    return Err("repaired original receipt provenance changed".into());
                }
                if operation.completed == original.completed
                    && !last
                        .approval
                        .matches(&self.repair_snapshot(last.approval.profile)?, true)?
                {
                    return Err("verified repair drifted before original recovery".into());
                }
            }
        }
        let installed = self.receipt("installed.json")?;
        for entry in &entries {
            let old = &entry.approval.original.desired;
            let selected = selection.or_else(|| operation.as_ref().map(|op| op.desired.as_str()));
            if selected == Some(old.as_str())
                && installed
                    .as_ref()
                    .is_some_and(|current| current.desired != *old)
            {
                return Err(
                    "package retained as defective repair history; selection refused".into(),
                );
            }
        }
        Ok(())
    }
    pub(super) fn repair_snapshot(&mut self, profile: Profile) -> Result<Value> {
        tsf::scope_preflight_except(Some(profile.index()))?;
        let mut resources = serde_json::Map::new();
        for resource in Self::resources() {
            if resource != profile.resource() {
                resources.insert(resource.clone(), self.read(&resource)?);
            }
        }
        let (source, mut native) = tsf::repair_snapshot(profile.index())?;
        let session = native
            .as_object_mut()
            .ok_or("native snapshot absent")?
            .remove("source")
            .ok_or("native source observation absent")?;
        let installed = self.root.join("installed.json");
        let installed = if installed.try_exists()? {
            json!(digest(&read_bytes(&installed)?))
        } else {
            Value::Null
        };
        Ok(
            json!({"source":source,"protected":{"resources":resources,"native":native,"installed":installed},"session":session}),
        )
    }
    fn repair_original(&self) -> Result<Vec<u8>> {
        let bytes = read_bytes(&self.root.join("operation.json"))?;
        let original = Journal::parse(&bytes)?;
        self.validate_receipt(&original)?;
        let next = original
            .changes
            .get(original.completed)
            .ok_or("no pending profile change")?;
        let profile = Profile::parse(&next.resource)?;
        let directory = self.root.join("versions").join(&original.desired);
        let icon = directory.join(if profile == Profile::English {
            "resources/english.ico"
        } else {
            "resources/russian.ico"
        });
        if next.after != tsf::profile_desired(profile.index(), &icon)? {
            return Err("repair destination differs from the original verified package".into());
        }
        Ok(bytes)
    }
    pub(super) fn inspect_profile_repair(&mut self) -> Result<()> {
        let original = self.repair_original()?;
        let journal = Journal::parse(&original)?;
        let profile = Profile::parse(&journal.changes[journal.completed].resource)?;
        let entries = self.repair_history()?;
        let snapshot = self.repair_snapshot(profile)?;
        let value = json!({"schema":1,"original":digest(&original),"desired":journal.desired,
            "previous":journal.previous,"cursor":journal.completed,"count":journal.changes.len(),
            "profile":profile.resource(),"source":snapshot["source"],"protected":snapshot["protected"],
            "session_observation":snapshot["session"],
            "tool":tool_identity()?,"predecessor":entries.last().map(|e| &e.approval.digest)});
        let bytes = serde_json::to_vec_pretty(&value)?;
        let approval = Approval::parse(&bytes, &digest(&bytes), &original)?;
        validate_successor(&entries, &approval)?;
        if self.repair_original()? != original || self.repair_snapshot(profile)? != snapshot {
            return Err("read-only repair inspection changed during observation".into());
        }
        println!("{}", String::from_utf8(bytes)?);
        Ok(())
    }
    pub(super) fn repair_profile(&mut self, path: &Path, expected: &str) -> Result<()> {
        files::require_elevated()?;
        let _lock = files::lock(&self.root)?;
        let original = self.repair_original()?;
        let bytes = read_bytes(path)?;
        let approval = Approval::parse(&bytes, expected, &original)?;
        self.preflight_profile(Some(approval.profile.index()))?;
        if approval.value["tool"] != tool_identity()? {
            return Err("running corrected tool differs from approved verified package".into());
        }
        let mut entries = self.repair_history()?;
        let phase = if entries
            .last()
            .is_some_and(|e| e.approval.digest == expected)
        {
            entries.last().ok_or("repair history empty")?.phase
        } else {
            validate_successor(&entries, &approval)?;
            if !approval.matches(&self.repair_snapshot(approval.profile)?, false)? {
                return Err("approved profile repair snapshot changed".into());
            }
            archive(&self.root, &original, &bytes)?;
            entries.push(Entry {
                approval: Approval::parse(&bytes, expected, &original)?,
                phase: Phase::Prepared,
                outcome: Value::Null,
            });
            let mut repair = Repair {
                installation: self,
                entries,
            };
            repair.checkpoint(Phase::Prepared, &Value::Null)?;
            entries = repair.entries;
            Phase::Prepared
        };
        let mut repair = Repair {
            installation: self,
            entries,
        };
        execute(&approval, phase, &mut repair)?;
        println!(
            "Profile and stable protected-state readback verified. Run ordinary recover with this corrected Setup."
        );
        if repair.entries.last().ok_or("repair history empty")?.outcome["session_verification"]
            == "unavailable-after-restart"
        {
            println!(
                "The original tool thread ended. Its source preservation is unavailable after restart; the retained outcome records this limitation."
            );
        }
        Ok(())
    }
}

fn validate_successor(entries: &[Entry], approval: &Approval) -> Result<()> {
    if entries.len() >= 16 || entries.iter().any(|e| e.approval.digest == approval.digest) {
        return Err("repair attempt identity repeated or history exhausted".into());
    }
    match entries.last() {
        None if approval.value["predecessor"].is_null() => Ok(()),
        Some(last)
            if matches!(last.phase, Phase::Attempted | Phase::Stopped)
                && approval.value["predecessor"] == last.approval.digest
                && approval.value["original"] == last.approval.value["original"]
                && approval.profile == last.approval.profile =>
        {
            Ok(())
        }
        _ => Err("new repair requires an explicit stopped predecessor link".into()),
    }
}

fn tool_identity() -> Result<Value> {
    tool_identity_at(&std::env::current_exe()?)
}
fn tool_identity_at(executable: &Path) -> Result<Value> {
    let executable = executable.canonicalize()?;
    let directory = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("setup package parent absent")?;
    let package = Package::open(directory)?;
    let target = if cfg!(target_arch = "x86") {
        Target::X86
    } else {
        Target::X64
    };
    let name = format!("{}/KeyrillSetup.exe", target.name());
    if directory.join(&name).canonicalize()? != executable {
        return Err("profile repair must run from its verified candidate package".into());
    }
    let mut retained = keyrill_windows_build::artifact::retain(&executable, false)?;
    package.verify_file(&name, &mut retained)?;
    Ok(
        json!({"package":package.digest(),"executable":package.file_digest(&name),"source":package.production(target)}),
    )
}

fn archive(root: &Path, original: &[u8], approval: &[u8]) -> Result<()> {
    for (name, data) in [
        (
            format!("profile-original-{}.json", digest(original)),
            original,
        ),
        (
            format!("profile-approval-{}.json", digest(approval)),
            approval,
        ),
    ] {
        let path = root.join(name);
        if path.try_exists()? {
            if read_bytes(&path)? != data {
                return Err("immutable repair evidence differs".into());
            }
        } else {
            files::create(root, &path, data)?;
        }
    }
    Ok(())
}

fn retained_outcome(previous: &Value, outcome: &Value) -> Value {
    let mut outcome = outcome.clone();
    if outcome.get("session_before").is_none() && previous.get("session_before").is_some() {
        outcome["session_before"] = previous["session_before"].clone();
    }
    outcome
}

#[cfg(test)]
#[path = "profile_repair_tests.rs"]
mod tests;
