//! Host dictionary sources and immutable adoption, without scheduling or native editing authority.

use crate::vocabulary_source::{Observation, Source, SourceError};
use crate::{
    BAD_TAG, BOUNDS, DICTIONARY_DATA, Engine, NULL, OK, POISONED, STATE, VERSION, compatible_abi,
    with_engine,
};
use keyrill_core::vocabulary::{Snapshot, Vocabulary};
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MAX_PATH: usize = 1024;
const SOURCE_IO: u32 = 10;
const SOURCE_CHANGED: u32 = 11;
const SOURCE_NOT_REGULAR: u32 = 12;
const PATH: u32 = 13;
const STALE_CONFIGURATION: u32 = 14;
const MISSING_IS_EMPTY: u32 = 1;
const DEFAULT_NAMES: [&str; 2] = ["en.utf-8.add", "ru.utf-8.add"];
// Vim's first 'runtimepath' entry, where `zg` creates spell files while 'spellfile' is empty
#[cfg(not(windows))]
const DEFAULT_DIRECTORY: &str = ".vim";
#[cfg(windows)]
const DEFAULT_DIRECTORY: &str = "vimfiles";

#[derive(Default)]
struct Language {
    generation: u64,
    path: Option<PathBuf>,
    missing_is_empty: bool,
    current: Option<Arc<Vocabulary>>,
}

#[derive(Default)]
struct Acquisition {
    generation: [u64; 2],
    sources: [Source; 2],
}

/// Opaque shared host handle. Free only after every operation has completed.
#[derive(Default)]
pub struct VocabularyHandle {
    poisoned: AtomicBool,
    acquisition: Mutex<Acquisition>,
    published: Mutex<[Language; 2]>,
}

fn language_index(language: u32) -> Result<usize, u32> {
    match language {
        1 => Ok(0),
        2 => Ok(1),
        _ => Err(BAD_TAG),
    }
}

// Unix paths are raw bytes; Windows paths cross the ABI as UTF-8
fn host_path(bytes: &[u8]) -> Option<&Path> {
    if bytes.contains(&0) {
        return None;
    }
    #[cfg(unix)]
    let path = Path::new(std::ffi::OsStr::from_bytes(bytes));
    #[cfg(not(unix))]
    let path = Path::new(std::str::from_utf8(bytes).ok()?);
    path.is_absolute().then_some(path)
}

fn source_status(error: SourceError) -> u32 {
    match error {
        SourceError::Io(_) => SOURCE_IO,
        SourceError::NotRegular => SOURCE_NOT_REGULAR,
        SourceError::TooLarge => BOUNDS,
        SourceError::Changed => SOURCE_CHANGED,
        SourceError::Invalid(_) => DICTIONARY_DATA,
    }
}

impl VocabularyHandle {
    fn configure(&self, index: usize, path: Option<PathBuf>, missing_is_empty: bool) -> u32 {
        let Ok(mut languages) = self.published.lock() else {
            return POISONED;
        };
        let language = &mut languages[index];
        if language.path == path && language.missing_is_empty == missing_is_empty {
            return OK;
        }
        let Some(generation) = language.generation.checked_add(1) else {
            return STATE;
        };
        let retired = std::mem::replace(
            language,
            Language {
                generation,
                path,
                missing_is_empty,
                current: None,
            },
        );
        drop(languages);
        drop(retired);
        OK
    }

    fn observe(&self, index: usize, checkpoint: impl FnOnce()) -> Result<Observation, u32> {
        let (generation, path, missing_is_empty) = {
            let languages = self.published.lock().map_err(|_| POISONED)?;
            let language = &languages[index];
            let path = language.path.clone().ok_or(STATE)?;
            (language.generation, path, language.missing_is_empty)
        };
        let mut acquisition = self.acquisition.lock().map_err(|_| POISONED)?;
        if acquisition.generation[index] != generation {
            acquisition.sources[index] = Source::default();
            acquisition.generation[index] = generation;
        }
        checkpoint();
        let result = acquisition.sources[index].observe(&path);
        let mut languages = self.published.lock().map_err(|_| POISONED)?;
        if languages[index].generation != generation {
            return Err(STALE_CONFIGURATION);
        }
        // An absent file publishes empty protection only until a real file has been published:
        // editors that save by rename can leave the path briefly absent, which must keep last-good
        if missing_is_empty
            && matches!(result, Err(SourceError::Io(ErrorKind::NotFound)))
            && acquisition.sources[index].current().is_none()
        {
            if languages[index].current.is_some() {
                return Ok(Observation::Unchanged);
            }
            languages[index].current = Some(Arc::new(Vocabulary::default()));
            return Ok(Observation::Published);
        }
        let observation = result.map_err(source_status)?;
        let retired = if observation == Observation::Published {
            std::mem::replace(
                &mut languages[index].current,
                acquisition.sources[index].current(),
            )
        } else {
            None
        };
        drop(languages);
        drop(acquisition);
        drop(retired);
        Ok(observation)
    }
}

// Shared references are synchronized internally; no native calls occur under either lock
unsafe fn with_vocabulary(
    pointer: *const VocabularyHandle,
    operation: impl FnOnce(&VocabularyHandle) -> u32,
) -> u32 {
    if pointer.is_null() {
        return NULL;
    }
    // SAFETY: the caller keeps this aligned allocation alive throughout concurrent operations
    let vocabulary = unsafe { &*pointer };
    if vocabulary.poisoned.load(Ordering::Acquire) {
        return POISONED;
    }
    let status = catch_unwind(AssertUnwindSafe(|| operation(vocabulary))).unwrap_or(POISONED);
    if status == POISONED {
        vocabulary.poisoned.store(true, Ordering::Release);
    }
    status
}

/// Allocate an isolated source handle with no configured paths or file reads.
///
/// # Safety
/// Non-null `out` must be aligned and writable for one distinct handle pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_vocabulary_new(abi: u32, out: *mut *mut VocabularyHandle) -> u32 {
    if out.is_null() {
        return NULL;
    }
    // SAFETY: the caller provides a distinct writable pointer-sized object
    unsafe { out.write(std::ptr::null_mut()) };
    if !compatible_abi(abi) {
        return VERSION;
    }
    match catch_unwind(|| Box::new(VocabularyHandle::default())) {
        Ok(vocabulary) => {
            // SAFETY: same output contract; transfer the allocation exactly once
            unsafe { out.write(Box::into_raw(vocabulary)) };
            OK
        }
        Err(_) => POISONED,
    }
}

/// Free after all source and adoption operations have completed. Null is a no-op.
///
/// # Safety
/// Non-null `pointer` must be the original live allocation, transferred exactly once, with no
/// concurrent operation or remaining Rust borrow. Adopted engines own independent Arc references.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_vocabulary_free(pointer: *mut VocabularyHandle) {
    if !pointer.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: the caller transfers the original allocation with every operation drained
            drop(unsafe { Box::from_raw(pointer) });
        }));
    }
}

/// Configure one logical source without I/O. Changed paths or flags immediately become missing.
///
/// With `MISSING_IS_EMPTY` in `flags`, observing a path that does not exist (ENOENT) publishes an
/// empty source until a file at that path has been published; unknown flag bits are `BAD_TAG`.
///
/// # Safety
/// `pointer` must be null or a live aligned source allocation throughout this call. For nonzero
/// `len <= MAX_PATH`, a non-null `path` must address readable bytes not aliasing the handle.
/// Zero/oversized/null arguments are checked without reading the path. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_vocabulary_configure(
    pointer: *mut VocabularyHandle,
    language: u32,
    path: *const u8,
    len: u32,
    flags: u32,
) -> u32 {
    // SAFETY: forward the live shared handle and bounded readable path contracts
    unsafe {
        with_vocabulary(pointer, |vocabulary| {
            let index = match language_index(language) {
                Ok(index) => index,
                Err(status) => return status,
            };
            if flags & !MISSING_IS_EMPTY != 0 {
                return BAD_TAG;
            }
            if len as usize > MAX_PATH {
                return BOUNDS;
            }
            let path = if len == 0 {
                None
            } else {
                if path.is_null() {
                    return NULL;
                }
                // SAFETY: bounded nonzero length; caller supplies distinct readable bytes
                let path = std::slice::from_raw_parts(path, len as usize);
                let Some(path) = host_path(path) else {
                    return PATH;
                };
                Some(path.to_path_buf())
            };
            let missing_is_empty = path.is_some() && flags & MISSING_IS_EMPTY != 0;
            vocabulary.configure(index, path, missing_is_empty)
        })
    }
}

/// Configure one language's Vim spell file under the absolute `home` directory, without I/O:
/// `.vim/spell` (`vimfiles\spell` on Windows) and `en.utf-8.add` or `ru.utf-8.add`. A missing
/// file there behaves as with `MISSING_IS_EMPTY`; a joined path over `MAX_PATH` is `BOUNDS`.
///
/// # Safety
/// `pointer` must be null or a live aligned source allocation throughout this call. For nonzero
/// `len <= MAX_PATH`, a non-null `home` must address readable bytes not aliasing the handle.
/// Zero/oversized/null arguments are checked without reading the path. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_vocabulary_configure_default(
    pointer: *mut VocabularyHandle,
    language: u32,
    home: *const u8,
    len: u32,
) -> u32 {
    // SAFETY: forward the live shared handle and bounded readable path contracts
    unsafe {
        with_vocabulary(pointer, |vocabulary| {
            let index = match language_index(language) {
                Ok(index) => index,
                Err(status) => return status,
            };
            if len as usize > MAX_PATH {
                return BOUNDS;
            }
            if len == 0 {
                return PATH;
            }
            if home.is_null() {
                return NULL;
            }
            // SAFETY: bounded nonzero length; caller supplies distinct readable bytes
            let home = std::slice::from_raw_parts(home, len as usize);
            let Some(home) = host_path(home) else {
                return PATH;
            };
            let path = home
                .join(DEFAULT_DIRECTORY)
                .join("spell")
                .join(DEFAULT_NAMES[index]);
            // The home is UTF-8 on Windows, so this counts the bytes an adapter passes on every OS
            if path.as_os_str().len() > MAX_PATH {
                return BOUNDS;
            }
            vocabulary.configure(index, Some(path), true)
        })
    }
}

/// Observe on a serialized acquisition queue, outside UI/native input callbacks.
///
/// # Safety
/// `pointer` must be null or a live aligned source allocation for the whole call. Non-null
/// `observation_out` must be writable/aligned for u32 and not alias any handle or live input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_vocabulary_observe(
    pointer: *mut VocabularyHandle,
    language: u32,
    observation_out: *mut u32,
) -> u32 {
    // SAFETY: the caller keeps the handle and distinct output alive until this call completes
    unsafe {
        with_vocabulary(pointer, |vocabulary| {
            let index = match language_index(language) {
                Ok(index) => index,
                Err(status) => return status,
            };
            if observation_out.is_null() {
                return NULL;
            }
            match vocabulary.observe(index, || {}) {
                Ok(observation) => {
                    observation_out.write(match observation {
                        Observation::Pending => 0,
                        Observation::Published => 1,
                        Observation::Unchanged => 2,
                    });
                    OK
                }
                Err(status) => status,
            }
        })
    }
}

/// Adopt both immutable language views together, without changing session state or authority.
///
/// # Safety
/// `engine` must be null or a live uniquely borrowed engine on its serialized thread. `pointer`
/// must be null or a distinct live source allocation and may be concurrently observed/configured.
/// Both allocations must remain alive until the call returns. No native callback occurs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_adopt_vocabulary(
    engine: *mut Engine,
    pointer: *const VocabularyHandle,
) -> u32 {
    // SAFETY: both allocation contracts are forwarded; locks/borrows end before returning
    unsafe {
        with_engine(engine, |engine| {
            if pointer.is_null() {
                return NULL;
            }
            if engine.session.has_pending() {
                return STATE;
            }
            let mut current = [None, None];
            let status = with_vocabulary(pointer, |vocabulary| {
                let Ok(languages) = vocabulary.published.lock() else {
                    return POISONED;
                };
                current = [languages[0].current.clone(), languages[1].current.clone()];
                OK
            });
            if status != OK {
                return status;
            }
            engine.dictionaries = current.map(Snapshot::from);
            OK
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{apply_event, type_text};
    use crate::{
        ABI, NativeEvent, NativePlan, kr_commit, kr_engine_free, kr_engine_new, kr_plan_event,
        kr_reset, kr_set_automatic_policy, kr_set_dictionary_utf8,
    };
    use keyrill_core::vocabulary::Membership;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::DirBuilderExt;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "keyrill-host-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            #[cfg(unix)]
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            #[cfg(not(unix))]
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    const ROOT: &[u8] = b"/";
    #[cfg(not(unix))]
    const ROOT: &[u8] = b"C:\\";

    #[cfg(unix)]
    fn bytes(path: &Path) -> &[u8] {
        path.as_os_str().as_bytes()
    }

    #[cfg(not(unix))]
    fn bytes(path: &Path) -> &[u8] {
        path.to_str().unwrap().as_bytes()
    }

    unsafe fn handles() -> (*mut Engine, *mut VocabularyHandle) {
        // SAFETY: both outputs are distinct live pointer-sized local objects
        unsafe {
            let mut engine = std::ptr::null_mut();
            let mut vocabulary = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_vocabulary_new(ABI, &mut vocabulary), OK);
            (engine, vocabulary)
        }
    }

    unsafe fn configure(vocabulary: *mut VocabularyHandle, language: u32, path: &Path) {
        let bytes = bytes(path);
        // SAFETY: the test retains the handle and readable distinct borrowed path for the call
        unsafe {
            assert_eq!(
                kr_vocabulary_configure(
                    vocabulary,
                    language,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    0
                ),
                OK
            );
        }
    }

    unsafe fn observe(vocabulary: *mut VocabularyHandle, language: u32, expected: u32) {
        let mut observation = u32::MAX;
        // SAFETY: the handle remains live and the distinct local output is writable
        unsafe {
            assert_eq!(
                kr_vocabulary_observe(vocabulary, language, &mut observation),
                OK
            );
        }
        assert_eq!(observation, expected);
    }

    fn mark(plan: &NativePlan) -> Vec<u16> {
        plan.mark[..plan.mark_len as usize].to_vec()
    }

    #[test]
    fn path_buffers_versions_generation_exhaustion_and_poison_are_transactional() {
        // SAFETY: all accepted pointers refer to distinct live local objects/allocations; every
        // handle is serialized here and freed exactly once after all borrows have ended
        unsafe {
            let (engine, vocabulary) = handles();
            let mut output = 77;
            assert_eq!(kr_vocabulary_new(ABI, std::ptr::null_mut()), NULL);
            assert_eq!(
                kr_vocabulary_observe(std::ptr::null_mut(), 1, &mut output),
                NULL
            );
            assert_eq!(
                kr_vocabulary_configure(std::ptr::null_mut(), 1, std::ptr::null(), 0, 0),
                NULL
            );
            assert_eq!(kr_adopt_vocabulary(std::ptr::null_mut(), vocabulary), NULL);
            assert_eq!(kr_adopt_vocabulary(engine, std::ptr::null()), NULL);
            assert_eq!(kr_vocabulary_observe(vocabulary, 0, &mut output), BAD_TAG);
            assert_eq!(
                kr_vocabulary_observe(vocabulary, 1, std::ptr::null_mut()),
                NULL
            );
            assert_eq!(kr_vocabulary_observe(vocabulary, 1, &mut output), STATE);
            assert_eq!(output, 77);
            // Each OS rejects the other's absolute form: a rooted path without a drive or UNC
            // prefix is relative on Windows, and a drive letter is a relative name on Unix
            #[cfg(unix)]
            let foreign = b"C:\\a".as_slice();
            #[cfg(not(unix))]
            let foreign = b"/a".as_slice();
            let nul = [ROOT, b"a\0b"].concat();
            for (path, language, len, expected) in [
                (b"relative".as_ptr(), 1, 8, PATH),
                (foreign.as_ptr(), 1, foreign.len() as u32, PATH),
                (nul.as_ptr(), 1, nul.len() as u32, PATH),
                (std::ptr::null(), 1, 1, NULL),
                (std::ptr::null(), 1, MAX_PATH as u32 + 1, BOUNDS),
                (std::ptr::null(), 3, 0, BAD_TAG),
            ] {
                assert_eq!(
                    kr_vocabulary_configure(vocabulary, language, path, len, 0),
                    expected
                );
            }
            let mut path = ROOT.to_vec();
            path.resize(MAX_PATH, b'x');
            path[ROOT.len()] = 0xff;
            // Unix passes raw bytes through; Windows requires UTF-8
            let status = if cfg!(unix) { OK } else { PATH };
            assert_eq!(
                kr_vocabulary_configure(vocabulary, 1, path.as_ptr(), path.len() as u32, 0),
                status
            );
            path[ROOT.len()] = b'y';
            assert_eq!(
                kr_vocabulary_configure(vocabulary, 1, path.as_ptr(), path.len() as u32, 0),
                OK
            );
            let stored = path.clone();
            path.fill(b'z');
            let configured = (*vocabulary).published.lock().unwrap()[0].path.clone();
            assert_eq!(bytes(configured.as_ref().unwrap()), stored);
            (*vocabulary).published.lock().unwrap()[0].generation = u64::MAX;
            let different = [ROOT, b"different"].concat();
            assert_eq!(
                kr_vocabulary_configure(
                    vocabulary,
                    1,
                    different.as_ptr(),
                    different.len() as u32,
                    0
                ),
                STATE
            );
            assert_eq!((*vocabulary).published.lock().unwrap()[0].path, configured);
            assert_eq!(
                kr_vocabulary_configure(vocabulary, 2, path.as_ptr(), 0, 0),
                OK
            );
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            let held = keyrill_core::vocabulary::Vocabulary::parse(b"hello").unwrap();
            (*engine).dictionaries[0] = Snapshot::from(Some(Arc::new(held)));
            let old = (*engine).dictionaries[0].current().unwrap();
            assert_eq!(
                with_vocabulary(vocabulary, |vocabulary| {
                    let _guard = vocabulary.published.lock().unwrap();
                    panic!("synthetic source containment test")
                }),
                POISONED
            );
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), POISONED);
            assert!(!(*engine).poisoned);
            assert!(Arc::ptr_eq(
                &old,
                &(*engine).dictionaries[0].current().unwrap()
            ));
            assert_eq!(kr_reset(engine), OK);
            (*vocabulary).poisoned.store(false, Ordering::Release);
            assert_eq!(
                kr_vocabulary_configure(vocabulary, 1, std::ptr::null(), 0, 0),
                POISONED
            );
            assert!((*vocabulary).poisoned.load(Ordering::Acquire));
            assert_eq!(kr_vocabulary_observe(vocabulary, 1, &mut output), POISONED);
            assert_eq!(output, 77);
            (*engine).poisoned = true;
            assert_eq!(kr_adopt_vocabulary(engine, std::ptr::null()), POISONED);
            kr_vocabulary_free(vocabulary);
            kr_vocabulary_free(std::ptr::null_mut());
            kr_engine_free(engine);

            let mut rejected = std::ptr::dangling_mut();
            assert_eq!(kr_vocabulary_new(ABI + 1, &mut rejected), VERSION);
            assert!(rejected.is_null());
            assert_eq!(kr_vocabulary_new(0x0001_0000, &mut rejected), VERSION);
            assert!(rejected.is_null());
            assert_eq!(kr_vocabulary_new(ABI - 1, &mut rejected), VERSION);
            assert!(rejected.is_null());
        }
    }

    #[test]
    fn source_configuration_identity_missing_empty_and_last_good_are_distinct() {
        let directory = Directory::new();
        let en = directory.file("en.add", b"ghbdtn");
        let next_en = directory.file("next-en.add", b"hello");
        let ru = directory.file("ru.add", b"");
        // SAFETY: source paths/outputs are distinct live objects, handles never overlap free,
        // and no Rust reference to the engine lives across an exported mutable call
        unsafe {
            let (engine, vocabulary) = handles();
            assert_eq!(kr_set_automatic_policy(engine, 2), OK);
            configure(vocabulary, 1, &en);
            configure(vocabulary, 2, &ru);
            for language in [1, 2] {
                observe(vocabulary, language, 0);
                observe(vocabulary, language, 1);
                observe(vocabulary, language, 2);
            }
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            let old = (*engine).dictionaries[0].current().unwrap();
            assert!((*engine).dictionaries[1].current().unwrap().is_empty());
            let mut isolated = std::ptr::null_mut();
            assert_eq!(kr_vocabulary_new(ABI, &mut isolated), OK);
            assert_eq!(kr_adopt_vocabulary(engine, isolated), OK);
            assert!((*engine).dictionaries[0].current().is_none());
            assert!((*engine).dictionaries[1].current().is_none());
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            kr_vocabulary_free(isolated);
            assert_eq!(
                mark(&type_text(engine, 1, "ghbdtn ")),
                "ghbdtn ".encode_utf16().collect::<Vec<_>>()
            );
            configure(vocabulary, 1, &en);
            observe(vocabulary, 1, 2);
            fs::write(&en, b"bad\0entry").unwrap();
            let mut output = 77;
            assert_eq!(
                kr_vocabulary_observe(vocabulary, 1, &mut output),
                DICTIONARY_DATA
            );
            assert_eq!(output, 77);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert!(Arc::ptr_eq(
                &old,
                &(*engine).dictionaries[0].current().unwrap()
            ));
            fs::remove_file(&en).unwrap();
            assert_eq!(kr_vocabulary_observe(vocabulary, 1, &mut output), SOURCE_IO);
            assert_eq!(output, 77);
            configure(vocabulary, 1, &next_en);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert!((*engine).dictionaries[0].current().is_none());
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(
                mark(&type_text(engine, 1, "ghbdtn ")),
                "ghbdtn ".encode_utf16().collect::<Vec<_>>()
            );
            observe(vocabulary, 1, 0);
            observe(vocabulary, 1, 1);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(
                mark(&type_text(engine, 1, "ghbdtn ")),
                "привет ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(
                kr_vocabulary_configure(vocabulary, 2, std::ptr::null(), 0, 0),
                OK
            );
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert!((*engine).dictionaries[1].current().is_none());
            configure(vocabulary, 2, &directory.0);
            assert_eq!(
                kr_vocabulary_observe(vocabulary, 2, &mut output),
                SOURCE_NOT_REGULAR
            );
            let large = directory.file(
                "large.add",
                &vec![b'x'; keyrill_core::vocabulary::MAX_SOURCE_BYTES + 1],
            );
            configure(vocabulary, 2, &large);
            assert_eq!(kr_vocabulary_observe(vocabulary, 2, &mut output), BOUNDS);
            assert_eq!(output, 77);
            assert_eq!(kr_reset(engine), OK);
            let event = NativeEvent {
                role: 1,
                mode: 1,
                context: 0,
                len: 1,
                units: [97; 8],
            };
            let mut out = std::mem::MaybeUninit::uninit();
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            assert_eq!(kr_commit(engine, out.assume_init_ref().plan_id, 2), OK);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), STATE);
            kr_vocabulary_free(vocabulary);
            kr_engine_free(engine);
        }
    }

    #[test]
    fn source_adoption_preserves_pending_history_receipts_and_independent_engines() {
        let directory = Directory::new();
        let en = directory.file("en.add", b"");
        let ru = directory.file("ru.add", b"");
        let protected = directory.file("protected.add", b"ghbdtn");
        // SAFETY: handle lifetimes are retained across all calls; local events/plans/path buffers
        // are distinct, and engines are each uniquely accessed on this serialized test thread
        unsafe {
            let (engine, vocabulary) = handles();
            let mut second = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI, &mut second), OK);
            configure(vocabulary, 1, &en);
            configure(vocabulary, 2, &ru);
            for language in [1, 2] {
                observe(vocabulary, language, 0);
                observe(vocabulary, language, 1);
            }
            for handle in [engine, second] {
                assert_eq!(kr_set_automatic_policy(handle, 2), OK);
                assert_eq!(kr_adopt_vocabulary(handle, vocabulary), OK);
            }
            let original = (*engine).dictionaries[0].current().unwrap();
            assert!(Arc::ptr_eq(
                &original,
                &(*second).dictionaries[0].current().unwrap()
            ));
            type_text(engine, 1, "hello world ");
            let event = NativeEvent {
                role: 1,
                mode: 1,
                context: 0,
                len: 1,
                units: [121; 8],
            };
            let mut out = std::mem::MaybeUninit::uninit();
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            let plan: NativePlan = out.assume_init();
            assert_eq!((plan.action, plan.disposition), (2, 2));
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), STATE);
            assert!(Arc::ptr_eq(
                &original,
                &(*engine).dictionaries[0].current().unwrap()
            ));
            assert_eq!(kr_commit(engine, plan.plan_id, 0), OK);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert_eq!(
                mark(&type_text(engine, 1, "yfv ")),
                "yfv ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(
                mark(&type_text(second, 1, "yfv ")),
                "нам ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(
                mark(&type_text(engine, 1, "ghbdtn ")),
                "привет ".encode_utf16().collect::<Vec<_>>()
            );
            configure(vocabulary, 1, &protected);
            observe(vocabulary, 1, 0);
            observe(vocabulary, 1, 1);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            let reverse = apply_event(
                engine,
                &NativeEvent {
                    role: 5,
                    mode: 1,
                    context: 0,
                    len: 0,
                    units: [0; 8],
                },
            );
            assert_eq!(mark(&reverse), "ghbdtn ".encode_utf16().collect::<Vec<_>>());
            assert!(Arc::ptr_eq(
                &original,
                &(*second).dictionaries[0].current().unwrap()
            ));
            assert_eq!(kr_adopt_vocabulary(second, vocabulary), OK);
            assert!(Arc::ptr_eq(
                &(*engine).dictionaries[0].current().unwrap(),
                &(*second).dictionaries[0].current().unwrap()
            ));
            assert_eq!(kr_reset(second), OK);
            assert_eq!(kr_set_dictionary_utf8(second, 1, std::ptr::null(), 0), OK);
            assert_eq!(
                mark(&type_text(second, 1, "ghbdtn ")),
                "привет ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(kr_adopt_vocabulary(second, vocabulary), OK);
            kr_vocabulary_free(vocabulary);
            for handle in [engine, second] {
                assert_eq!(kr_reset(handle), OK);
                assert_eq!(
                    mark(&type_text(handle, 1, "ghbdtn ")),
                    "ghbdtn ".encode_utf16().collect::<Vec<_>>()
                );
                let units: Vec<_> = "ghbdtn".encode_utf16().collect();
                assert_eq!(
                    (*handle).dictionaries[0].current().unwrap().lookup(&units),
                    Membership::Recognized
                );
                kr_engine_free(handle);
            }
        }
    }

    #[test]
    fn source_observation_cannot_publish_across_configuration_or_block_adoption_on_io() {
        let directory = Directory::new();
        let old = directory.file("old.add", b"old");
        let new = directory.file("new.add", b"new");
        // SAFETY: scoped threads share only synchronized immutable handle references. The engine
        // stays uniquely on this thread, and free occurs after thread join and every borrow ends.
        unsafe {
            let (engine, vocabulary) = handles();
            configure(vocabulary, 1, &old);
            observe(vocabulary, 1, 0);
            let shared = &*vocabulary;
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            std::thread::scope(|scope| {
                let worker = scope.spawn(move || {
                    shared.observe(0, || {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    })
                });
                entered_rx.recv().unwrap();
                configure(vocabulary, 1, &new);
                assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
                assert!((*engine).dictionaries[0].current().is_none());
                release_tx.send(()).unwrap();
                assert_eq!(worker.join().unwrap(), Err(STALE_CONFIGURATION));
            });
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert!((*engine).dictionaries[0].current().is_none());
            observe(vocabulary, 1, 0);
            observe(vocabulary, 1, 1);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            let current = (*engine).dictionaries[0].current().unwrap();
            assert_eq!(
                current.lookup(&"new".encode_utf16().collect::<Vec<_>>()),
                Membership::Recognized
            );
            assert_eq!(
                current.lookup(&"old".encode_utf16().collect::<Vec<_>>()),
                Membership::Absent
            );
            kr_vocabulary_free(vocabulary);
            kr_engine_free(engine);
        }
    }

    unsafe fn configure_flags(
        vocabulary: *mut VocabularyHandle,
        language: u32,
        path: &Path,
        flags: u32,
    ) -> u32 {
        let bytes = bytes(path);
        // SAFETY: the test retains the handle and readable distinct borrowed path for the call
        unsafe {
            kr_vocabulary_configure(
                vocabulary,
                language,
                bytes.as_ptr(),
                bytes.len() as u32,
                flags,
            )
        }
    }

    #[test]
    fn source_missing_is_empty_applies_only_to_absent_files_before_a_publication() {
        let directory = Directory::new();
        let absent = directory.0.join("absent.add");
        let configured = directory.0.join("configured.add");
        // SAFETY: handles and outputs are distinct live objects; the engine is used only on this
        // thread and every handle is freed once after its last use
        unsafe {
            let (engine, vocabulary) = handles();
            assert_eq!(kr_set_automatic_policy(engine, 2), OK);
            let mut output = 77;
            for flags in [2, 3, u32::MAX] {
                assert_eq!(configure_flags(vocabulary, 1, &absent, flags), BAD_TAG);
            }
            assert!((*vocabulary).published.lock().unwrap()[0].path.is_none());

            // Without the flag an absent file stays missing and automatic correction abstains
            assert_eq!(configure_flags(vocabulary, 1, &absent, 0), OK);
            assert_eq!(kr_vocabulary_observe(vocabulary, 1, &mut output), SOURCE_IO);
            assert_eq!(output, 77);

            // Changing only the flag is a new configuration; the absent file is then empty
            assert_eq!(configure_flags(vocabulary, 1, &absent, 1), OK);
            assert_eq!(configure_flags(vocabulary, 2, &configured, 1), OK);
            observe(vocabulary, 1, 1);
            observe(vocabulary, 1, 2);
            observe(vocabulary, 2, 1);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert!((*engine).dictionaries[0].current().unwrap().is_empty());
            assert_eq!(
                mark(&type_text(engine, 1, "ghbdtn ")),
                "привет ".encode_utf16().collect::<Vec<_>>()
            );

            // A file created later replaces the empty source through the usual settling
            fs::write(&configured, b"rjn").unwrap();
            observe(vocabulary, 2, 0);
            observe(vocabulary, 2, 1);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            let published = (*engine).dictionaries[1].current().unwrap();
            assert_eq!(
                published.lookup(&"rjn".encode_utf16().collect::<Vec<_>>()),
                Membership::Recognized
            );

            // Once a file was published, its absence is an ordinary failure that keeps last-good
            fs::remove_file(&configured).unwrap();
            assert_eq!(kr_vocabulary_observe(vocabulary, 2, &mut output), SOURCE_IO);
            assert_eq!(output, 77);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert!(Arc::ptr_eq(
                &published,
                &(*engine).dictionaries[1].current().unwrap()
            ));

            // The flag doesn't excuse other errors: a directory is still not a regular file
            assert_eq!(configure_flags(vocabulary, 1, &directory.0, 1), OK);
            assert_eq!(
                kr_vocabulary_observe(vocabulary, 1, &mut output),
                SOURCE_NOT_REGULAR
            );
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert!((*engine).dictionaries[0].current().is_none());
            kr_vocabulary_free(vocabulary);
            kr_engine_free(engine);
        }
    }

    unsafe fn configure_default(
        vocabulary: *mut VocabularyHandle,
        language: u32,
        home: &[u8],
    ) -> u32 {
        // SAFETY: the test retains the handle and readable distinct borrowed home for the call
        unsafe {
            kr_vocabulary_configure_default(vocabulary, language, home.as_ptr(), home.len() as u32)
        }
    }

    #[cfg(not(windows))]
    const DEFAULT_SUFFIX: &str = "/.vim/spell/en.utf-8.add";
    #[cfg(windows)]
    const DEFAULT_SUFFIX: &str = "\\vimfiles\\spell\\en.utf-8.add";

    #[test]
    fn default_home_arguments_are_checked_and_bound_the_joined_path() {
        // SAFETY: every accepted pointer refers to a distinct live local allocation; the handle is
        // serialized here and freed once after its last use
        unsafe {
            let mut vocabulary = std::ptr::null_mut();
            assert_eq!(kr_vocabulary_new(ABI, &mut vocabulary), OK);
            assert_eq!(configure_default(std::ptr::null_mut(), 1, ROOT), NULL);
            assert_eq!(configure_default(vocabulary, 3, ROOT), BAD_TAG);
            assert_eq!(configure_default(vocabulary, 1, b""), PATH);
            assert_eq!(configure_default(vocabulary, 1, b"home"), PATH);
            assert_eq!(
                configure_default(vocabulary, 1, &[ROOT, b"a\0b"].concat()),
                PATH
            );
            assert_eq!(
                kr_vocabulary_configure_default(vocabulary, 1, std::ptr::null(), 1),
                NULL
            );
            assert_eq!(
                kr_vocabulary_configure_default(
                    vocabulary,
                    1,
                    std::ptr::null(),
                    MAX_PATH as u32 + 1
                ),
                BOUNDS
            );

            // The joined file name, not only the home, has to fit in MAX_PATH
            let mut home = ROOT.to_vec();
            home.resize(MAX_PATH - DEFAULT_SUFFIX.len() + 1, b'h');
            assert_eq!(configure_default(vocabulary, 1, &home), BOUNDS);
            assert!((*vocabulary).published.lock().unwrap()[0].path.is_none());
            home.pop();
            assert_eq!(configure_default(vocabulary, 1, &home), OK);
            let languages = (*vocabulary).published.lock().unwrap();
            let configured = bytes(languages[0].path.as_ref().unwrap());
            assert_eq!(configured, [&home, DEFAULT_SUFFIX.as_bytes()].concat());
            assert_eq!(configured.len(), MAX_PATH);
            assert!(languages[0].missing_is_empty);
            drop(languages);
            kr_vocabulary_free(vocabulary);
        }
    }

    #[test]
    fn source_default_names_vims_spell_files_and_treats_their_absence_as_empty() {
        let directory = Directory::new();
        let home = bytes(&directory.0).to_vec();
        let expected = [
            [&home, DEFAULT_SUFFIX.as_bytes()].concat(),
            [&home, DEFAULT_SUFFIX.replace("en.", "ru.").as_bytes()].concat(),
        ];
        // SAFETY: handles and outputs are distinct live objects; the engine is used only on this
        // thread and every handle is freed once after its last use
        unsafe {
            let (engine, vocabulary) = handles();
            assert_eq!(kr_set_automatic_policy(engine, 2), OK);
            for language in [1, 2] {
                assert_eq!(configure_default(vocabulary, language, &home), OK);
            }
            for (index, expected) in expected.iter().enumerate() {
                let languages = (*vocabulary).published.lock().unwrap();
                assert_eq!(bytes(languages[index].path.as_ref().unwrap()), expected);
            }

            // Neither file exists, so both languages publish empty and correction runs
            observe(vocabulary, 1, 1);
            observe(vocabulary, 2, 1);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert_eq!(
                mark(&type_text(engine, 1, "ghbdtn ")),
                "привет ".encode_utf16().collect::<Vec<_>>()
            );

            // A file Vim creates there later replaces the empty source and protects its words
            let english = Path::new(std::str::from_utf8(&expected[0]).unwrap());
            fs::create_dir_all(english.parent().unwrap()).unwrap();
            fs::write(english, b"ghbdtn\n").unwrap();
            observe(vocabulary, 1, 0);
            observe(vocabulary, 1, 1);

            // Repeating the default is idempotent and keeps the published file
            let generation = (*vocabulary).published.lock().unwrap()[0].generation;
            assert_eq!(configure_default(vocabulary, 1, &home), OK);
            assert_eq!(
                (*vocabulary).published.lock().unwrap()[0].generation,
                generation
            );
            observe(vocabulary, 1, 2);
            assert_eq!(kr_adopt_vocabulary(engine, vocabulary), OK);
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(
                mark(&type_text(engine, 1, "ghbdtn ")),
                "ghbdtn ".encode_utf16().collect::<Vec<_>>()
            );
            kr_vocabulary_free(vocabulary);
            kr_engine_free(engine);
        }
    }
}
