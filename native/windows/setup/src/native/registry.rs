use crate::Result;
use serde_json::{Value, json};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND};
use windows::Win32::System::Registry::*;
use windows_core::{PCWSTR, PWSTR};

pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: this owner closes its single successfully opened key exactly once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}
fn open(hive: HKEY, view: REG_SAM_FLAGS, path: &str, access: REG_SAM_FLAGS) -> Result<Option<Key>> {
    let path = wide(path);
    let mut key = HKEY::default();
    // SAFETY: the path is terminated and output owns distinct storage.
    let result =
        unsafe { RegOpenKeyExW(hive, PCWSTR(path.as_ptr()), None, access | view, &mut key) };
    if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    result.ok()?;
    Ok(Some(Key(key)))
}
pub(super) fn exists(hive: HKEY, view: REG_SAM_FLAGS, path: &str) -> Result<bool> {
    Ok(open(hive, view, path, KEY_READ)?.is_some())
}
pub(super) fn require_write(hive: HKEY, view: REG_SAM_FLAGS, path: &str) -> Result<()> {
    open(hive, view, path, KEY_READ | KEY_WRITE)?
        .ok_or("machine registration scope unavailable")?;
    Ok(())
}
pub(super) fn read(hive: HKEY, view: REG_SAM_FLAGS, path: &str, name: &str) -> Result<Value> {
    let Some(key) = open(hive, view, path, KEY_QUERY_VALUE)? else {
        return Ok(Value::Null);
    };
    let name = wide(name);
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0;
    // SAFETY: key is retained; this query only measures a value and writes distinct outputs.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(Value::Null);
    }
    status.ok()?;
    if size > 65536 {
        return Err("owned registry value exceeds bound".into());
    }
    let mut data = vec![0u8; size as usize];
    // SAFETY: data spans the advertised size and key/name remain live.
    unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    }
    .ok()?;
    data.truncate(size as usize);
    Ok(json!({"type":kind.0,"data":data}))
}
pub(super) fn string(value: &str) -> Value {
    json!({"type":REG_SZ.0,"data":wide(value).into_iter().flat_map(u16::to_le_bytes).collect::<Vec<_>>()})
}
pub(super) fn dword(value: u32) -> Value {
    json!({"type":REG_DWORD.0,"data":value.to_le_bytes()})
}
pub(super) fn decode_string(value: &Value) -> Result<String> {
    if value["type"] != REG_SZ.0 {
        return Err("registry string type mismatch".into());
    }
    let data = bytes(value)?;
    if data.len() < 2 || data.len() % 2 != 0 || data[data.len() - 2..] != [0, 0] {
        return Err("registry string malformed".into());
    }
    let units: Vec<_> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    if units[..units.len() - 1].contains(&0) {
        return Err("registry string contains embedded terminator".into());
    }
    Ok(String::from_utf16(&units[..units.len() - 1])?)
}
fn bytes(value: &Value) -> Result<Vec<u8>> {
    let values = value["data"]
        .as_array()
        .filter(|v| v.len() <= 65536)
        .ok_or("registry receipt data invalid")?;
    values
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| "registry receipt byte invalid".into())
        })
        .collect()
}
pub(super) fn write(
    hive: HKEY,
    view: REG_SAM_FLAGS,
    path: &str,
    name: &str,
    value: &Value,
) -> Result<()> {
    let name = wide(name);
    if value.is_null() {
        let Some(key) = open(hive, view, path, KEY_SET_VALUE)? else {
            return Ok(());
        };
        // SAFETY: deletes only the exact journaled value, never an enclosing tree.
        let result = unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) };
        if result != ERROR_FILE_NOT_FOUND {
            result.ok()?;
        }
        return Ok(());
    }
    let kind = REG_VALUE_TYPE(u32::try_from(
        value["type"]
            .as_u64()
            .ok_or("registry receipt type invalid")?,
    )?);
    if ![REG_SZ, REG_DWORD].contains(&kind) {
        return Err("unsupported owned registry value type".into());
    }
    let data = bytes(value)?;
    let path = wide(path);
    let mut key = HKEY::default();
    // SAFETY: fixed authorized hive/path, terminated input and distinct key output.
    unsafe {
        RegCreateKeyExW(
            hive,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | view,
            None,
            &mut key,
            None,
        )
    }
    .ok()?;
    let key = Key(key);
    // SAFETY: the exact typed bytes and key remain live through the call.
    unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, kind, Some(&data)) }.ok()?;
    // SAFETY: flushing this retained key makes the completed registry effect durable.
    unsafe { RegFlushKey(key.0) }.ok()?;
    Ok(())
}
pub(super) fn children(hive: HKEY, view: REG_SAM_FLAGS, path: &str) -> Result<Vec<String>> {
    let Some(key) = open(hive, view, path, KEY_READ)? else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for index in 0..128 {
        let mut data = [0u16; 256];
        let mut size = data.len() as u32;
        // SAFETY: the retained key and bounded output buffer are valid for enumeration.
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(data.as_mut_ptr())),
                &mut size,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            return Ok(result);
        }
        status.ok()?;
        result.push(String::from_utf16(&data[..size as usize])?);
    }
    Err("owned registry subkey enumeration exceeded bound".into())
}
pub(super) fn names(hive: HKEY, view: REG_SAM_FLAGS, path: &str) -> Result<Vec<String>> {
    let Some(key) = open(hive, view, path, KEY_READ)? else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for index in 0..128 {
        let mut data = [0u16; 256];
        let mut size = data.len() as u32;
        // SAFETY: this bounded enumeration reads names only from the retained key.
        let status = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(data.as_mut_ptr())),
                &mut size,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            return Ok(result);
        }
        status.ok()?;
        result.push(String::from_utf16(&data[..size as usize])?);
    }
    Err("owned registry value enumeration exceeded bound".into())
}

#[cfg(test)]
pub(super) fn remove_empty_key(hive: HKEY, view: REG_SAM_FLAGS, path: &str) -> Result<()> {
    if !names(hive, view, path)?.is_empty() || !children(hive, view, path)?.is_empty() {
        return Err("disposable registry key is not empty".into());
    }
    let terminated = wide(path);
    // SAFETY: the caller owns this exact fresh test path. The preceding empty-key
    // inspection is separate from deletion and cannot exclude concurrent writers.
    let status = unsafe { RegDeleteKeyExW(hive, PCWSTR(terminated.as_ptr()), view.0, None) };
    if status != ERROR_FILE_NOT_FOUND && status != ERROR_PATH_NOT_FOUND {
        status.ok()?;
    }
    if exists(hive, view, path)? {
        return Err("disposable registry key remains after deletion".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_native_registry_preserves_unrelated_values_across_exact_write_delete() {
        let path = format!("Software\\KeyrillTests\\Setup-{}", std::process::id());
        assert!(!exists(HKEY_CURRENT_USER, KEY_WOW64_64KEY, &path).unwrap());
        write(
            HKEY_CURRENT_USER,
            KEY_WOW64_64KEY,
            &path,
            "unrelated",
            &dword(42),
        )
        .unwrap();
        for value in [
            string("Keyrill Русский"),
            string("verified replacement"),
            Value::Null,
        ] {
            write(HKEY_CURRENT_USER, KEY_WOW64_64KEY, &path, "owned", &value).unwrap();
            assert_eq!(
                read(HKEY_CURRENT_USER, KEY_WOW64_64KEY, &path, "owned").unwrap(),
                value
            );
            assert_eq!(
                read(HKEY_CURRENT_USER, KEY_WOW64_64KEY, &path, "unrelated").unwrap(),
                dword(42)
            );
        }
        write(
            HKEY_CURRENT_USER,
            KEY_WOW64_64KEY,
            &path,
            "unrelated",
            &Value::Null,
        )
        .unwrap();
        let path = wide(&path);
        // SAFETY: the uniquely named isolated test key now has no values or subkeys.
        unsafe {
            RegDeleteKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                KEY_WOW64_64KEY.0,
                None,
            )
        }
        .ok()
        .unwrap();
    }
    #[test]
    fn malformed_native_string_readbacks_are_not_coerced() {
        for value in [
            json!({"type":1,"data":[65]}),
            json!({"type":1,"data":[65,0]}),
            json!({"type":1,"data":[0,0,65,0,0,0]}),
            dword(0),
        ] {
            assert!(decode_string(&value).is_err());
        }
        assert_eq!(decode_string(&string("Я")).unwrap(), "Я");
    }
}
