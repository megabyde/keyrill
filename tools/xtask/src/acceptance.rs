//! Required Windows acceptance inventory and local result-ledger consistency validation.
//!
//! A matching ledger establishes complete, candidate-bound records. Its assertions still need
//! independent artifact review and physical application evidence before product acceptance.

use keyrill_windows_build::artifact::{Package, is_digest};
use keyrill_windows_build::{BuildConfig, Digest, Instrumentation, Target, identity};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs::File;
use std::io::Read as _;
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MANIFEST: &str = "native/windows/proof/acceptance.json";
const LIMIT: u64 = 4 * 1024 * 1024;
const TARGETS: [&str; 2] = ["x86_64-pc-windows-msvc", "i686-pc-windows-msvc"];
const APPLICATIONS: [(&str, &str, &str); 7] = [
    ("notepad", "Notepad", "automatic"),
    ("chrome", "Chrome", "automatic"),
    ("edge", "Edge", "automatic"),
    ("firefox", "Firefox", "automatic"),
    ("vscode", "VS Code", "manual"),
    ("word", "Word", "manual"),
    ("terminal", "Windows Terminal", "manual"),
];
const DOCUMENT: [&str; 8] = [
    "whole-public-document",
    "prefix",
    "suffix",
    "caret",
    "selection",
    "focus",
    "owned-composition",
    "input-order",
];
const EXCLUSION: [&str; 6] = [
    "metadata-only",
    "zero-character-reads",
    "zero-composition-starts",
    "zero-character-writes",
    "input-pass-through",
    "focus",
];

/// One expanded acceptance obligation with a stable slash-separated key.
#[derive(Debug)]
pub(crate) struct Requirement {
    pub(crate) application: String,
    pub(crate) target: Option<&'static str>,
    pub(crate) evidence: String,
    pub(crate) mode: String,
    pub(crate) expected: String,
    pub(crate) oracles: BTreeSet<String>,
}

/// The validated product acceptance matrix, separate from the C++ successor ledger.
#[derive(Debug)]
pub(crate) struct Inventory {
    pub(crate) requirements: BTreeMap<String, Requirement>,
}

fn fields(value: &Value, expected: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or("acceptance object required")?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err("acceptance object has missing or unknown fields".into());
    }
    Ok(())
}

fn text(value: &Value) -> Result<&str> {
    let text = value.as_str().ok_or("acceptance string required")?;
    if text.is_empty() || text.len() > 1024 || text.chars().any(char::is_control) {
        return Err("acceptance string outside bounds".into());
    }
    Ok(text)
}

fn array(value: &Value) -> Result<&[Value]> {
    let array = value.as_array().ok_or("acceptance array required")?;
    if array.len() > 4096 {
        return Err("acceptance array outside bounds".into());
    }
    Ok(array)
}

fn strings(value: &Value) -> Result<BTreeSet<String>> {
    let mut result = BTreeSet::new();
    for item in array(value)? {
        if !result.insert(text(item)?.to_owned()) {
            return Err("duplicate acceptance string".into());
        }
    }
    Ok(result)
}

fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn exact_strings(value: &Value, expected: &[&str]) -> Result<()> {
    if strings(value)? != set(expected) {
        return Err("acceptance required set differs".into());
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    parse_json(&read_bounded(path)?)
}

enum JsonContainer {
    Object {
        keys: BTreeSet<String>,
        awaiting_key: bool,
    },
    Array,
}

fn parse_json(bytes: &[u8]) -> Result<Value> {
    let value = serde_json::from_slice(bytes)?;
    // Value normalizes repeated object keys, so inspect the valid token stream before use.
    let mut containers = Vec::new();
    let mut cursor = 0;
    while let Some(byte) = bytes.get(cursor) {
        match byte {
            b'{' => containers.push(JsonContainer::Object {
                keys: BTreeSet::new(),
                awaiting_key: true,
            }),
            b'[' => containers.push(JsonContainer::Array),
            b'}' | b']' => {
                containers.pop();
            }
            b',' => {
                if let Some(JsonContainer::Object { awaiting_key, .. }) = containers.last_mut() {
                    *awaiting_key = true;
                }
            }
            b'"' => {
                let start = cursor;
                cursor += 1;
                while let Some(byte) = bytes.get(cursor) {
                    match byte {
                        b'\\' => cursor += 2,
                        b'"' => break,
                        _ => cursor += 1,
                    }
                }
                if let Some(JsonContainer::Object { keys, awaiting_key }) = containers.last_mut()
                    && *awaiting_key
                {
                    let key: String = serde_json::from_slice(&bytes[start..=cursor])?;
                    if !keys.insert(key) {
                        return Err("duplicate acceptance JSON object key".into());
                    }
                    *awaiting_key = false;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    Ok(value)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err("acceptance input exceeds size bound".into());
    }
    Ok(bytes)
}

fn scenario_contracts() -> BTreeMap<&'static str, Vec<(&'static str, &'static str)>> {
    BTreeMap::from([
        (
            "ordinary",
            vec![
                ("auto-en-ru", "automatic"),
                ("auto-ru-en", "automatic"),
                ("manual-k", "manual"),
                ("retained-z", "manual"),
                ("escape-reversal", "automatic"),
                ("rapid-continuation", "automatic"),
                ("focus-round-trip", "manual"),
                ("external-source-change", "manual"),
                ("automatic-source-switch", "automatic"),
                ("owned-close", "manual"),
                ("empty-close", "manual"),
                ("selection-navigation", "manual"),
                ("keyboard-edges", "manual"),
                ("held-commands", "manual"),
            ],
        ),
        ("editor", vec![("native-undo", "automatic")]),
        ("browser", vec![("renderer-tab-change", "manual")]),
        ("vscode", vec![("completion-snippet-window", "manual")]),
        ("word", vec![("formatting-preservation", "manual")]),
        (
            "terminal",
            vec![
                ("child-byte-order", "manual"),
                ("terminal-command-output", "manual"),
            ],
        ),
        ("excluded", vec![("pass-through", "automatic")]),
        ("terminal-excluded", vec![("secret-no-echo", "manual")]),
        ("application-off", vec![("ordinary-field-off", "off")]),
        (
            "faults",
            vec![
                ("denied-edit-session", "manual"),
                ("rejected-composition", "manual"),
                ("stale-client-generation", "manual"),
                ("foreign-mark-range", "manual"),
                ("unknown-write-effect", "manual"),
                ("reentrant-exclusion", "manual"),
                ("observer-wrong-identity", "off"),
                ("observer-stale-unavailable", "off"),
                ("source-echo-reentry", "automatic"),
            ],
        ),
    ])
}

fn control_contracts() -> BTreeMap<String, BTreeSet<String>> {
    let mut controls = BTreeMap::new();
    for (application, ordinary, excluded, extra) in [
        (
            "proof-editor",
            vec!["document"],
            vec!["read-only", "password"],
            "editor",
        ),
        ("notepad", vec!["document"], vec!["find"], "application-off"),
        (
            "chrome",
            vec!["text-input", "textarea", "contenteditable"],
            vec!["password", "read-only", "sensitive"],
            "browser",
        ),
        (
            "edge",
            vec!["text-input", "textarea", "contenteditable"],
            vec!["password", "read-only", "sensitive"],
            "browser",
        ),
        (
            "firefox",
            vec!["text-input", "textarea", "contenteditable"],
            vec!["password", "read-only", "sensitive"],
            "browser",
        ),
        (
            "vscode",
            vec!["editor", "search"],
            vec!["integrated-terminal", "read-only"],
            "vscode",
        ),
        (
            "word",
            vec!["body", "table-cell"],
            vec!["protected-document"],
            "word",
        ),
    ] {
        for control in ordinary {
            controls.insert(
                format!("{application}/{control}"),
                set(&["ordinary", "editor", extra]),
            );
        }
        for control in excluded {
            controls.insert(format!("{application}/{control}"), set(&["excluded"]));
        }
    }
    controls.insert(
        "terminal/ordinary-input".into(),
        set(&["ordinary", "terminal"]),
    );
    controls.insert(
        "terminal/secret-no-echo".into(),
        set(&["terminal-excluded"]),
    );
    controls.insert("proof-provider/native-boundaries".into(), set(&["faults"]));
    controls
}

fn validate_manifest(manifest: &Value) -> Result<Inventory> {
    fields(
        manifest,
        &[
            "schema",
            "purpose",
            "platform",
            "nativeTargets",
            "evidenceKinds",
            "applications",
            "oracleSets",
            "scenarioGroups",
            "controls",
        ],
    )?;
    if manifest["schema"] != 1 || manifest["platform"] != "Windows 11 Intel/AMD" {
        return Err("unsupported Windows acceptance manifest".into());
    }
    text(&manifest["purpose"])?;
    exact_strings(&manifest["nativeTargets"], &TARGETS)?;
    fields(
        &manifest["evidenceKinds"],
        &["controlled", "generated", "physical", "historical"],
    )?;
    for description in manifest["evidenceKinds"]
        .as_object()
        .ok_or("evidence kinds missing")?
        .values()
    {
        text(description)?;
    }
    let mut applications = BTreeSet::new();
    for application in array(&manifest["applications"])? {
        fields(application, &["id", "name", "defaultMode"])?;
        let id = text(&application["id"])?;
        if !applications.insert(id)
            || !APPLICATIONS.iter().any(|(expected_id, name, mode)| {
                id == *expected_id
                    && application["name"] == *name
                    && application["defaultMode"] == *mode
            })
        {
            return Err("Windows acceptance application scope differs".into());
        }
    }
    if applications.len() != APPLICATIONS.len() {
        return Err("Windows acceptance application scope incomplete".into());
    }
    fields(
        &manifest["oracleSets"],
        &[
            "document",
            "exclusion",
            "fault",
            "formatted-document",
            "terminal-bytes",
        ],
    )?;
    exact_strings(&manifest["oracleSets"]["document"], &DOCUMENT)?;
    exact_strings(&manifest["oracleSets"]["exclusion"], &EXCLUSION)?;
    let mut fault_oracles = DOCUMENT.to_vec();
    fault_oracles.extend(["one-write-per-plan", "no-replay", "no-guessed-repair"]);
    exact_strings(&manifest["oracleSets"]["fault"], &fault_oracles)?;
    let mut formatted = DOCUMENT.to_vec();
    formatted.push("formatting");
    exact_strings(&manifest["oracleSets"]["formatted-document"], &formatted)?;
    exact_strings(
        &manifest["oracleSets"]["terminal-bytes"],
        &[
            "caret",
            "selection",
            "focus",
            "owned-composition",
            "input-order",
            "committed-bytes",
            "native-command-baseline",
            "output-order",
            "ordinary-input-authority",
            "no-echo-distinction",
        ],
    )?;
    validate_groups(manifest)?;
    expand_controls(manifest)
}

fn validate_groups(manifest: &Value) -> Result<()> {
    let contracts = scenario_contracts();
    let groups = &manifest["scenarioGroups"];
    fields(groups, &contracts.keys().copied().collect::<Vec<_>>())?;
    for (name, expected) in contracts {
        let group = &groups[name];
        fields(group, &["evidence", "oracles", "scenarios"])?;
        let (evidence, oracle): (&[_], _) = match name {
            "faults" => (&["controlled"], "fault"),
            "terminal" => (&["physical"], "terminal-bytes"),
            "terminal-excluded" => (&["physical"], "exclusion"),
            "excluded" | "application-off" => (&["generated", "physical"], "exclusion"),
            "word" => (&["generated", "physical"], "formatted-document"),
            _ => (&["generated", "physical"], "document"),
        };
        exact_strings(&group["evidence"], evidence)?;
        if group["oracles"] != oracle {
            return Err("acceptance scenario oracle differs".into());
        }
        let mut actual = BTreeSet::new();
        for scenario in array(&group["scenarios"])? {
            fields(scenario, &["id", "mode", "expected"])?;
            let id = text(&scenario["id"])?;
            let mode = text(&scenario["mode"])?;
            text(&scenario["expected"])?;
            if !actual.insert((id, mode)) {
                return Err("duplicate acceptance scenario".into());
            }
        }
        if actual != expected.into_iter().collect() {
            return Err("acceptance scenario coverage differs".into());
        }
    }
    Ok(())
}

fn expand_controls(manifest: &Value) -> Result<Inventory> {
    let contracts = control_contracts();
    let mut controls = BTreeSet::new();
    let mut requirements = BTreeMap::new();
    for control in array(&manifest["controls"])? {
        let application = text(&control["application"])?;
        let id = format!("{application}/{}", text(&control["id"])?);
        let mut expected_fields = vec!["application", "id", "architectures", "groups"];
        let override_oracles = if id == "terminal/ordinary-input" {
            expected_fields.push("oracleOverride");
            if control["oracleOverride"] != "terminal-bytes" {
                return Err(
                    "Terminal acceptance requires owned-range and child-byte oracles".into(),
                );
            }
            Some("terminal-bytes")
        } else {
            None
        };
        fields(control, &expected_fields)?;
        let groups = strings(&control["groups"])?;
        if !controls.insert(id.clone()) || contracts.get(&id) != Some(&groups) {
            return Err("acceptance control inventory differs".into());
        }
        let targets: Vec<Option<&'static str>> = if application.starts_with("proof-") {
            if control["architectures"] != "both" {
                return Err("controlled native evidence requires both architectures".into());
            }
            TARGETS.into_iter().map(Some).collect()
        } else {
            if control["architectures"] != "installed" {
                return Err("application architecture requires installed metadata".into());
            }
            vec![None]
        };
        for name in groups {
            let group = &manifest["scenarioGroups"][&name];
            let oracle_name = override_oracles.unwrap_or(text(&group["oracles"])?);
            let oracles = strings(&manifest["oracleSets"][oracle_name])?;
            for scenario in array(&group["scenarios"])? {
                for evidence in strings(&group["evidence"])? {
                    // Terminal input is maintainer-driven under the computer-use contract.
                    if application == "terminal" && evidence == "generated" {
                        continue;
                    }
                    for target in &targets {
                        let key = format!(
                            "{id}/{}/{evidence}/{}",
                            text(&scenario["id"])?,
                            target.unwrap_or("installed")
                        );
                        let requirement = Requirement {
                            application: application.into(),
                            target: *target,
                            evidence: evidence.clone(),
                            mode: text(&scenario["mode"])?.into(),
                            expected: text(&scenario["expected"])?.into(),
                            oracles: oracles.clone(),
                        };
                        if requirements.insert(key, requirement).is_some() {
                            return Err("duplicate expanded acceptance requirement".into());
                        }
                    }
                }
            }
        }
    }
    if controls.len() != contracts.len() {
        return Err("acceptance control inventory incomplete".into());
    }
    Ok(Inventory { requirements })
}

/// Load and strictly validate the static acceptance inventory without desktop observations.
///
/// # Errors
/// Returns bounded JSON, schema, scope, uniqueness or coverage errors.
pub(crate) fn inventory(root: &Path) -> Result<Inventory> {
    validate_manifest(&read_json(&root.join(MANIFEST))?)
}

fn candidate_identities(root: &Path) -> Result<BTreeMap<String, (String, String)>> {
    let identities: BTreeMap<String, (String, String)> = TARGETS
        .into_iter()
        .map(|target| {
            let build = identity(
                root,
                BuildConfig {
                    target,
                    profile: "release",
                    instrumentation: Instrumentation::None,
                },
            )?;
            Ok((
                target.into(),
                (build.source.to_string(), build.production.to_string()),
            ))
        })
        .collect::<Result<_>>()?;
    let sources: BTreeSet<_> = identities.values().map(|identity| &identity.0).collect();
    if sources.len() != 1 {
        return Err("acceptance source changed while resolving native identities".into());
    }
    Ok(identities)
}

fn positive(value: &Value) -> Result<u64> {
    value
        .as_u64()
        .filter(|number| *number > 0)
        .ok_or_else(|| "positive acceptance integer required".into())
}

#[derive(Debug, PartialEq, Eq)]
struct PackageIdentity {
    manifest: String,
    dlls: BTreeMap<String, String>,
}

fn verified_package(
    ledger: &Value,
    directory: &Path,
    identities: &BTreeMap<String, (String, String)>,
) -> Result<PackageIdentity> {
    let reference = &ledger["package"];
    fields(reference, &["path", "sha256"])?;
    let package = Package::open(&directory.join(text(&reference["path"])?))?;
    if reference["sha256"] != package.digest() {
        return Err("acceptance package manifest digest mismatch".into());
    }
    let mut dlls = BTreeMap::new();
    for target in [Target::X64, Target::X86] {
        if package.production(target) != identities[target.triple()].1 {
            return Err("acceptance package source identity mismatch".into());
        }
        dlls.insert(
            target.triple().into(),
            package
                .file_digest(&format!("{}/KeyrillTsf.dll", target.name()))
                .ok_or("acceptance package DLL receipt missing")?
                .to_owned(),
        );
    }
    Ok(PackageIdentity {
        manifest: package.digest().to_owned(),
        dlls,
    })
}

fn validate_ledger(
    inventory: &Inventory,
    ledger: &Value,
    identities: &BTreeMap<String, (String, String)>,
) -> Result<()> {
    if ledger["schema"] != 2 {
        return Err("Windows acceptance result ledger schema 2 required; older ledgers lack package and loaded-module evidence".into());
    }
    fields(
        ledger,
        &[
            "schema",
            "run",
            "windowsBuild",
            "candidate",
            "package",
            "installations",
            "results",
        ],
    )?;
    let run = text(&ledger["run"])?;
    text(&ledger["windowsBuild"])?;
    validate_candidate(&ledger["candidate"], identities)?;
    let mut installations = BTreeMap::new();
    for installed in array(&ledger["installations"])? {
        fields(
            installed,
            &["application", "version", "packageIdentity", "target"],
        )?;
        let application = text(&installed["application"])?;
        if !APPLICATIONS.iter().any(|(id, _, _)| *id == application) {
            return Err("acceptance installation outside approved application scope".into());
        }
        text(&installed["version"])?;
        text(&installed["packageIdentity"])?;
        let target = text(&installed["target"])?;
        if !identities.contains_key(target) || installations.insert(application, target).is_some() {
            return Err("duplicate or unsupported acceptance installation architecture".into());
        }
    }
    if installations.len() != APPLICATIONS.len() {
        return Err("installed acceptance application metadata incomplete".into());
    }
    if !installations.values().any(|target| *target == TARGETS[1]) {
        return Err(
            "acceptance requires a registered x86 target application beyond the proof editor"
                .into(),
        );
    }
    let mut seen = BTreeSet::new();
    for result in array(&ledger["results"])? {
        let key = text(&result["requirement"])?;
        let requirement = inventory
            .requirements
            .get(key)
            .ok_or("unknown acceptance requirement")?;
        if !seen.insert(key) {
            return Err("duplicate acceptance result".into());
        }
        let target = match requirement.target {
            Some(target) => target,
            None => installations[requirement.application.as_str()],
        };
        validate_result(result, requirement, target, run, identities)?;
    }
    if seen.len() != inventory.requirements.len() {
        return Err(format!(
            "acceptance evidence incomplete: {} of {} required records",
            seen.len(),
            inventory.requirements.len()
        )
        .into());
    }
    Ok(())
}

fn validate_candidate(
    candidate: &Value,
    identities: &BTreeMap<String, (String, String)>,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for artifact in array(candidate)? {
        fields(artifact, &["target", "sourceDigest", "productionDigest"])?;
        let target = text(&artifact["target"])?;
        let expected = identities
            .get(target)
            .ok_or("unsupported acceptance candidate architecture")?;
        if !seen.insert(target)
            || artifact["sourceDigest"] != expected.0
            || artifact["productionDigest"] != expected.1
        {
            return Err("acceptance candidate identity mismatch".into());
        }
    }
    if seen.len() != TARGETS.len() {
        return Err("acceptance candidate requires both native architectures".into());
    }
    Ok(())
}

fn validate_result(
    result: &Value,
    requirement: &Requirement,
    target: &str,
    run: &str,
    identities: &BTreeMap<String, (String, String)>,
) -> Result<()> {
    fields(
        result,
        &[
            "requirement",
            "status",
            "target",
            "sourceDigest",
            "productionDigest",
            "evidence",
            "mode",
            "run",
            "processId",
            "processCreated",
            "threadId",
            "controlIdentity",
            "fixtureIdentity",
            "armedClient",
            "armedPublication",
            "publication",
            "oracleChecks",
            "artifact",
            "activation",
            "moduleObservation",
        ],
    )?;
    if result["status"] != "Verified" {
        return Err("acceptance evidence remains pending, failed or inconclusive".into());
    }
    if requirement.evidence == "controlled" {
        if result["activation"] != "controlled" || !result["moduleObservation"].is_null() {
            return Err(
                "controlled acceptance must not claim installed production module evidence".into(),
            );
        }
    } else {
        let activation = text(&result["activation"])?;
        if activation != "registered"
            && !(requirement.application == "proof-editor" && activation == "registration-free")
        {
            return Err("application acceptance requires registered activation evidence".into());
        }
        artifact_reference(&result["moduleObservation"])?;
    }
    let expected = &identities[target];
    if result["target"] != target
        || result["sourceDigest"] != expected.0
        || result["productionDigest"] != expected.1
        || result["evidence"] != requirement.evidence
        || result["mode"] != requirement.mode
        || result["run"] != run
    {
        return Err("acceptance result source, architecture, origin, mode or run mismatch".into());
    }
    for key in ["processId", "processCreated", "threadId"] {
        positive(&result[key])?;
    }
    for key in ["controlIdentity", "fixtureIdentity"] {
        text(&result[key])?;
    }
    let client_fields = [
        "processId",
        "processCreated",
        "threadId",
        "controlIdentity",
        "fixtureIdentity",
    ];
    fields(&result["armedClient"], &client_fields)?;
    if client_fields
        .iter()
        .any(|key| result["armedClient"][*key] != result[*key])
    {
        return Err("acceptance armed and observed client identities differ".into());
    }
    artifact_reference(&result["artifact"])?;
    let armed = result["armedPublication"]
        .as_u64()
        .ok_or("armed publication required")?;
    if positive(&result["publication"])? <= armed {
        return Err("stale acceptance observation publication".into());
    }
    if strings(&result["oracleChecks"])? != requirement.oracles {
        return Err("acceptance oracle evidence incomplete".into());
    }
    Ok(())
}

/// Require a complete local ledger bound to current source and verified package bytes.
///
/// This checks record consistency, candidate integrity and captured module receipts. Independent
/// artifact review and the physical gate establish behavior and observation provenance.
/// Keep actual results outside versioned source inputs.
///
/// # Errors
/// Returns inventory, identity, schema, missing evidence, stale observation or mismatch errors.
pub(crate) fn validate_results(root: &Path, path: &Path) -> Result<()> {
    let inventory = inventory(root)?;
    let ledger = read_json(path)?;
    let identities = candidate_identities(root)?;
    validate_ledger(&inventory, &ledger, &identities)?;
    let directory = path.parent().ok_or("result ledger directory unavailable")?;
    let package = verified_package(&ledger, directory, &identities)?;
    validate_artifacts(&ledger, directory, &package)?;
    if verified_package(&ledger, directory, &identities)? != package {
        return Err("acceptance package changed during ledger validation".into());
    }
    if candidate_identities(root)? != identities {
        return Err("acceptance source changed during ledger validation".into());
    }
    Ok(())
}

fn artifact_reference(value: &Value) -> Result<(&str, &str)> {
    fields(value, &["path", "sha256"])?;
    let path = text(&value["path"])?;
    let digest = text(&value["sha256"])?;
    if !is_digest(digest) {
        return Err("acceptance evidence digest invalid".into());
    }
    Ok((path, digest))
}

fn validate_artifacts(ledger: &Value, directory: &Path, package: &PackageIdentity) -> Result<()> {
    let mut artifacts = BTreeMap::new();
    let mut observations = BTreeSet::new();
    for result in array(&ledger["results"])? {
        for key in ["artifact", "moduleObservation"] {
            if result[key].is_null() {
                continue;
            }
            let (path, digest) = artifact_reference(&result[key])?;
            let path = directory.join(path);
            if key == "moduleObservation" {
                observations.insert(path.clone());
            }
            if let Some(previous) = artifacts.insert(path, digest)
                && previous != digest
            {
                return Err("acceptance evidence reference has conflicting digests".into());
            }
        }
    }
    let mut modules = BTreeMap::new();
    for (path, digest) in artifacts {
        let bytes = read_bounded(&path)?;
        if Digest::of_bytes(&bytes).to_string() != digest {
            return Err("acceptance evidence artifact digest mismatch".into());
        }
        if observations.contains(&path) {
            let module = parse_json(&bytes)?;
            fields(
                &module,
                &[
                    "schema",
                    "processId",
                    "processCreated",
                    "target",
                    "dllSha256",
                    "packageManifestSha256",
                ],
            )?;
            if module["schema"] != 1 {
                return Err("unsupported acceptance module observation".into());
            }
            modules.insert(path, module);
        }
    }
    let mut registered_x86 = false;
    for result in array(&ledger["results"])? {
        if result["moduleObservation"].is_null() {
            continue;
        }
        let module = &modules[&directory.join(text(&result["moduleObservation"]["path"])?)];
        let target = text(&result["target"])?;
        if module["processId"] != result["processId"]
            || module["processCreated"] != result["processCreated"]
            || module["target"] != target
            || module["dllSha256"] != package.dlls[target]
            || module["packageManifestSha256"] != package.manifest
        {
            return Err("acceptance loaded module does not match the observed process and verified package architecture".into());
        }
        let application = text(&result["requirement"])?
            .split('/')
            .next()
            .ok_or("acceptance application missing")?;
        if APPLICATIONS.iter().any(|(id, _, _)| *id == application)
            && target == TARGETS[1]
            && result["activation"] == "registered"
        {
            registered_x86 = true;
        }
    }
    if !registered_x86 {
        return Err("acceptance requires a registered x86 target application module observation beyond the proof editor".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../native/windows/build-support/src/artifact/fixture.rs"]
mod package_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> Value {
        parse_json(include_bytes!(
            "../../../native/windows/proof/acceptance.json"
        ))
        .unwrap()
    }

    fn identities() -> BTreeMap<String, (String, String)> {
        TARGETS
            .into_iter()
            .enumerate()
            .map(|(index, target)| {
                (
                    target.into(),
                    ("a".repeat(64), index.to_string().repeat(64)),
                )
            })
            .collect()
    }

    fn complete_ledger(inventory: &Inventory) -> Value {
        let identities = identities();
        let results: Vec<_> = inventory.requirements.iter().map(|(key, requirement)| {
            let target = requirement.target.unwrap_or(if requirement.application == "word" { TARGETS[1] } else { TARGETS[0] });
            let identity = &identities[target];
            let module = module_observation(target);
            let module_bytes = serde_json::to_vec(&module).unwrap();
            json!({
                "requirement": key, "status": "Verified", "target": target,
                "sourceDigest": identity.0, "productionDigest": identity.1,
                "evidence": requirement.evidence, "mode": requirement.mode, "run": "public-test-run",
                "processId": 1234, "processCreated": 4567, "threadId": 1235,
                "controlIdentity": "synthetic-control", "fixtureIdentity": "public-fixture-v1",
                "armedClient": {"processId": 1234, "processCreated": 4567, "threadId": 1235, "controlIdentity": "synthetic-control", "fixtureIdentity": "public-fixture-v1"},
                "armedPublication": 1, "publication": 2, "oracleChecks": requirement.oracles,
                "activation": if requirement.evidence == "controlled" {"controlled"} else if requirement.application == "proof-editor" {"registration-free"} else {"registered"},
                "moduleObservation": if requirement.evidence == "controlled" {Value::Null} else {json!({"path": module_name(target), "sha256": Digest::of_bytes(&module_bytes).to_string()})},
                "artifact": {"path": "synthetic-evidence.json", "sha256": Digest::of_bytes(b"public synthetic evidence").to_string()}
            })
        }).collect();
        json!({
            "schema": 2, "run": "public-test-run", "windowsBuild": "synthetic-Windows-11-build",
            "package": {"path": "synthetic-candidate", "sha256": "e".repeat(64)},
            "candidate": identities.iter().map(|(target, (source, production))| json!({"target": target, "sourceDigest": source, "productionDigest": production})).collect::<Vec<_>>(),
            "installations": APPLICATIONS.iter().map(|(id, _, _)| json!({"application": id, "version": "synthetic-1", "packageIdentity": "synthetic-package", "target": if *id == "word" {TARGETS[1]} else {TARGETS[0]}})).collect::<Vec<_>>(),
            "results": results
        })
    }

    fn package_identity() -> PackageIdentity {
        PackageIdentity {
            manifest: "e".repeat(64),
            dlls: BTreeMap::from([
                (TARGETS[0].into(), "8".repeat(64)),
                (TARGETS[1].into(), "9".repeat(64)),
            ]),
        }
    }

    fn module_name(target: &str) -> String {
        format!("module-{target}.json")
    }

    fn module_observation(target: &str) -> Value {
        json!({"schema": 1, "processId": 1234, "processCreated": 4567,
            "target": target, "dllSha256": package_identity().dlls[target],
            "packageManifestSha256": package_identity().manifest})
    }

    fn write_evidence(directory: &Path) {
        std::fs::write(
            directory.join("synthetic-evidence.json"),
            b"public synthetic evidence",
        )
        .unwrap();
        for target in TARGETS {
            std::fs::write(
                directory.join(module_name(target)),
                serde_json::to_vec(&module_observation(target)).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn inventory_expands_unique_fixed_scope_and_complete_native_tracks() {
        let inventory = validate_manifest(&manifest()).unwrap();
        assert!(inventory.requirements.len() > 500);
        for target in TARGETS {
            assert!(inventory.requirements.contains_key(&format!(
                "proof-provider/native-boundaries/unknown-write-effect/controlled/{target}"
            )));
            assert!(
                inventory
                    .requirements
                    .contains_key(&format!("proof-editor/document/manual-k/physical/{target}"))
            );
        }
        assert!(
            inventory
                .requirements
                .values()
                .all(|requirement| !requirement.expected.is_empty())
        );
        assert!(
            !inventory
                .requirements
                .keys()
                .any(|key| key.starts_with("terminal/") && key.contains("/generated/"))
        );
        for (key, requirement) in &inventory.requirements {
            if requirement.oracles.contains("zero-character-reads") {
                assert_eq!(
                    requirement.mode == "off",
                    key.contains("/ordinary-field-off/"),
                    "{key}"
                );
            }
        }
        for (_, requirement) in inventory
            .requirements
            .iter()
            .filter(|(key, _)| key.starts_with("terminal/ordinary-input/"))
        {
            assert!(requirement.oracles.contains("committed-bytes"));
            assert!(requirement.oracles.contains("ordinary-input-authority"));
            for transcript_oracle in ["whole-public-document", "prefix", "suffix"] {
                assert!(!requirement.oracles.contains(transcript_oracle));
            }
        }
        assert!(validate_ledger(&inventory, &complete_ledger(&inventory), &identities()).is_ok());
    }

    #[test]
    fn rejects_scope_schema_duplicates_and_missing_oracles() {
        let original = manifest();
        let mut mutations = Vec::new();
        let mut changed = original.clone();
        changed["schema"] = json!(2);
        mutations.push(changed);
        let mut changed = original.clone();
        changed["applications"][0]["id"] = json!("outlook");
        mutations.push(changed);
        let mut changed = original.clone();
        changed["nativeTargets"] = json!([TARGETS[0]]);
        mutations.push(changed);
        let mut changed = original.clone();
        changed["controls"].as_array_mut().unwrap().pop();
        mutations.push(changed);
        let mut changed = original.clone();
        changed["controls"][1] = changed["controls"][0].clone();
        mutations.push(changed);
        let mut changed = original.clone();
        changed["scenarioGroups"]["ordinary"]["scenarios"][1] =
            changed["scenarioGroups"]["ordinary"]["scenarios"][0].clone();
        mutations.push(changed);
        let mut changed = original.clone();
        changed["scenarioGroups"]["ordinary"]["evidence"] = json!(["generated"]);
        mutations.push(changed);
        let mut changed = original.clone();
        changed["oracleSets"]["document"]
            .as_array_mut()
            .unwrap()
            .pop();
        mutations.push(changed);
        let mut changed = original;
        changed["results"] = json!([]);
        mutations.push(changed);
        for changed in mutations {
            assert!(validate_manifest(&changed).is_err());
        }
    }

    #[test]
    fn missing_pending_or_partial_evidence_keeps_completion_closed() {
        let inventory = validate_manifest(&manifest()).unwrap();
        let complete = complete_ledger(&inventory);
        let mut missing = complete.clone();
        missing["results"].as_array_mut().unwrap().pop();
        assert!(
            validate_ledger(&inventory, &missing, &identities())
                .unwrap_err()
                .to_string()
                .starts_with("acceptance evidence incomplete")
        );
        for status in ["Pending", "Failed", "Inconclusive", "NotApplicable"] {
            let mut changed = complete.clone();
            changed["results"][0]["status"] = json!(status);
            assert!(validate_ledger(&inventory, &changed, &identities()).is_err());
        }
        let mut changed = complete;
        changed["results"][0]["oracleChecks"] = json!(["focus"]);
        assert!(validate_ledger(&inventory, &changed, &identities()).is_err());
    }

    #[test]
    fn rejects_wrong_stale_source_architecture_and_origin_identity() {
        let inventory = validate_manifest(&manifest()).unwrap();
        let complete = complete_ledger(&inventory);
        for (field, replacement) in [
            ("sourceDigest", json!("b".repeat(64))),
            ("productionDigest", json!("c".repeat(64))),
            ("target", json!(TARGETS[1])),
            ("evidence", json!("historical")),
            ("run", json!("stale-run")),
            ("processId", json!(1236)),
            ("processCreated", json!(4568)),
            ("threadId", json!(1236)),
            ("controlIdentity", json!("other-control")),
            ("fixtureIdentity", json!("other-fixture")),
            ("publication", json!(1)),
        ] {
            let mut changed = complete.clone();
            changed["results"][0][field] = replacement;
            assert!(
                validate_ledger(&inventory, &changed, &identities()).is_err(),
                "{field}"
            );
        }
        let mut changed = complete.clone();
        changed["candidate"].as_array_mut().unwrap().pop();
        assert!(validate_ledger(&inventory, &changed, &identities()).is_err());
        let mut changed = complete.clone();
        changed["candidate"][0]["productionDigest"] = json!("d".repeat(64));
        assert!(validate_ledger(&inventory, &changed, &identities()).is_err());
        let mut changed = complete;
        changed["installations"][0]["target"] = json!("aarch64-pc-windows-msvc");
        assert!(validate_ledger(&inventory, &changed, &identities()).is_err());
    }

    #[test]
    fn requires_x86_application_beyond_both_proof_tracks() {
        let inventory = validate_manifest(&manifest()).unwrap();
        let mut ledger = complete_ledger(&inventory);
        assert!(validate_ledger(&inventory, &ledger, &identities()).is_ok());
        ledger["installations"][5]["target"] = json!(TARGETS[0]);
        for result in ledger["results"].as_array_mut().unwrap() {
            if result["requirement"].as_str().unwrap().starts_with("word/") {
                result["target"] = json!(TARGETS[0]);
                result["productionDigest"] = json!(identities()[TARGETS[0]].1);
            }
        }
        assert!(
            validate_ledger(&inventory, &ledger, &identities())
                .unwrap_err()
                .to_string()
                .contains("registered x86 target application")
        );

        let mut legacy = complete_ledger(&inventory);
        legacy["schema"] = json!(1);
        assert!(
            validate_ledger(&inventory, &legacy, &identities())
                .unwrap_err()
                .to_string()
                .contains("schema 2 required")
        );
    }

    #[test]
    fn evidence_artifacts_must_exist_and_match_their_complete_digest() {
        let inventory = validate_manifest(&manifest()).unwrap();
        let mut ledger = complete_ledger(&inventory);
        let directory = std::env::temp_dir().join(format!(
            "keyrill-acceptance-artifacts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        assert!(validate_artifacts(&ledger, &directory, &package_identity()).is_err());
        let artifact = directory.join("synthetic-evidence.json");
        write_evidence(&directory);
        validate_artifacts(&ledger, &directory, &package_identity()).unwrap();
        std::fs::write(&artifact, b"different synthetic evidence").unwrap();
        assert!(validate_artifacts(&ledger, &directory, &package_identity()).is_err());
        ledger["results"][0]["artifact"]["sha256"] = json!("b".repeat(64));
        assert!(
            validate_artifacts(&ledger, &directory, &package_identity())
                .unwrap_err()
                .to_string()
                .contains("conflicting digests")
        );
        std::fs::remove_file(artifact).unwrap();
        for target in TARGETS {
            std::fs::remove_file(directory.join(module_name(target))).unwrap();
        }
        std::fs::remove_dir(directory).unwrap();
    }

    fn replace_module_evidence(ledger: &mut Value, directory: &Path, target: &str, module: &Value) {
        let name = module_name(target);
        let bytes = serde_json::to_vec(module).unwrap();
        std::fs::write(directory.join(&name), &bytes).unwrap();
        for result in ledger["results"].as_array_mut().unwrap() {
            if result["moduleObservation"]["path"] == name {
                result["moduleObservation"]["sha256"] = json!(Digest::of_bytes(&bytes).to_string());
            }
        }
    }

    #[test]
    fn rejects_module_substitution_even_with_rehashed_observation_artifacts() {
        let inventory = validate_manifest(&manifest()).unwrap();
        let fixture = package_fixture::Fixture::new();
        let original = complete_ledger(&inventory);
        write_evidence(&fixture.0);
        validate_artifacts(&original, &fixture.0, &package_identity()).unwrap();
        for (field, replacement) in [
            ("processId", json!(999)),
            ("processCreated", json!(4568)),
            ("target", json!(TARGETS[0])),
            ("dllSha256", json!(package_identity().dlls[TARGETS[0]])),
            ("dllSha256", json!("a".repeat(64))),
            ("packageManifestSha256", json!("f".repeat(64))),
        ] {
            let mut ledger = original.clone();
            let mut module = module_observation(TARGETS[1]);
            module[field] = replacement;
            replace_module_evidence(&mut ledger, &fixture.0, TARGETS[1], &module);
            assert!(
                validate_artifacts(&ledger, &fixture.0, &package_identity()).is_err(),
                "{field}"
            );
        }

        let mut ledger = original.clone();
        for result in ledger["results"].as_array_mut().unwrap() {
            if result["requirement"].as_str().unwrap().starts_with("word/") {
                result["activation"] = json!("registration-free");
            }
        }
        assert!(validate_ledger(&inventory, &ledger, &identities()).is_err());
        assert!(validate_artifacts(&ledger, &fixture.0, &package_identity()).is_err());
    }

    #[test]
    fn controlled_test_api_results_do_not_assert_loaded_production_identity() {
        let inventory = validate_manifest(&manifest()).unwrap();
        let mut ledger = complete_ledger(&inventory);
        let controlled = ledger["results"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|result| result["evidence"] == "controlled")
            .unwrap();
        assert_eq!(controlled["activation"], "controlled");
        assert!(controlled["moduleObservation"].is_null());
        controlled["activation"] = json!("registered");
        assert!(validate_ledger(&inventory, &ledger, &identities()).is_err());
    }

    #[test]
    fn package_binding_checks_actual_manifest_and_both_verified_dll_receipts() {
        let fixture = package_fixture::Fixture::new();
        let mut manifest = fixture.candidate();
        let identities: BTreeMap<_, _> = [Target::X64, Target::X86]
            .into_iter()
            .map(|target| {
                (
                    target.triple().into(),
                    (
                        manifest["source"].as_str().unwrap().to_owned(),
                        manifest["architectures"][target.name()]["production"]
                            .as_str()
                            .unwrap()
                            .to_owned(),
                    ),
                )
            })
            .collect();
        let manifest_bytes = std::fs::read(fixture.0.join("package.json")).unwrap();
        let mut ledger = json!({"package": {"path": fixture.0, "sha256": Digest::of_bytes(&manifest_bytes).to_string()}});
        let original = verified_package(&ledger, &fixture.0, &identities).unwrap();
        assert_ne!(original.dlls[TARGETS[0]], original.dlls[TARGETS[1]]);
        for target in [Target::X64, Target::X86] {
            let path = fixture.0.join(format!("{}/KeyrillTsf.dll", target.name()));
            assert_eq!(
                original.dlls[target.triple()],
                Digest::of_bytes(&std::fs::read(path).unwrap()).to_string()
            );
        }
        ledger["package"]["sha256"] = json!("f".repeat(64));
        assert!(verified_package(&ledger, &fixture.0, &identities).is_err());
        ledger["package"]["sha256"] = json!(original.manifest);
        let mut wrong_source = identities.clone();
        wrong_source.get_mut(TARGETS[1]).unwrap().1 = "c".repeat(64);
        assert!(verified_package(&ledger, &fixture.0, &wrong_source).is_err());

        let dll = fixture.0.join("x86/KeyrillTsf.dll");
        let mut bytes = std::fs::read(&dll).unwrap();
        bytes.extend(b"alternate public build output");
        std::fs::write(&dll, &bytes).unwrap();
        assert!(verified_package(&ledger, &fixture.0, &identities).is_err());
        manifest["files"]["x86/KeyrillTsf.dll"] = json!(Digest::of_bytes(&bytes).to_string());
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        std::fs::write(fixture.0.join("package.json"), &manifest_bytes).unwrap();
        assert!(verified_package(&ledger, &fixture.0, &identities).is_err());
        ledger["package"]["sha256"] = json!(Digest::of_bytes(&manifest_bytes).to_string());
        let replacement = verified_package(&ledger, &fixture.0, &identities).unwrap();
        assert_ne!(replacement.manifest, original.manifest);
        assert_ne!(replacement.dlls[TARGETS[1]], original.dlls[TARGETS[1]]);
    }

    #[test]
    fn replacing_only_package_header_cannot_reuse_prior_run_with_unchanged_dlls() {
        let inventory = validate_manifest(&manifest()).unwrap();
        let candidate = package_fixture::Fixture::new();
        let mut manifest = candidate.candidate();
        let identities: BTreeMap<_, _> = [Target::X64, Target::X86]
            .into_iter()
            .map(|target| {
                (
                    target.triple().into(),
                    (
                        manifest["source"].as_str().unwrap().to_owned(),
                        manifest["architectures"][target.name()]["production"]
                            .as_str()
                            .unwrap()
                            .to_owned(),
                    ),
                )
            })
            .collect();
        let mut ledger = complete_ledger(&inventory);
        ledger["package"] = json!({"path": candidate.0, "sha256": Digest::of_bytes(&std::fs::read(candidate.0.join("package.json")).unwrap()).to_string()});
        let original = verified_package(&ledger, &candidate.0, &identities).unwrap();
        let evidence = package_fixture::Fixture::new();
        write_evidence(&evidence.0);
        for target in TARGETS {
            let mut observation = module_observation(target);
            observation["dllSha256"] = json!(original.dlls[target]);
            observation["packageManifestSha256"] = json!(original.manifest);
            replace_module_evidence(&mut ledger, &evidence.0, target, &observation);
        }
        validate_artifacts(&ledger, &evidence.0, &original).unwrap();

        let changed_license = b"Alternate synthetic public license text";
        std::fs::write(candidate.0.join("LICENSE"), changed_license).unwrap();
        manifest["files"]["LICENSE"] = json!(Digest::of_bytes(changed_license).to_string());
        let bytes = serde_json::to_vec(&manifest).unwrap();
        std::fs::write(candidate.0.join("package.json"), &bytes).unwrap();
        ledger["package"]["sha256"] = json!(Digest::of_bytes(&bytes).to_string());
        let replacement = verified_package(&ledger, &candidate.0, &identities).unwrap();
        assert_eq!(replacement.dlls, original.dlls);
        assert_ne!(replacement.manifest, original.manifest);
        assert!(validate_artifacts(&ledger, &evidence.0, &replacement).is_err());
    }

    #[test]
    fn json_rejects_duplicate_keys_including_escaped_equivalents() {
        for duplicate in [
            br#"{"schema":1,"schema":1}"#.as_slice(),
            br#"{"schema":1,"\u0073chema":2}"#.as_slice(),
            br#"[{"client":{"pid":1,"pid":2}}]"#.as_slice(),
        ] {
            assert!(parse_json(duplicate).is_err());
        }
        assert!(
            parse_json(
                br#"{"quoted\"key":{"value":"escaped\\\" , {}[]"},"array":[{"key":1},{"key":2}]}"#
            )
            .is_ok()
        );
        assert!(parse_json(br#"{"incomplete":"#).is_err());
    }
}
