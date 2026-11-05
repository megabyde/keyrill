//! Bounded vocabulary acquisition, invoked outside input callbacks.
#![forbid(unsafe_code)]

use keyrill_core::vocabulary::{MAX_SOURCE_BYTES, ParseError, Vocabulary};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{ErrorKind, Read};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::Arc;
#[cfg(not(unix))]
use std::time::SystemTime;

/// Content-free reason an observation failed. The last-good vocabulary remains available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceError {
    /// The source could not be opened, read or inspected.
    Io(ErrorKind),
    /// Only regular files, including symlinks resolving to regular files, are eligible.
    NotRegular,
    /// The source exceeds the byte bound.
    TooLarge,
    /// Identity, size or timestamps changed during acquisition.
    Changed,
    /// The complete candidate is outside the supported word-list format.
    Invalid(ParseError),
}

/// Result of one explicitly requested observation; no timer or background task is created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    /// A valid candidate awaits another equal observation.
    Pending,
    /// Two equal observations published a new immutable snapshot.
    Published,
    /// The already-published candidate is unchanged.
    Unchanged,
}

#[cfg(unix)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

#[cfg(unix)]
impl Stamp {
    fn of(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}

// Stable std exposes no file identity on Windows, so a replacement with the same length and
// timestamps keeps its stamp; the exact byte comparison between observations still catches it
#[cfg(not(unix))]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}

#[cfg(not(unix))]
impl Stamp {
    fn of(metadata: &Metadata) -> Self {
        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
        }
    }
}

struct Candidate {
    stamp: Stamp,
    bytes: Vec<u8>,
    vocabulary: Arc<Vocabulary>,
}

/// Parses a complete candidate; the source's word-list format.
pub type Parser = fn(&[u8]) -> Result<Vocabulary, ParseError>;

/// Last-good personal protection plus one staged candidate. Never reads a default/home path.
///
/// Two equal observations are a settling heuristic: a writer paused across both can still
/// expose an incomplete but valid file. This does not prove that an editor save completed.
pub struct Source {
    current: Option<Arc<Vocabulary>>,
    staged: Option<Candidate>,
    parse: Parser,
}

/// A Vim word list.
impl Default for Source {
    fn default() -> Self {
        Self::new(Vocabulary::parse)
    }
}

impl Source {
    /// An empty source whose candidates `parse` reads.
    pub fn new(parse: Parser) -> Self {
        Self {
            current: None,
            staged: None,
            parse,
        }
    }

    /// Retain an immutable last-good snapshot; `None` means never successfully published.
    pub fn current(&self) -> Option<Arc<Vocabulary>> {
        self.current.clone()
    }

    /// Read a bounded candidate without writing the source. Call on a serialized reload path,
    /// outside input callbacks. Failure discards staging without dropping last-good protection.
    /// An initial valid empty file also requires two equal observations.
    ///
    /// # Errors
    ///
    /// A [`SourceError`] if the file can't be read, isn't a regular file, exceeds the bound,
    /// changes during the read or doesn't parse. Staging is discarded; the published snapshot stays.
    pub fn observe(&mut self, path: &Path) -> Result<Observation, SourceError> {
        self.observe_at(path, |_, _| {})
    }

    fn observe_at(
        &mut self,
        path: &Path,
        checkpoint: impl FnMut(Step, Option<&File>),
    ) -> Result<Observation, SourceError> {
        let result = self.acquire(path, checkpoint);
        if result.is_err() {
            self.staged = None;
        }
        result
    }

    fn acquire(
        &mut self,
        path: &Path,
        mut checkpoint: impl FnMut(Step, Option<&File>),
    ) -> Result<Observation, SourceError> {
        let io = |error: std::io::Error| SourceError::Io(error.kind());
        let before = fs::metadata(path).map_err(io)?;
        if !before.is_file() {
            return Err(SourceError::NotRegular);
        }
        if before.len() > MAX_SOURCE_BYTES as u64 {
            return Err(SourceError::TooLarge);
        }
        let stamp = Stamp::of(&before);
        checkpoint(Step::CheckedPath, None);

        // Pre-stat narrows device exposure; nonblocking open also avoids waiting on a raced FIFO.
        // Windows needs no flag: a named-pipe client open fails at once when no instance is free.
        // Follow symlinks because normal dotfile arrangements use them. std sets close-on-exec.
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
        let mut file = options.open(path).map_err(io)?;
        let opened = file.metadata().map_err(io)?;
        if !opened.is_file() {
            return Err(SourceError::NotRegular);
        }
        if Stamp::of(&opened) != stamp {
            return Err(SourceError::Changed);
        }
        checkpoint(Step::Opened, Some(&file));
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_SOURCE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(SourceError::TooLarge);
        }
        checkpoint(Step::Read, None);
        let after = file.metadata().map_err(io)?;
        let resolved = fs::metadata(path).map_err(io)?;
        if !resolved.is_file()
            || Stamp::of(&after) != stamp
            || Stamp::of(&resolved) != stamp
            || bytes.len() as u64 != stamp.length
        {
            return Err(SourceError::Changed);
        }

        if let Some(staged) = &self.staged
            && staged.stamp == stamp
            && staged.bytes == bytes
        {
            if self
                .current
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &staged.vocabulary))
            {
                return Ok(Observation::Unchanged);
            }
            self.current = Some(staged.vocabulary.clone());
            return Ok(Observation::Published);
        }
        let vocabulary = (self.parse)(&bytes).map_err(SourceError::Invalid)?;
        self.staged = Some(Candidate {
            stamp,
            bytes,
            vocabulary: Arc::new(vocabulary),
        });
        Ok(Observation::Pending)
    }
}

// Deterministic barriers exercise replacement and in-place writes without timing assumptions
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    CheckedPath,
    Opened,
    Read,
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyrill_core::vocabulary::Membership;
    // Symlink, FIFO and permission tests are Unix-only; Windows symlinks need a privilege
    #[cfg(unix)]
    use std::os::unix::fs::{DirBuilderExt, symlink};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "keyrill-source-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            #[cfg(unix)]
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            #[cfg(not(unix))]
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn published(source: &mut Source, path: &Path) -> Arc<Vocabulary> {
        assert_eq!(source.observe(path), Ok(Observation::Pending));
        assert_eq!(source.observe(path), Ok(Observation::Published));
        source.current().unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn symlink_reload_settles_and_retains_immutable_readers() {
        let directory = Directory::new();
        let target = directory.path("target");
        let path = directory.path("source");
        fs::write(&target, b"hello\n").unwrap();
        symlink(&target, &path).unwrap();
        let mut source = Source::default();
        assert!(source.current().is_none());
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
        assert!(source.current().is_none());
        assert_eq!(source.observe(&path), Ok(Observation::Published));
        let old = source.current().unwrap();
        assert_eq!(source.observe(&path), Ok(Observation::Unchanged));
        assert!(Arc::ptr_eq(&old, &source.current().unwrap()));
        fs::write(&target, b"world\n").unwrap();
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
        assert!(Arc::ptr_eq(&old, &source.current().unwrap()));
        assert_eq!(source.observe(&path), Ok(Observation::Published));
        assert_eq!(
            old.lookup(&[104, 101, 108, 108, 111]),
            Membership::Recognized
        );
        assert_eq!(
            source.current().unwrap().lookup(&[119, 111, 114, 108, 100]),
            Membership::Recognized
        );
        assert_eq!(fs::read(&target).unwrap(), b"world\n");
    }

    #[test]
    fn failures_drop_staging_and_preserve_last_good() {
        let directory = Directory::new();
        let path = directory.path("source");
        fs::write(&path, b"hello").unwrap();
        let mut source = Source::default();
        let good = published(&mut source, &path);
        for bytes in [b"a\0b".as_slice(), b"/encoding=latin1\nhello"] {
            fs::write(&path, bytes).unwrap();
            assert!(matches!(
                source.observe(&path),
                Err(SourceError::Invalid(_))
            ));
            assert!(source.staged.is_none());
            assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        }
        fs::write(&path, vec![b'a'; MAX_SOURCE_BYTES + 1]).unwrap();
        assert_eq!(source.observe(&path), Err(SourceError::TooLarge));
        fs::remove_file(&path).unwrap();
        assert_eq!(
            source.observe(&path),
            Err(SourceError::Io(ErrorKind::NotFound))
        );
        fs::create_dir(&path).unwrap();
        assert_eq!(source.observe(&path), Err(SourceError::NotRegular));
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        fs::remove_dir(&path).unwrap();
        fs::write(&path, b"").unwrap();
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        fs::remove_file(&path).unwrap();
        assert!(source.observe(&path).is_err());
        fs::write(&path, b"").unwrap();
        assert!(published(&mut source, &path).is_empty());
        let mut initial = Source::default();
        assert_eq!(initial.observe(&path), Ok(Observation::Pending));
        assert!(initial.current().is_none());
        assert_eq!(initial.observe(&path), Ok(Observation::Published));
        assert!(initial.current().unwrap().is_empty());
    }

    #[test]
    fn deterministic_mutations_during_acquisition_never_publish() {
        for step in [Step::CheckedPath, Step::Opened, Step::Read] {
            let directory = Directory::new();
            let path = directory.path("source");
            fs::write(&path, b"hello").unwrap();
            let mut source = Source::default();
            let good = published(&mut source, &path);
            assert_eq!(
                source.observe_at(&path, |at, _| {
                    if at == step {
                        fs::write(&path, b"different length").unwrap();
                    }
                }),
                Err(SourceError::Changed)
            );
            assert!(source.staged.is_none());
            assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        }
    }

    #[cfg(unix)]
    #[test]
    fn replacement_and_truncate_then_write_require_new_observations() {
        let directory = Directory::new();
        let target = directory.path("target");
        let path = directory.path("source");
        let replacement = directory.path("replacement");
        fs::write(&target, b"hello").unwrap();
        symlink(&target, &path).unwrap();
        let mut source = Source::default();
        let good = published(&mut source, &path);
        fs::write(&replacement, b"world").unwrap();
        assert_eq!(
            source.observe_at(&path, |at, _| {
                if at == Step::Read {
                    fs::rename(&replacement, &target).unwrap();
                }
            }),
            Err(SourceError::Changed)
        );
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        let good = published(&mut source, &path);
        assert_eq!(
            good.lookup(&[119, 111, 114, 108, 100]),
            Membership::Recognized
        );
        fs::write(&target, b"").unwrap();
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        fs::write(&target, b"complete\n").unwrap();
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        assert_eq!(source.observe(&path), Ok(Observation::Published));
        assert_eq!(source.current().unwrap().len(), 1);
        assert_eq!(fs::read(&target).unwrap(), b"complete\n");
    }

    #[test]
    fn growth_and_nonregular_descriptor_races_are_rejected() {
        let directory = Directory::new();
        let path = directory.path("source");
        fs::write(&path, b"hello").unwrap();
        let mut source = Source::default();
        let good = published(&mut source, &path);
        assert_eq!(
            source.observe_at(&path, |at, _| {
                if at == Step::Opened {
                    fs::write(&path, vec![b'a'; MAX_SOURCE_BYTES + 1]).unwrap();
                }
            }),
            Err(SourceError::TooLarge)
        );
        fs::write(&path, b"hello").unwrap();
        // Windows refuses to open a directory as a file, so the race fails at the open instead
        let expected = if cfg!(windows) {
            SourceError::Io(ErrorKind::PermissionDenied)
        } else {
            SourceError::NotRegular
        };
        assert_eq!(
            source.observe_at(&path, |at, _| {
                if at == Step::CheckedPath {
                    fs::remove_file(&path).unwrap();
                    fs::create_dir(&path).unwrap();
                }
            }),
            Err(expected)
        );
        assert!(source.staged.is_none());
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_retarget_and_disappearance_discard_staging() {
        let directory = Directory::new();
        let first = directory.path("first");
        let second = directory.path("second");
        let path = directory.path("source");
        fs::write(&first, b"hello").unwrap();
        fs::write(&second, b"world").unwrap();
        symlink(&first, &path).unwrap();
        let mut source = Source::default();
        let good = published(&mut source, &path);
        assert_eq!(
            source.observe_at(&path, |at, _| {
                if at == Step::Read {
                    fs::remove_file(&path).unwrap();
                    symlink(&second, &path).unwrap();
                }
            }),
            Err(SourceError::Changed)
        );
        assert!(source.staged.is_none());
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
        assert_eq!(
            source.observe_at(&path, |at, _| {
                if at == Step::Read {
                    fs::remove_file(&path).unwrap();
                }
            }),
            Err(SourceError::Io(ErrorKind::NotFound))
        );
        assert!(source.staged.is_none());
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        symlink(&second, &path).unwrap();
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
    }

    #[cfg(unix)]
    #[test]
    fn nonblocking_flag_is_checked_before_the_fifo_race() {
        let directory = Directory::new();
        let path = directory.path("source");
        fs::write(&path, b"hello").unwrap();
        let mut source = Source::default();
        let good = published(&mut source, &path);
        let mut checked = false;
        assert_eq!(
            source.observe_at(&path, |at, file| {
                if at == Step::Opened {
                    let flags = crate::descriptor_flags(file.unwrap());
                    assert!(flags >= 0);
                    assert_ne!(flags & libc::O_NONBLOCK, 0);
                    checked = true;
                }
            }),
            Ok(Observation::Unchanged)
        );
        assert!(checked);
        assert_eq!(
            source.observe_at(&path, |at, _| {
                if at == Step::CheckedPath {
                    fs::remove_file(&path).unwrap();
                    assert!(
                        std::process::Command::new("/usr/bin/mkfifo")
                            .arg(&path)
                            .status()
                            .unwrap()
                            .success()
                    );
                }
            }),
            Err(SourceError::NotRegular)
        );
        assert!(source.staged.is_none());
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
    }

    #[test]
    fn paused_writer_across_both_observations_can_publish_empty() {
        let directory = Directory::new();
        let path = directory.path("source");
        fs::write(&path, b"hello").unwrap();
        let mut source = Source::default();
        let good = published(&mut source, &path);
        fs::write(&path, b"").unwrap();
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        assert_eq!(source.observe(&path), Ok(Observation::Published));
        assert!(source.current().unwrap().is_empty());
        assert!(!good.is_empty());
    }

    #[test]
    fn equal_metadata_does_not_replace_exact_byte_comparison() {
        let directory = Directory::new();
        let path = directory.path("source");
        fs::write(&path, b"hello").unwrap();
        let mut source = Source::default();
        let good = published(&mut source, &path);
        fs::write(&path, b"world").unwrap();
        // Model a filesystem whose timestamp granularity hides a same-length rewrite
        source.staged.as_mut().unwrap().stamp = Stamp::of(&fs::metadata(&path).unwrap());
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        assert_eq!(source.observe(&path), Ok(Observation::Published));
        assert_eq!(
            source.current().unwrap().lookup(&[119, 111, 114, 108, 100]),
            Membership::Recognized
        );
    }

    #[cfg(unix)]
    #[test]
    fn permission_failure_preserves_last_good() {
        use std::os::unix::fs::PermissionsExt;

        let directory = Directory::new();
        let path = directory.path("source");
        fs::write(&path, b"hello").unwrap();
        let mut source = Source::default();
        let good = published(&mut source, &path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(
            source.observe(&path),
            Err(SourceError::Io(ErrorKind::PermissionDenied))
        );
        assert!(source.staged.is_none());
        assert!(Arc::ptr_eq(&good, &source.current().unwrap()));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(source.observe(&path), Ok(Observation::Pending));
    }
}
