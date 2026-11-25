use super::{profile_path, registry, tip};
use crate::Result;
use keyrill_windows::profiles::{PROFILES, SERVICE_ID};
use serde_json::{Value, json};
use std::path::Path;
use windows::Win32::System::Com::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL};
use windows::Win32::UI::TextServices::*;
use windows_core::Interface;

pub(super) struct Com;
impl Com {
    pub(super) fn new() -> Result<Self> {
        // SAFETY: setup initializes COM on its sole owner thread and balances success in Drop.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: the single successful initialization belongs to this thread and owner.
        unsafe {
            CoUninitialize();
        }
    }
}
fn profiles() -> Result<ITfInputProcessorProfiles> {
    // SAFETY: COM is initialized and the requested SDK interface owns its returned reference.
    Ok(unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }?)
}
fn categories() -> Result<ITfCategoryMgr> {
    // SAFETY: COM is initialized and the returned interface is retained by its Rust owner.
    Ok(unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) }?)
}
fn both(mut inspect: impl FnMut(REG_SAM_FLAGS) -> Result<Value>) -> Result<Value> {
    let first = inspect(KEY_WOW64_64KEY)?;
    if inspect(KEY_WOW64_32KEY)? != first {
        return Err("machine TSF views differ; scope or partial effect requires inspection".into());
    }
    Ok(first)
}
pub(super) fn service_state() -> Result<Value> {
    service_state_with(|view| {
        Ok((
            registry::exists(HKEY_LOCAL_MACHINE, view, &tip())?,
            service_metadata_at(HKEY_LOCAL_MACHINE, view, &tip())?,
        ))
    })
}
fn service_state_with(
    mut inspect: impl FnMut(REG_SAM_FLAGS) -> Result<(bool, Value)>,
) -> Result<Value> {
    let first = inspect(KEY_WOW64_64KEY)?;
    if inspect(KEY_WOW64_32KEY)? != first {
        return Err(format!("machine TSF service root {} Enable or presence differs between registry views; no mutation permitted", tip()).into());
    }
    if !first.0 && !first.1.is_null() {
        return Err("machine TSF service root disappeared during Enable readback".into());
    }
    Ok(json!(first.0))
}
fn service_metadata_at(hive: HKEY, view: REG_SAM_FLAGS, path: &str) -> Result<Value> {
    let names = registry::names(hive, view, path)?;
    let enable = registry::read(hive, view, path, "Enable")?;
    validate_service_metadata(path, &names, &enable)?;
    Ok(enable)
}
fn validate_service_metadata(path: &str, names: &[String], enable: &Value) -> Result<()> {
    if names
        .iter()
        .any(|name| !name.eq_ignore_ascii_case("Enable"))
    {
        return Err(format!("machine TSF service root {path} has an unexpected value name; only optional Enable is recognized; no mutation permitted").into());
    }
    if (names.is_empty() && enable.is_null())
        || (names.len() == 1 && *enable == registry::string("1"))
    {
        return Ok(());
    }
    Err(format!("machine TSF service root {path} Enable has invalid type or data, or changed during readback; expected absent or exact REG_SZ 1; no mutation permitted").into())
}
pub(super) fn service_write(value: &Value) -> Result<()> {
    let enabled = value.as_bool().ok_or("invalid service desired state")?;
    scope_preflight()?;
    service_write_with(&profiles()?, enabled)
}
fn service_write_with(manager: &ITfInputProcessorProfiles, enabled: bool) -> Result<()> {
    // SAFETY: only Keyrill's stable service is changed; preflight excludes foreign descendants.
    unsafe {
        if enabled {
            manager.Register(&SERVICE_ID)?;
        } else {
            manager.Unregister(&SERVICE_ID)?;
        }
    }
    Ok(())
}
pub(super) fn profile_state(index: usize) -> Result<Value> {
    both(|view| {
        let path = profile_path(index);
        let description = registry::read(HKEY_LOCAL_MACHINE, view, &path, "Description")?;
        let icon = registry::read(HKEY_LOCAL_MACHINE, view, &path, "IconFile")?;
        let icon_index = registry::read(HKEY_LOCAL_MACHINE, view, &path, "IconIndex")?;
        if description.is_null() && icon.is_null() && icon_index.is_null() {
            return Ok(Value::Null);
        }
        if icon_index != registry::dword(0) {
            return Err("profile icon index differs from owned resource".into());
        }
        Ok(
            json!({"description":registry::decode_string(&description)?,"icon":registry::decode_string(&icon)?,"index":0}),
        )
    })
}
pub(super) fn profile_desired(index: usize, icon: &Path) -> Result<Value> {
    Ok(
        json!({"description":PROFILES[index].description,"icon":icon.to_str().ok_or("non-Unicode icon path")?,"index":0}),
    )
}
pub(super) fn repair_snapshot(index: usize) -> Result<(Value, Value)> {
    let mut raw = Vec::new();
    let mut trees = Vec::new();
    let mut com = Vec::new();
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        let mut profile = serde_json::Map::new();
        for name in ["Description", "IconFile", "IconIndex"] {
            profile.insert(
                name.into(),
                registry::read(HKEY_LOCAL_MACHINE, view, &profile_path(index), name)?,
            );
        }
        raw.push(Value::Object(profile));
        for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            let mut budget = 64;
            let mut tree = registry_tree(hive, view, &tip(), 0, &mut budget)?;
            if hive == HKEY_LOCAL_MACHINE {
                let p = &PROFILES[index];
                let values = tree["children"]["languageprofile"]["children"]
                    [format!("0x{:08x}", p.language)]["children"]
                    [format!("{{{:?}}}", p.id).to_lowercase()]["values"]
                    .as_object_mut()
                    .ok_or("repair target profile tree absent")?;
                for name in ["description", "iconfile", "iconindex"] {
                    values.remove(name);
                }
            }
            trees.push(tree);
            let mut budget = 64;
            let class = super::com_path();
            let class = class
                .strip_suffix("\\InprocServer32")
                .ok_or("COM class root absent")?;
            com.push(registry_tree(hive, view, class, 0, &mut budget)?);
        }
    }
    if raw[0] != raw[1] {
        return Err("repair raw machine profile views differ".into());
    }
    let mut layouts = [HKL::default(); 256];
    // SAFETY: the output slice is bounded and querying does not select a source.
    let count = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
    if count <= 0 || count as usize >= layouts.len() {
        return Err("repair loaded layouts unavailable".into());
    }
    let mut layouts: Vec<_> = layouts[..count as usize]
        .iter()
        .map(|layout| layout.0 as usize as u64)
        .collect();
    layouts.sort_unstable();
    let manager: ITfInputProcessorProfileMgr = profiles()?.cast()?;
    let mut active = TF_INPUTPROCESSORPROFILE::default();
    // SAFETY: COM owns the profile manager and the output is an SDK value owned by this call.
    let status = unsafe {
        (manager.vtable().GetActiveProfile)(manager.as_raw(), &GUID_TFCAT_TIP_KEYBOARD, &mut active)
    };
    if status != windows::Win32::Foundation::S_OK && status != windows::Win32::Foundation::S_FALSE {
        return Err("repair owner-thread active source unavailable".into());
    }
    // SAFETY: zero queries this owner thread and performs no source selection.
    let keyboard = unsafe { GetKeyboardLayout(0) }.0 as usize as u64;
    let source = json!({"scope":"setup-owner-thread","keyboard":keyboard,"status":status.0,
        "type":active.dwProfileType,"language":active.langid,"service":format!("{:?}",active.clsid),
        "profile":format!("{:?}",active.guidProfile),"category":format!("{:?}",active.catid),
        "substitute":active.hklSubstitute.0 as usize as u64,"layout":active.hkl.0 as usize as u64,
        "caps":active.dwCaps,"flags":active.dwFlags});
    Ok((
        json!(raw),
        json!({"trees":trees,"com":com,"user":super::files::user_sid()?,"layouts":layouts,"source":source}),
    ))
}

fn registry_tree(
    hive: HKEY,
    view: REG_SAM_FLAGS,
    path: &str,
    depth: usize,
    budget: &mut usize,
) -> Result<Value> {
    registry_tree_bounded(hive, view, path, depth, budget, &mut (512 * 1024))
}
fn registry_tree_bounded(
    hive: HKEY,
    view: REG_SAM_FLAGS,
    path: &str,
    depth: usize,
    budget: &mut usize,
    bytes: &mut usize,
) -> Result<Value> {
    if depth > 8 || *budget == 0 {
        return Err("repair registration tree exceeds bound".into());
    }
    *budget -= 1;
    let mut values = serde_json::Map::new();
    let mut value_names = registry::names(hive, view, path)?;
    value_names.sort();
    for name in &value_names {
        let value = registry::read(hive, view, path, name)?;
        *bytes = bytes
            .checked_sub(serde_json::to_vec(&value)?.len() + 2 * name.len())
            .ok_or("repair registry data exceeds snapshot bound")?;
        values.insert(name.to_lowercase(), value);
    }
    let mut children = serde_json::Map::new();
    let mut child_names = registry::children(hive, view, path)?;
    child_names.sort();
    for name in &child_names {
        *bytes = bytes
            .checked_sub(2 * name.len())
            .ok_or("repair registry names exceed snapshot bound")?;
        children.insert(
            name.to_lowercase(),
            registry_tree_bounded(
                hive,
                view,
                &format!("{path}\\{name}"),
                depth + 1,
                budget,
                bytes,
            )?,
        );
    }
    Ok(
        json!({"present":registry::exists(hive, view, path)?,"value_names":value_names,"child_names":child_names,"values":values,"children":children}),
    )
}
pub(super) fn profile_write(index: usize, value: &Value) -> Result<()> {
    let manager = profiles()?;
    profile_write_with(&manager, index, value)
}
fn profile_buffer(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn profile_write_with(
    manager: &ITfInputProcessorProfiles,
    index: usize,
    value: &Value,
) -> Result<()> {
    let profile = &PROFILES[index];
    if value.is_null() {
        // SAFETY: only this immutable service/language/profile tuple is removed.
        unsafe {
            manager.RemoveLanguageProfile(&SERVICE_ID, profile.language, &profile.id)?;
        }
    } else {
        let description = profile_buffer(
            value["description"]
                .as_str()
                .ok_or("profile description missing")?,
        );
        let icon = profile_buffer(value["icon"].as_str().ok_or("profile icon missing")?);
        if value["index"] != 0 {
            return Err("profile icon index invalid".into());
        }
        // SAFETY: GUIDs and NUL-terminated storage stay live. SDK slice lengths exclude the NUL.
        unsafe {
            manager.AddLanguageProfile(
                &SERVICE_ID,
                profile.language,
                &profile.id,
                &description[..description.len() - 1],
                &icon[..icon.len() - 1],
                0,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tsf_tests.rs"]
mod tests;
pub(super) fn layout_state(index: usize) -> Result<Value> {
    both(|view| {
        let value = registry::read(
            HKEY_LOCAL_MACHINE,
            view,
            &profile_path(index),
            "SubstituteLayout",
        )?;
        if value.is_null() {
            Ok(Value::Null)
        } else {
            Ok(json!(registry::decode_string(&value)?))
        }
    })
}
pub(super) fn layout_write(index: usize, value: &Value) -> Result<()> {
    layout_write_with(&profiles()?, index, value, loaded_layout(index)?)
}
fn layout_write_with(
    manager: &ITfInputProcessorProfiles,
    index: usize,
    value: &Value,
    hkl: HKL,
) -> Result<()> {
    let profile = &PROFILES[index];
    let expected = format!("0x{:04x}{:04x}", profile.language, profile.language);
    if value.as_str() != Some(&expected) {
        return Err("unsupported substitute layout desired state".into());
    }
    // SAFETY: the exact standard layout handle was returned by the native loaded-layout query.
    unsafe {
        manager.SubstituteKeyboardLayout(&SERVICE_ID, profile.language, &profile.id, hkl)?;
    }
    Ok(())
}
pub(super) fn loaded_layout(index: usize) -> Result<HKL> {
    let mut layouts = [HKL::default(); 256];
    // SAFETY: this bounded slice is writable native HKL storage. Querying selects no source.
    let count = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
    if count <= 0 || count as usize >= layouts.len() {
        return Err("loaded keyboard layouts unavailable or excessive".into());
    }
    standard_layout(&layouts[..count as usize], PROFILES[index].language)
        .ok_or_else(||"required standard US or Russian keyboard layout is not loaded; add it in Windows Settings before installing Keyrill".into())
}

fn standard_layout(layouts: &[HKL], language: u16) -> Option<HKL> {
    let language = usize::from(language);
    let identity = (language << 16) | language;
    layouts
        .iter()
        .copied()
        .find(|layout| layout.0 as usize == identity)
}
pub(super) fn category_state() -> Result<Value> {
    both(|view| {
        let category = format!(
            "{}\\Category\\Category\\{{{GUID_TFCAT_TIP_KEYBOARD:?}}}\\{{{SERVICE_ID:?}}}",
            tip()
        );
        let item = format!(
            "{}\\Category\\Item\\{{{SERVICE_ID:?}}}\\{{{GUID_TFCAT_TIP_KEYBOARD:?}}}",
            tip()
        );
        let first = registry::exists(HKEY_LOCAL_MACHINE, view, &category)?;
        if registry::exists(HKEY_LOCAL_MACHINE, view, &item)? != first {
            return Err("partial category registration requires inspection".into());
        }
        Ok(json!(first))
    })
}
pub(super) fn category_write(value: &Value) -> Result<()> {
    let enabled = value.as_bool().ok_or("invalid category desired state")?;
    let manager = categories()?;
    // SAFETY: the exact immutable service/category tuple is retained throughout the call.
    unsafe {
        if enabled {
            manager.RegisterCategory(&SERVICE_ID, &GUID_TFCAT_TIP_KEYBOARD, &SERVICE_ID)?;
        } else {
            manager.UnregisterCategory(&SERVICE_ID, &GUID_TFCAT_TIP_KEYBOARD, &SERVICE_ID)?;
        }
    }
    Ok(())
}
fn only(actual: Vec<String>, expected: &[String]) -> Result<()> {
    if actual.iter().any(|name| {
        !expected
            .iter()
            .any(|value| value.eq_ignore_ascii_case(name))
    }) {
        return Err("unrecognized owned registration descendant; no mutation permitted".into());
    }
    Ok(())
}
pub(super) fn scope_preflight() -> Result<()> {
    scope_preflight_except(None)
}
pub(super) fn scope_preflight_except(profile: Option<usize>) -> Result<()> {
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        only(
            registry::children(HKEY_LOCAL_MACHINE, view, &tip())?,
            &["Category".into(), "LanguageProfile".into()],
        )?;
        service_metadata_at(HKEY_LOCAL_MACHINE, view, &tip())?;
        let languages = format!("{}\\LanguageProfile", tip());
        only(registry::names(HKEY_LOCAL_MACHINE, view, &languages)?, &[])?;
        only(registry::names(HKEY_CURRENT_USER, view, &tip())?, &[])?;
        only(
            registry::children(HKEY_CURRENT_USER, view, &tip())?,
            &["LanguageProfile".into()],
        )?;
        only(registry::names(HKEY_CURRENT_USER, view, &languages)?, &[])?;
        only(
            registry::children(HKEY_CURRENT_USER, view, &languages)?,
            &PROFILES.map(|p| format!("0x{:08x}", p.language)),
        )?;
        only(
            registry::children(HKEY_LOCAL_MACHINE, view, &languages)?,
            &PROFILES.map(|p| format!("0x{:08x}", p.language)),
        )?;
        for (index, p) in PROFILES.iter().enumerate() {
            let language_path = format!("{languages}\\0x{:08x}", p.language);
            for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
                only(registry::names(hive, view, &language_path)?, &[])?;
                only(
                    registry::children(hive, view, &language_path)?,
                    &[format!("{{{:?}}}", p.id)],
                )?;
            }
            only(
                registry::children(
                    HKEY_LOCAL_MACHINE,
                    view,
                    &format!("{languages}\\0x{:08x}", p.language),
                )?,
                &[format!("{{{:?}}}", p.id)],
            )?;
            let path = profile_path(index);
            only(
                registry::names(HKEY_LOCAL_MACHINE, view, &path)?,
                &["Description", "IconFile", "IconIndex", "SubstituteLayout"].map(str::to_owned),
            )?;
            only(registry::children(HKEY_LOCAL_MACHINE, view, &path)?, &[])?;
            only(registry::children(HKEY_CURRENT_USER, view, &path)?, &[])?;
            only(
                registry::names(HKEY_CURRENT_USER, view, &path)?,
                &["Enable".into()],
            )?;
        }
        let category = format!("{}\\Category", tip());
        only(registry::names(HKEY_LOCAL_MACHINE, view, &category)?, &[])?;
        only(
            registry::children(HKEY_LOCAL_MACHINE, view, &category)?,
            &["Category".into(), "Item".into()],
        )?;
        for (branch, first, second) in [
            ("Category", GUID_TFCAT_TIP_KEYBOARD, SERVICE_ID),
            ("Item", SERVICE_ID, GUID_TFCAT_TIP_KEYBOARD),
        ] {
            let path = format!("{category}\\{branch}");
            for exact in [
                &path,
                &format!("{path}\\{{{first:?}}}"),
                &format!("{path}\\{{{first:?}}}\\{{{second:?}}}"),
            ] {
                only(registry::names(HKEY_LOCAL_MACHINE, view, exact)?, &[])?;
            }
            only(
                registry::children(HKEY_LOCAL_MACHINE, view, &path)?,
                &[format!("{{{first:?}}}")],
            )?;
            only(
                registry::children(HKEY_LOCAL_MACHINE, view, &format!("{path}\\{{{first:?}}}"))?,
                &[format!("{{{second:?}}}")],
            )?;
            only(
                registry::children(
                    HKEY_LOCAL_MACHINE,
                    view,
                    &format!("{path}\\{{{first:?}}}\\{{{second:?}}}"),
                )?,
                &[],
            )?;
        }
    }
    service_state()?;
    category_state()?;
    for i in 0..2 {
        if profile != Some(i) {
            profile_state(i)?;
        }
        layout_state(i)?;
    }
    Ok(())
}
pub(super) fn is_proof_resource(
    resource: &str,
    before: &Value,
    proof: Option<&Path>,
) -> Result<bool> {
    let Some(proof) = proof else {
        return Ok(false);
    };
    match resource {
        "service" | "category" => Ok(before == true),
        "layout/en" => Ok(before == "0x04090409"),
        "layout/ru" => Ok(before == "0x04190419"),
        "profile/en" | "profile/ru" => {
            let description = if resource == "profile/en" {
                "Keyrill proof EN"
            } else {
                "Keyrill proof RU"
            };
            let icon = before["icon"].as_str().ok_or("proof icon path absent")?;
            Ok(before["description"] == description
                && before["index"] == 0
                && Path::new(icon).canonicalize()? == proof)
        }
        _ => Ok(false),
    }
}
