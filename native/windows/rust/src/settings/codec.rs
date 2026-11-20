//! Private canonical little-endian complete-value codec.

use super::*;

pub(super) fn encode(snapshot: &Snapshot) -> Result<Vec<u8>, SettingsError> {
    let nonce = snapshot.revision.0.ok_or(SettingsError::Invalid)?;
    if nonce == [0; 16] || snapshot.value.applications.len() > MAX_APPLICATIONS {
        return Err(SettingsError::Invalid);
    }
    let mut bytes = b"KRST\x01\0\0\0\0\0\0\0".to_vec();
    bytes.extend(nonce);
    bytes.push(u8::from(snapshot.value.automatic));
    bytes.push(u8::from(snapshot.value.switch_source));
    for source in &snapshot.value.dictionaries {
        match source {
            DictionaryPath::Default => bytes.push(0),
            DictionaryPath::Explicit(path) => {
                bytes.push(1);
                push_text(&mut bytes, path.as_str())?;
            }
        }
    }
    bytes.extend((snapshot.value.applications.len() as u16).to_le_bytes());
    for (identity, policy) in &snapshot.value.applications {
        bytes.push(match identity.0 {
            Identity::Executable(_) => 0,
            Identity::Package(_) => 1,
        });
        push_text(&mut bytes, identity.as_str())?;
        bytes.push(match policy {
            AppPolicy::Automatic => 0,
            AppPolicy::Manual => 1,
            AppPolicy::Off => 2,
        });
    }
    if bytes.len() > MAX_BYTES {
        return Err(SettingsError::Invalid);
    }
    let length = (bytes.len() as u32).to_le_bytes();
    bytes[8..12].copy_from_slice(&length);
    Ok(bytes)
}

fn push_text(bytes: &mut Vec<u8>, text: &str) -> Result<(), SettingsError> {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() > 4095 || bytes.len() + 2 + units.len() * 2 > MAX_BYTES {
        return Err(SettingsError::Invalid);
    }
    bytes.extend((units.len() as u16).to_le_bytes());
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    Ok(())
}

pub(super) fn decode(bytes: &[u8]) -> Result<Snapshot, SettingsError> {
    if bytes.len() > MAX_BYTES || bytes.len() < 34 || bytes[..8] != *b"KRST\x01\0\0\0" {
        return Err(SettingsError::Invalid);
    }
    let mut input = Input(bytes);
    input.take(8)?;
    let length = u32::from_le_bytes(
        input
            .take(4)?
            .try_into()
            .map_err(|_| SettingsError::Invalid)?,
    );
    if length as usize != bytes.len() {
        return Err(SettingsError::Invalid);
    }
    let revision = Revision(Some(
        input
            .take(16)?
            .try_into()
            .map_err(|_| SettingsError::Invalid)?,
    ));
    let automatic = input.boolean()?;
    let switch_source = input.boolean()?;
    let mut dictionaries = [DictionaryPath::Default, DictionaryPath::Default];
    for source in &mut dictionaries {
        *source = match input.byte()? {
            0 => DictionaryPath::Default,
            1 => DictionaryPath::Explicit(LocalPath::new(&input.text()?)?),
            _ => return Err(SettingsError::Invalid),
        };
    }
    let count = input.short()? as usize;
    if count > MAX_APPLICATIONS {
        return Err(SettingsError::Invalid);
    }
    let mut applications = BTreeMap::new();
    for _ in 0..count {
        let kind = input.byte()?;
        let text = input.text()?;
        let identity = match kind {
            0 => ApplicationIdentity::executable(&text)?,
            1 => ApplicationIdentity::package_family(&text)?,
            _ => return Err(SettingsError::Invalid),
        };
        let policy = match input.byte()? {
            0 => AppPolicy::Automatic,
            1 => AppPolicy::Manual,
            2 => AppPolicy::Off,
            _ => return Err(SettingsError::Invalid),
        };
        if applications.insert(identity, policy).is_some() {
            return Err(SettingsError::Invalid);
        }
    }
    let snapshot = Snapshot {
        revision,
        value: Arc::new(Preferences {
            automatic,
            switch_source,
            dictionaries,
            applications,
        }),
    };
    if encode(&snapshot)? != bytes {
        return Err(SettingsError::Invalid);
    }
    Ok(snapshot)
}

struct Input<'a>(&'a [u8]);

impl<'a> Input<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], SettingsError> {
        let (taken, rest) = self
            .0
            .split_at_checked(length)
            .ok_or(SettingsError::Invalid)?;
        self.0 = rest;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, SettingsError> {
        Ok(self.take(1)?[0])
    }

    fn short(&mut self) -> Result<u16, SettingsError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| SettingsError::Invalid)?,
        ))
    }

    fn boolean(&mut self) -> Result<bool, SettingsError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SettingsError::Invalid),
        }
    }

    fn text(&mut self) -> Result<String, SettingsError> {
        let count = self.short()? as usize;
        if count > 4095 {
            return Err(SettingsError::Invalid);
        }
        let units: Vec<u16> = self
            .take(count * 2)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units).map_err(|_| SettingsError::Invalid)
    }
}
