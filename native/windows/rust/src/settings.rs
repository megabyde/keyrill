//! Complete, bounded per-user preferences. Call storage operations outside input callbacks.
//!
//! ```
//! use keyrill_windows::settings::{Preferences, Reader};
//! let reader = Reader::default();
//! assert_eq!(reader.snapshot().value.as_ref(), &Preferences::default());
//! ```

mod codec;
mod native;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

const MAX_APPLICATIONS: usize = 256;
const MAX_BYTES: usize = 1024 * 1024;

/// Content-free validation and storage failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsError {
    /// The complete candidate violates the supported schema or bounds.
    Invalid,
    /// Native storage, entropy or writer serialization is unavailable.
    Unavailable,
    /// Existing writer-object security differs from the private user-only contract.
    Security,
    /// A submitted write could not be verified by exact fresh readback.
    Readback,
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "invalid settings",
            Self::Unavailable => "settings unavailable",
            Self::Security => "settings writer security rejected",
            Self::Readback => "settings write unverified",
        })
    }
}

impl std::error::Error for SettingsError {}

/// Validated absolute drive-local path with bounded ordinary Windows components.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalPath(String);

impl LocalPath {
    /// Validates a path using the vocabulary worker's local-path grammar.
    ///
    /// # Errors
    /// Returns [`SettingsError::Invalid`] for relative, device, network or unbounded paths.
    pub fn new(text: &str) -> Result<Self, SettingsError> {
        let units: Vec<u16> = text.encode_utf16().take(4096).collect();
        crate::vocabulary::parse_path(&units).ok_or(SettingsError::Invalid)?;
        let mut text = text.to_owned();
        text[..1].make_ascii_uppercase();
        Ok(Self(text))
    }

    /// Returns private configuration text for the settings UI and vocabulary worker.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LocalPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LocalPath(<private>)")
    }
}

/// A default dictionary location or an explicitly selected local file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DictionaryPath {
    /// Resolve the ordinary per-user default on the existing vocabulary worker.
    #[default]
    Default,
    /// Use this exact user-selected local path.
    Explicit(LocalPath),
}

/// Restrictive application preference. Native field admission is always required.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPolicy {
    /// Permit inference when the global automatic preference is enabled.
    Automatic,
    /// Track ordinary owned compositions with inference disabled and manual commands available.
    Manual,
    /// Inspect no text and start no composition in this application.
    Off,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Identity {
    Executable(LocalPath),
    Package(String),
}

/// Canonical native executable path or package-family identity acquired outside callbacks.
/// Constructors validate syntax only. Callers obtain the identity from native process metadata.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ApplicationIdentity(Identity);

impl ApplicationIdentity {
    /// Canonicalizes a native executable path with invariant Windows uppercase mapping.
    /// Callers resolve the actual executable path before constructing this identity.
    ///
    /// # Errors
    /// Returns an error if the path or native case mapping is unavailable or invalid.
    pub fn executable(path: &str) -> Result<Self, SettingsError> {
        let path = LocalPath::new(path)?;
        Ok(Self(Identity::Executable(LocalPath::new(
            &native::uppercase(path.as_str())?,
        )?)))
    }

    /// Canonicalizes a native package-family name to ASCII lowercase.
    ///
    /// # Errors
    /// Returns [`SettingsError::Invalid`] for an invalid or oversized family name.
    pub fn package_family(family: &str) -> Result<Self, SettingsError> {
        if family.len() > 255 {
            return Err(SettingsError::Invalid);
        }
        let (name, publisher) = family.split_once('_').ok_or(SettingsError::Invalid)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
            || publisher.len() != 13
            || !publisher.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(SettingsError::Invalid);
        }
        Ok(Self(Identity::Package(family.to_ascii_lowercase())))
    }

    /// Returns private identity text for an application settings row.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Identity::Executable(path) => path.as_str(),
            Identity::Package(family) => family,
        }
    }
}

impl fmt::Debug for ApplicationIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApplicationIdentity(<private>)")
    }
}

/// A complete candidate. Save validates total size and application count before any write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    /// Enable automatic inference under native admission and application policy.
    pub automatic: bool,
    /// Request source switching after verified correction.
    pub switch_source: bool,
    /// English and Russian dictionary choices, in that order.
    pub dictionaries: [DictionaryPath; 2],
    /// Optional application overrides keyed by canonical native identity.
    pub applications: BTreeMap<ApplicationIdentity, AppPolicy>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            automatic: false,
            switch_source: true,
            dictionaries: [DictionaryPath::Default, DictionaryPath::Default],
            applications: BTreeMap::new(),
        }
    }
}

/// Equality-only persisted revision. Every saved value contains a fresh native random nonce.
/// Missing state compares equal only to current absence, including after create then delete.
/// A saved revision always conflicts with absence or a recreated value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Revision(Option<[u8; 16]>);

/// One complete validated settings value and its compare-and-save revision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Opaque revision supplied unchanged to Save.
    pub revision: Revision,
    /// Complete immutable preferences.
    pub value: Arc<Preferences>,
}

/// The result of one complete compare-and-save operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    /// The complete write matched exact fresh native readback.
    Saved(Snapshot),
    /// Current validated state has a different revision. No write was submitted.
    Conflict(Snapshot),
    /// Validation, native storage, writer security or readback failed.
    Rejected(SettingsError),
}

/// Concrete per-user registry store shared by both process bitnesses.
pub struct Store {
    native: native::Store,
}

impl Store {
    /// Opens an explicitly isolated synthetic registry namespace for controlled DLL schedules.
    /// This feature-gated provider never addresses the user's production preference value.
    ///
    /// # Errors
    /// Returns an error for an invalid namespace or unavailable native user identity.
    #[cfg(any(test, feature = "test-api"))]
    pub fn isolated_test(namespace: &str) -> Result<Self, SettingsError> {
        Ok(Self {
            native: native::Store::isolated_test(namespace)?,
        })
    }

    /// Selects the current process user's store without reading or writing preferences.
    ///
    /// # Errors
    /// Returns an error when the native user identity cannot be acquired.
    pub fn current_user() -> Result<Self, SettingsError> {
        Ok(Self {
            native: native::Store::current_user()?,
        })
    }

    /// Reads one bounded complete value. Absence returns the shipping defaults.
    /// This operation has no opening or initialization write side effect.
    ///
    /// # Errors
    /// Returns an error for denied access, invalid type, unknown schema or incomplete data.
    pub fn load(&self) -> Result<Snapshot, SettingsError> {
        match self.native.read()? {
            None => Ok(Snapshot::default()),
            Some(bytes) => codec::decode(&bytes),
        }
    }

    /// Validates and serializes a complete candidate, reloads under the private writer mutex,
    /// compares revisions, submits one value replacement and verifies exact fresh readback.
    /// An abandoned mutex follows the same fresh reload. Readback failure has unknown effect.
    pub fn save(&self, expected: Revision, value: Preferences) -> SaveOutcome {
        match self.save_checked(expected, value) {
            Ok(outcome) => outcome,
            Err(error) => SaveOutcome::Rejected(error),
        }
    }

    fn save_checked(
        &self,
        expected: Revision,
        value: Preferences,
    ) -> Result<SaveOutcome, SettingsError> {
        let snapshot = Snapshot {
            revision: Revision(Some(native::nonce()?)),
            value: Arc::new(value),
        };
        let encoded = codec::encode(&snapshot)?;
        let _writer = self.native.lock()?;
        let current = self.load()?;
        if current.revision != expected {
            return Ok(SaveOutcome::Conflict(current));
        }
        self.native.write(&encoded)?;
        match self.native.read() {
            Ok(Some(actual)) if actual == encoded => Ok(SaveOutcome::Saved(snapshot)),
            _ => Err(SettingsError::Readback),
        }
    }
}

/// Worker-owned reader retaining its last complete valid snapshot after a failed refresh.
#[derive(Default)]
pub struct Reader {
    last_good: Snapshot,
}

impl Reader {
    /// Returns the last complete valid snapshot, initially the shipping defaults.
    pub fn snapshot(&self) -> &Snapshot {
        &self.last_good
    }

    /// Refreshes outside callbacks. A missing value adopts complete shipping defaults.
    ///
    /// # Errors
    /// Returns the storage or parse error while retaining the previous snapshot unchanged.
    pub fn refresh(&mut self, store: &Store) -> Result<&Snapshot, SettingsError> {
        self.last_good = store.load()?;
        Ok(&self.last_good)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub(crate) struct Isolated(pub(crate) Store);

    impl Isolated {
        pub(crate) fn new() -> Self {
            let store = Store {
                native: native::testing::isolated(),
            };
            assert!(matches!(
                store.save(Revision::default(), Preferences::default()),
                SaveOutcome::Saved(_)
            ));
            Self(store)
        }

        pub(crate) fn invalidate(&self) {
            native::testing::wrong_type(&self.0.native);
        }
    }

    impl Drop for Isolated {
        fn drop(&mut self) {
            native::testing::cleanup(&self.0.native);
        }
    }
}
