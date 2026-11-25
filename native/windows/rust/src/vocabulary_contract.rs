//! Shared service/worker values. Worker publication transfers immutable snapshots.

use keyrill_ffi::{Dictionary, kr_abi_version, kr_dictionary_free, kr_dictionary_parse_utf8};
use std::ptr::NonNull;
use std::sync::Arc;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Error {
    #[default]
    None,
    Configuration,
    Unavailable,
    Missing,
    Unstable,
    Read,
    Parse,
    Model,
    Startup,
    Cancelled,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Status {
    #[default]
    Missing,
    Pending,
    Ready,
    Retained,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Path {
    pub(crate) drive: Vec<u16>,
    pub(crate) components: Vec<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Default(Option<Path>),
    Explicit(Path),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Model {
    pub(crate) sources: [Source; 2],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Config {
    #[default]
    Off,
    Persisted,
    Fixture,
    Model(Model),
}

impl Config {
    pub(crate) fn policy(&self) -> u32 {
        match self {
            Self::Off | Self::Persisted => 0,
            Self::Fixture => 1,
            Self::Model(_) => 2,
        }
    }
}

pub(crate) struct DictionaryOwner(NonNull<Dictionary>);

// SAFETY: the ABI explicitly permits concurrent immutable dictionary borrows. Arc keeps the
// allocation live until all readers and publication slots drain. Mutation is never exposed.
unsafe impl Send for DictionaryOwner {}
// SAFETY: all access is immutable and final free runs only after the last Arc is released.
unsafe impl Sync for DictionaryOwner {}

impl DictionaryOwner {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Arc<Self>, Error> {
        let length = u32::try_from(bytes.len()).map_err(|_| Error::Parse)?;
        let mut output = std::ptr::null_mut();
        // SAFETY: bytes are live for the complete call, and output is distinct writable storage.
        let status = unsafe {
            kr_dictionary_parse_utf8(kr_abi_version(), bytes.as_ptr(), length, &mut output)
        };
        if status != 0 {
            return Err(Error::Parse);
        }
        let pointer = NonNull::new(output).ok_or(Error::Parse)?;
        Ok(Arc::new(Self(pointer)))
    }

    pub(crate) fn pointer(&self) -> *const Dictionary {
        self.0.as_ptr()
    }
}

impl Drop for DictionaryOwner {
    fn drop(&mut self) {
        // SAFETY: the final Arc owns the original allocation and all immutable borrows drained.
        unsafe { kr_dictionary_free(self.0.as_ptr()) };
    }
}

#[derive(Clone, Default)]
pub(crate) struct SourceReady {
    pub(crate) status: Status,
    pub(crate) error: Error,
    pub(crate) dictionary: Option<Arc<DictionaryOwner>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub(crate) settings: crate::settings::Snapshot,
    pub(crate) application: crate::settings::AppPolicy,
}

impl Prepared {
    pub(crate) fn policy(&self) -> u32 {
        u32::from(
            self.settings.value.automatic
                && self.application == crate::settings::AppPolicy::Automatic,
        ) * 2
    }
}

#[derive(Clone, Default)]
pub(crate) struct Ready {
    pub(crate) activation: u32,
    pub(crate) revision: u64,
    pub(crate) model: bool,
    pub(crate) configuration: Option<Prepared>,
    pub(crate) configuration_error: Error,
    pub(crate) error: Error,
    pub(crate) sources: [SourceReady; 2],
}

impl Ready {
    pub(crate) fn capability(&self) -> u8 {
        match &self.configuration {
            Some(configuration) if configuration.application == crate::settings::AppPolicy::Off => {
                0
            }
            Some(configuration) if configuration.policy() == 0 => 1,
            _ => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModuleOwner {
    Dll,
    Process,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Boundary {
    BeforeWork,
    ParentOpened,
    BeforeRead,
    AfterRead,
    BeforeParse,
    BeforePublish,
    Published,
    Cleanup,
    AfterCount,
}
