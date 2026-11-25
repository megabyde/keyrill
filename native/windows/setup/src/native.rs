//! Native machine-scope installer. Every mutation is behind an explicit maintainer command.
mod files;
mod profile_repair;
mod registry;
mod tsf;

use crate::Result;
use crate::journal::{Backend, Change, Journal};
use keyrill_windows::profiles::{PROFILES, SERVICE_ID};
use keyrill_windows_build::Target;
use keyrill_windows_build::artifact::{Package, is_digest};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use windows::Win32::System::Registry::*;

struct Installation {
    root: PathBuf,
    shortcut: PathBuf,
    _com: tsf::Com,
}

fn tip() -> String {
    format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{{{SERVICE_ID:?}}}")
}
fn com_path() -> String {
    format!("SOFTWARE\\Classes\\CLSID\\{{{SERVICE_ID:?}}}\\InprocServer32")
}
fn profile_path(index: usize) -> String {
    let p = &PROFILES[index];
    format!(
        "{}\\LanguageProfile\\0x{:08x}\\{{{:?}}}",
        tip(),
        p.language,
        p.id
    )
}
const UNINSTALL: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Keyrill";

fn registry_resource(name: &str) -> Option<(HKEY, REG_SAM_FLAGS, String, String)> {
    let parts: Vec<_> = name.split('/').collect();
    match parts.as_slice() {
        ["com", hive, arch, value]
            if ["machine", "user"].contains(hive) && ["server", "threading"].contains(value) =>
        {
            let view = match *arch {
                "x64" => KEY_WOW64_64KEY,
                "x86" => KEY_WOW64_32KEY,
                _ => return None,
            };
            Some((
                if *hive == "machine" {
                    HKEY_LOCAL_MACHINE
                } else {
                    HKEY_CURRENT_USER
                },
                view,
                com_path(),
                if *value == "server" {
                    ""
                } else {
                    "ThreadingModel"
                }
                .into(),
            ))
        }
        ["uninstall", value]
            if [
                "DisplayName",
                "DisplayVersion",
                "Publisher",
                "InstallLocation",
                "UninstallString",
                "QuietUninstallString",
                "NoModify",
                "NoRepair",
                "DisplayIcon",
            ]
            .contains(value) =>
        {
            Some((
                HKEY_LOCAL_MACHINE,
                KEY_WOW64_64KEY,
                UNINSTALL.into(),
                (*value).into(),
            ))
        }
        _ => None,
    }
}

impl Backend for Installation {
    fn read(&mut self, resource: &str) -> Result<Value> {
        if let Some((hive, view, path, name)) = registry_resource(resource) {
            return registry::read(hive, view, &path, &name);
        }
        match resource {
            "service" => tsf::service_state(),
            "profile/en" => tsf::profile_state(0),
            "profile/ru" => tsf::profile_state(1),
            "layout/en" => tsf::layout_state(0),
            "layout/ru" => tsf::layout_state(1),
            "enable/en" => registry::read(
                HKEY_CURRENT_USER,
                KEY_WOW64_64KEY,
                &profile_path(0),
                "Enable",
            ),
            "enable/ru" => registry::read(
                HKEY_CURRENT_USER,
                KEY_WOW64_64KEY,
                &profile_path(1),
                "Enable",
            ),
            "category" => tsf::category_state(),
            "shortcut" => files::read(&self.shortcut),
            _ => Err("unrecognized installation resource".into()),
        }
    }
    fn write(&mut self, resource: &str, value: &Value) -> Result<()> {
        if let Some((hive, view, path, name)) = registry_resource(resource) {
            return registry::write(hive, view, &path, &name, value);
        }
        match resource {
            "service" => tsf::service_write(value),
            "profile/en" => tsf::profile_write(0, value),
            "profile/ru" => tsf::profile_write(1, value),
            "layout/en" => tsf::layout_write(0, value),
            "layout/ru" => tsf::layout_write(1, value),
            "category" => tsf::category_write(value),
            "shortcut" => files::write(&self.shortcut, value),
            _ => Err("unrecognized installation resource".into()),
        }
    }
    fn persist(&mut self, journal: &Journal) -> Result<()> {
        files::atomic(
            &self.root.join("operation.json"),
            &serde_json::to_vec_pretty(&journal.value())?,
        )
    }
}

impl Installation {
    fn open() -> Result<Self> {
        let root = files::program_files()?.join("Keyrill");
        let shortcut = files::programs()?.join("Keyrill Settings.lnk");
        Ok(Self {
            root,
            shortcut,
            _com: tsf::Com::new()?,
        })
    }
    fn receipt(&self, name: &str) -> Result<Option<Journal>> {
        let path = self.root.join(name);
        if !path.try_exists()? {
            return Ok(None);
        }
        files::ordinary(&path)?;
        Ok(Some(Journal::parse(&fs::read(path)?)?))
    }
    fn pending(&self) -> Result<Option<Journal>> {
        let installed = self.receipt("installed.json")?;
        Ok(self.receipt("operation.json")?.filter(|receipt| {
            receipt.completed < receipt.changes.len() || Some(receipt) != installed.as_ref()
        }))
    }
    fn resources() -> Vec<String> {
        let mut values = Vec::new();
        for hive in ["machine", "user"] {
            for arch in ["x64", "x86"] {
                for value in ["server", "threading"] {
                    values.push(format!("com/{hive}/{arch}/{value}"));
                }
            }
        }
        values.extend(
            [
                "service",
                "profile/en",
                "profile/ru",
                "layout/en",
                "layout/ru",
                "enable/en",
                "enable/ru",
                "category",
                "shortcut",
            ]
            .map(str::to_owned),
        );
        for value in [
            "DisplayName",
            "DisplayVersion",
            "Publisher",
            "InstallLocation",
            "UninstallString",
            "QuietUninstallString",
            "NoModify",
            "NoRepair",
            "DisplayIcon",
        ] {
            values.push(format!("uninstall/{value}"));
        }
        values
    }
    fn status(&mut self) -> Result<()> {
        for resource in Self::resources() {
            match self.read(&resource) {
                Ok(value) => println!(
                    "{resource}: {}",
                    if value.is_null() || value == false {
                        "absent"
                    } else {
                        "present"
                    }
                ),
                Err(_) => println!("{resource}: unavailable"),
            }
        }
        for (index, name) in ["en", "ru"].into_iter().enumerate() {
            let enabled = registry::read(
                HKEY_CURRENT_USER,
                KEY_WOW64_64KEY,
                &profile_path(index),
                "Enable",
            )?;
            println!(
                "current-user/{name}/Enable: {}",
                if enabled.is_null() {
                    "absent"
                } else {
                    "present (user-owned)"
                }
            );
        }
        println!(
            "Scope: machine COM x64/x86 and TSF definitions; current-user enablement is separate. No elevation or source selection."
        );
        println!(
            "Operation receipt: {}",
            if self.receipt("operation.json")?.is_some() {
                "present"
            } else {
                "absent"
            }
        );
        Ok(())
    }
    fn preflight(&self) -> Result<()> {
        self.preflight_profile(None)
    }
    fn preflight_profile(&self, profile: Option<usize>) -> Result<()> {
        files::require_elevated()?;
        files::ordinary_ancestors(&self.root)?;
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            registry::require_write(HKEY_LOCAL_MACHINE, view, "SOFTWARE\\Classes\\CLSID")?;
            registry::require_write(HKEY_LOCAL_MACHINE, view, "SOFTWARE\\Microsoft\\CTF\\TIP")?;
        }
        tsf::scope_preflight_except(profile)?;
        Ok(())
    }
    fn stage(&self, package: &Package) -> Result<PathBuf> {
        let destination = self.root.join("versions").join(package.digest());
        files::ordinary_ancestors(&destination)?;
        if destination.exists() {
            for name in package
                .files()
                .keys()
                .map(String::as_str)
                .chain(std::iter::once("package.json"))
            {
                let target = destination.join(name);
                files::ordinary_ancestors(&target)?;
                let expected = fs::read(package.directory().join(name))?;
                if target.exists() {
                    if fs::read(&target)? != expected {
                        return Err(
                            "installed version contains modified bytes; repair refused".into()
                        );
                    }
                } else {
                    fs::create_dir_all(target.parent().ok_or("installed file parent missing")?)?;
                    files::create(&self.root, &target, &expected)?;
                }
            }
            if Package::open(&destination)?.digest() != package.digest() {
                return Err("installed package conflict".into());
            }
            return Ok(destination);
        }
        let stage = self.root.join("staging").join(package.digest());
        fs::create_dir_all(&stage)?;
        files::ordinary_ancestors(&stage)?;
        for name in package
            .files()
            .keys()
            .map(String::as_str)
            .chain(std::iter::once("package.json"))
        {
            let source = package.directory().join(name);
            let target = stage.join(name);
            files::ordinary_ancestors(&target)?;
            let bytes = fs::read(source)?;
            if target.exists() {
                if fs::read(&target)? != bytes {
                    return Err("interrupted staging contains conflicting bytes".into());
                }
            } else {
                fs::create_dir_all(target.parent().ok_or("staging parent missing")?)?;
                files::create(&self.root, &target, &bytes)?;
            }
        }
        if Package::open(&stage)?.digest() != package.digest() {
            return Err("staged package verification failed".into());
        }
        fs::create_dir_all(destination.parent().ok_or("version parent missing")?)?;
        fs::rename(&stage, &destination)?;
        Package::open(&destination)?;
        Ok(destination)
    }
    fn desired(&self, package: &Package, directory: &Path) -> Result<Vec<(String, Value)>> {
        let mut result = Vec::new();
        for target in [Target::X64, Target::X86] {
            result.push((
                format!("com/machine/{}/server", target.name()),
                registry::string(
                    directory
                        .join(target.name())
                        .join("KeyrillTsf.dll")
                        .to_str()
                        .ok_or("non-Unicode installation path")?,
                ),
            ));
            result.push((
                format!("com/machine/{}/threading", target.name()),
                registry::string("Apartment"),
            ));
        }
        result.push(("service".into(), json!(true)));
        for (index, name) in ["en", "ru"].into_iter().enumerate() {
            let icon = directory.join(if index == 0 {
                "resources/english.ico"
            } else {
                "resources/russian.ico"
            });
            result.push((
                format!("profile/{name}"),
                tsf::profile_desired(index, &icon)?,
            ));
            result.push((
                format!("layout/{name}"),
                json!(format!(
                    "0x{:04x}{:04x}",
                    PROFILES[index].language, PROFILES[index].language
                )),
            ));
        }
        result.push(("category".into(), json!(true)));
        let setup = directory.join("x64/KeyrillSetup.exe");
        for (name, value) in [
            ("DisplayName", "Keyrill".to_owned()),
            ("DisplayVersion", package.version().to_owned()),
            ("Publisher", "Keyrill contributors".into()),
            (
                "InstallLocation",
                directory
                    .to_str()
                    .ok_or("non-Unicode installation path")?
                    .into(),
            ),
            (
                "UninstallString",
                format!("\"{}\" uninstall --scope machine", setup.display()),
            ),
            (
                "QuietUninstallString",
                format!("\"{}\" uninstall --scope machine", setup.display()),
            ),
            (
                "DisplayIcon",
                directory
                    .join("resources/english.ico")
                    .to_str()
                    .ok_or("non-Unicode icon path")?
                    .into(),
            ),
        ] {
            result.push((format!("uninstall/{name}"), registry::string(&value)));
        }
        for name in ["NoModify", "NoRepair"] {
            result.push((format!("uninstall/{name}"), registry::dword(1)));
        }
        result.push((
            "shortcut".into(),
            files::shortcut(&self.root, &directory.join("x64/KeyrillSettings.exe"))?,
        ));
        Ok(result)
    }
    fn install(&mut self, package: &Package, proof: Option<&Path>) -> Result<()> {
        self.preflight()?;
        for index in 0..2 {
            tsf::loaded_layout(index)?;
        }
        let _lock = files::lock(&self.root)?;
        self.repair_gate(Some(package.digest()))?;
        if let Some(mut pending) = self.pending()? {
            if pending.desired != package.digest() {
                return Err("another installation is pending; use recover".into());
            }
            self.validate_receipt(&pending)?;
            pending.reconcile(self)?;
            return self.finish(&pending);
        }
        let previous = self.receipt("installed.json")?;
        let previous_id = previous
            .as_ref()
            .and_then(|r| previous_package(r, package.digest()));
        let directory = self.stage(package)?;
        let desired = self.desired(package, &directory)?;
        let mut changes = Vec::new();
        for (resource, after) in desired {
            let before = self.read(&resource)?;
            let old = previous
                .as_ref()
                .and_then(|r| r.changes.iter().find(|c| c.resource == resource));
            if before != after
                && !before.is_null()
                && before != false
                && old.is_none_or(|c| c.after != before)
                && !tsf::is_proof_resource(&resource, &before, proof)?
            {
                return Err("existing registration is not owned by a matching receipt; use explicit proof migration where applicable".into());
            }
            changes.push(Change {
                resource,
                before,
                after,
            });
        }
        for target in [Target::X64, Target::X86] {
            let resource = format!("com/user/{}/server", target.name());
            let before = self.read(&resource)?;
            if before.is_null() {
                continue;
            }
            let proof = proof
                .ok_or("current-user COM shadow exists; explicit proof migration is required")?;
            if Path::new(&registry::decode_string(&before)?).canonicalize()? != proof {
                return Err("current-user COM shadow is not the selected proof DLL".into());
            }
            let threading = format!("com/user/{}/threading", target.name());
            let threading_before = self.read(&threading)?;
            if threading_before != registry::string("Apartment") {
                return Err("current-user COM threading value differs".into());
            }
            changes.push(Change {
                resource: threading,
                before: threading_before,
                after: Value::Null,
            });
            changes.push(Change {
                resource,
                before,
                after: Value::Null,
            });
        }
        for name in ["enable/en", "enable/ru"] {
            let before = self.read(name)?;
            changes.push(Change {
                resource: name.into(),
                after: before.clone(),
                before,
            });
        }
        let mut operation = Journal {
            desired: package.digest().into(),
            previous: previous_id,
            changes,
            completed: 0,
        };
        operation.reconcile(self)?;
        self.finish(&operation)
    }
    fn validate_receipt(&self, receipt: &Journal) -> Result<()> {
        for change in &receipt.changes {
            if !Self::resources().contains(&change.resource) {
                return Err("receipt resource outside installation ownership".into());
            }
        }
        if receipt.desired != "absent" {
            let package = Package::open(&self.root.join("versions").join(&receipt.desired))?;
            if package.digest() != receipt.desired {
                return Err("receipt package changed".into());
            }
        }
        Ok(())
    }
    fn finish(&self, receipt: &Journal) -> Result<()> {
        files::atomic(
            &self.root.join("installed.json"),
            &serde_json::to_vec_pretty(&receipt.value())?,
        )?;
        println!(
            "Verified machine registration converged. Sources were not enabled or selected. Settings and dictionaries preserved."
        );
        println!(
            "Old version files are retained while clients may have them loaded. Run cleanup after those clients exit."
        );
        Ok(())
    }
    fn recover(&mut self) -> Result<()> {
        self.preflight()?;
        let _lock = files::lock(&self.root)?;
        self.repair_gate(None)?;
        let mut operation = self
            .receipt("operation.json")?
            .ok_or("no installation operation to recover")?;
        self.validate_receipt(&operation)?;
        operation.reconcile(self)?;
        self.finish(&operation)
    }
    fn uninstall(&mut self) -> Result<()> {
        self.preflight()?;
        let _lock = files::lock(&self.root)?;
        self.repair_gate(None)?;
        if self.pending()?.is_some() {
            return Err("pending installation must be recovered before uninstall".into());
        }
        let installed = self
            .receipt("installed.json")?
            .ok_or("no owned installation receipt")?;
        if installed.desired == "absent" {
            return Ok(());
        }
        self.validate_receipt(&installed)?;
        let mut changes = Vec::new();
        for old in installed.changes.iter().rev() {
            if old.resource.starts_with("com/user/")
                || old.resource.starts_with("layout/")
                || old.resource.starts_with("enable/")
            {
                continue;
            }
            let before = self.read(&old.resource)?;
            if before != old.after {
                return Err("uninstall conflict: owned value changed since installation".into());
            }
            changes.push(Change {
                resource: old.resource.clone(),
                before,
                after: if ["service", "category"].contains(&old.resource.as_str()) {
                    json!(false)
                } else {
                    Value::Null
                },
            });
        }
        let mut operation = Journal {
            desired: "absent".into(),
            previous: Some(installed.desired),
            changes,
            completed: 0,
        };
        operation.reconcile(self)?;
        self.finish(&operation)
    }
    fn cleanup(&mut self) -> Result<()> {
        self.preflight()?;
        let _lock = files::lock(&self.root)?;
        self.repair_gate(None)?;
        let retained = self.repair_packages()?;
        if self.pending()?.is_some() {
            return Err("recover pending operation before cleanup".into());
        }
        let installed = self
            .receipt("installed.json")?
            .ok_or("installation receipt absent")?;
        let versions = self.root.join("versions");
        if !versions.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(versions)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == installed.desired
                || retained.contains(&name)
                || (installed.desired != "absent" && installed.previous.as_ref() == Some(&name))
            {
                continue;
            }
            if !is_digest(&name) {
                return Err("unexpected version directory; cleanup refused".into());
            }
            if !files::cleanup_version(&self.root, &entry.path())? {
                println!("Old version retained; files are in use.");
            }
        }
        Ok(())
    }
}

fn previous_package(installed: &Journal, desired: &str) -> Option<String> {
    if installed.desired == desired {
        return installed
            .previous
            .clone()
            .filter(|previous| previous != desired);
    }
    (installed.desired != "absent").then(|| installed.desired.clone())
}

/// Run the exact explicit command-line contract without installing, enabling or elevating implicitly.
///
/// # Errors
/// Returns validation, privilege, ownership, unknown-effect and native failures.
pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 2 && args[0] == "verify" {
        let package = Package::open(Path::new(&args[1]))?;
        println!("Verified unsigned candidate {}", package.digest());
        return Ok(());
    }
    let mut installation = Installation::open()?;
    if args.len() == 1 && args[0] == "status" {
        return installation.status();
    }
    if args.len() == 1 && args[0] == "inspect-profile-repair" {
        return installation.inspect_profile_repair();
    }
    if args.len() < 3 || args[1] != "--scope" || args[2] != "machine" {
        return Err("Maintainer only. KeyrillSetup status | inspect-profile-repair | verify PACKAGE | install|repair --scope machine PACKAGE | migrate-proof --scope machine PACKAGE PROOF_DLL | repair-profile --scope machine APPROVAL SHA256 | recover|rollback|uninstall|cleanup --scope machine. No automatic elevation; run explicitly as administrator for machine changes.".into());
    }
    match args[0].to_str() {
        Some("repair-profile") if args.len() == 5 => installation.repair_profile(
            Path::new(&args[3]),
            args[4].to_str().ok_or("approval digest is not Unicode")?,
        ),
        Some("install" | "repair") if args.len() == 4 => {
            installation.install(&Package::open(Path::new(&args[3]))?, None)
        }
        Some("migrate-proof") if args.len() == 5 => {
            let proof = Path::new(&args[4]).canonicalize()?;
            files::ordinary(&proof)?;
            let bytes = fs::read(&proof)?;
            let exports = keyrill_windows_build::pe::exports(&bytes, Target::X64)?;
            if exports.iter().any(|name| name.starts_with("KeyrillTest"))
                || !exports.contains("DllGetClassObject")
            {
                return Err("proof migration requires the selected production proof DLL".into());
            }
            installation.install(&Package::open(Path::new(&args[3]))?, Some(&proof))
        }
        Some("recover") if args.len() == 3 => installation.recover(),
        Some("uninstall") if args.len() == 3 => installation.uninstall(),
        Some("cleanup") if args.len() == 3 => installation.cleanup(),
        Some("rollback") if args.len() == 3 => {
            let installed = installation
                .receipt("installed.json")?
                .ok_or("no previous installation")?;
            let previous = installed
                .previous
                .ok_or("no verified previous package available")?;
            let package = Package::open(&installation.root.join("versions").join(previous))?;
            installation.install(&package, None)
        }
        _ => Err("invalid explicit installation command".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_fixture::Fixture;

    fn receipt(desired: &str, previous: Option<&str>) -> Journal {
        Journal {
            desired: desired.into(),
            previous: previous.map(str::to_owned),
            changes: vec![Change {
                resource: "service".into(),
                before: json!(false),
                after: json!(true),
            }],
            completed: 1,
        }
    }

    #[test]
    fn completed_native_effects_remain_pending_until_exact_receipt_publication() {
        let fixture = Fixture::new();
        let installation = Installation {
            root: fixture.0.clone(),
            shortcut: fixture.0.join("settings.lnk"),
            _com: tsf::Com::new().unwrap(),
        };
        let old = receipt(&"a".repeat(64), None);
        let next = receipt(&"b".repeat(64), Some(&old.desired));
        files::atomic(
            &fixture.0.join("installed.json"),
            &serde_json::to_vec(&old.value()).unwrap(),
        )
        .unwrap();
        files::atomic(
            &fixture.0.join("operation.json"),
            &serde_json::to_vec(&next.value()).unwrap(),
        )
        .unwrap();
        assert_eq!(installation.pending().unwrap(), Some(next.clone()));
        installation.finish(&next).unwrap();
        assert_eq!(installation.pending().unwrap(), None);
        let mut different = next.clone();
        different.changes[0].before = Value::Null;
        files::atomic(
            &fixture.0.join("operation.json"),
            &serde_json::to_vec(&different.value()).unwrap(),
        )
        .unwrap();
        assert_eq!(installation.pending().unwrap(), Some(different));
    }

    #[test]
    fn same_package_repair_keeps_the_previous_rollback_and_cleanup_version() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let installed_a = receipt(&a, None);
        let installed_b = receipt(&b, previous_package(&installed_a, &b).as_deref());
        assert_eq!(installed_b.previous.as_ref(), Some(&a));
        let repaired_b = receipt(&b, previous_package(&installed_b, &b).as_deref());
        assert_eq!(repaired_b.previous.as_ref(), Some(&a));
        assert_eq!(previous_package(&repaired_b, &a), Some(b.clone()));
        assert_eq!(previous_package(&receipt(&b, Some(&b)), &b), None);
    }
}
