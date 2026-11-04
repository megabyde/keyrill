//! C ABI over [`keyrill_core`] for native input-method adapters.
//!
//! Built as a static library and linked into each platform adapter. `include/keyrill.h` is the
//! caller contract: struct layouts, ownership, threading, versioning and every status code. The
//! exports here implement it and are not meant to be called from Rust.
//!
//! - Engine handles (`kr_engine_*`, `kr_plan_event`, `kr_commit`) wrap one
//!   [`keyrill_core::session::Session`] each, copy every buffer across the boundary, catch
//!   panics and poison the handle on one.
//! - Configuration (`kr_set_automatic_policy`, `kr_set_keyboard`) applies while idle.
//! - Dictionaries (`kr_set_dictionary_utf8`, `kr_clear_dictionary`,
//!   `kr_dictionary_skipped_lines`) publish and describe immutable snapshots; reading files
//!   stays in the adapter.
//! - The compatibility family (`kr_plan_compatibility_event`, `kr_commit_compatibility`) tracks
//!   observed input for the macOS synthetic-rewrite backend, separately from verified edits.
//! - On macOS, `vocabulary_source` and `kr_vocabulary_*` read and settle dictionary files on a
//!   serial queue for the adapter.
//!
//! `docs/porting.md` explains how a new platform adapter uses these calls.
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "macos")]
pub mod vocabulary_source;

#[cfg(target_os = "macos")]
pub mod vocabulary_host;

mod compatibility;

#[cfg(all(test, target_os = "macos"))]
fn descriptor_flags(file: &std::fs::File) -> i32 {
    use std::os::fd::AsRawFd;
    // SAFETY: the borrowed File keeps the descriptor live; F_GETFL takes no third argument
    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) }
}

use keyrill_core::inference::{Dictionaries, Policy};
use keyrill_core::mapping::Keyboard;
use keyrill_core::model::{Model, ModelError};
use keyrill_core::session::{Action, Context, Disposition, Event, Mode, Outcome, Session};
use keyrill_core::vocabulary::{MAX_SOURCE_BYTES, Snapshot};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

const OK: u32 = 0;
const NULL: u32 = 1;
const BOUNDS: u32 = 2;
const BAD_TAG: u32 = 3;
const STALE: u32 = 4;
const STATE: u32 = 5;
const POISONED: u32 = 6;
const VERSION: u32 = 7;
const MODEL_ASSET: u32 = 8;
const DICTIONARY_DATA: u32 = 9;
const ABI: u32 = 0x0000_0001;

#[derive(Default, PartialEq, Eq)]
enum Backend {
    #[default]
    Unselected,
    Composition,
    Compatibility,
}

#[derive(Clone, Copy)]
struct CompatibilityPending {
    id: u64,
    rewrite: bool,
    boundary: bool,
    word_boundary: bool,
    collect: bool,
}

// Before 1.0 the ABI makes no compatibility promise, so only the exact version is accepted
fn compatible_abi(abi: u32) -> bool {
    abi == ABI
}

#[derive(Default)]
enum Automatic {
    #[default]
    Off,
    Fixture,
    Model(Model<'static>),
}

/// Opaque native-owned handle; mutable access must be serialized without aliasing.
pub struct Engine {
    session: Session,
    poisoned: bool,
    automatic: Automatic,
    dictionaries: [Snapshot; 2],
    backend: Backend,
    compatibility_boundary: bool,
    compatibility_word_boundary: bool,
    compatibility_collect: bool,
    compatibility_pending: Option<CompatibilityPending>,
}

/// Fixed-size event layout matching the C header.
#[repr(C)]
pub struct NativeEvent {
    role: u32,
    mode: u32,
    context: u32,
    len: u32,
    units: [u16; 8],
}

/// Copied fixed-size proposal; no Rust allocation crosses the interface.
#[repr(C)]
pub struct NativePlan {
    plan_id: u64,
    action: u32,
    disposition: u32,
    expected_len: u32,
    mark_len: u32,
    expected: [u16; 128],
    mark: [u16; 128],
}

/// Best-effort committed-input proposal; submission is not document verification.
#[repr(C)]
pub struct CompatibilityPlan {
    plan_id: u64,
    operation: u32,
    disposition: u32,
    delete_units: u32,
    replacement_len: u32,
    replacement: [u16; 128],
}

const _: () = assert!(size_of::<CompatibilityPlan>() == 280);

const _: () = assert!(size_of::<NativeEvent>() == 32);
const _: () = assert!(size_of::<NativePlan>() == 536);
const _: () = assert!(std::mem::offset_of!(NativePlan, expected) == 24);
const _: () = assert!(keyrill_core::session::MAX_WORD < 128);
const _: () = assert!(MAX_SOURCE_BYTES == 1_048_576);

fn embedded_model() -> Result<Model<'static>, ModelError> {
    static MODEL: OnceLock<Result<Model<'static>, ModelError>> = OnceLock::new();
    *MODEL.get_or_init(Model::embedded)
}

fn set_policy(
    engine: &mut Engine,
    policy: u32,
    model: impl FnOnce() -> Result<Model<'static>, ModelError>,
) -> u32 {
    if policy > 2 {
        return BAD_TAG;
    }
    if let Err(error) = engine.session.set_fixtures(policy == 1) {
        return status(error);
    }
    if matches!(
        (&engine.automatic, policy),
        (Automatic::Off, 0) | (Automatic::Fixture, 1) | (Automatic::Model(_), 2)
    ) {
        return OK;
    }
    engine.session.reset();
    engine.compatibility_pending = None;
    engine.compatibility_boundary = false;
    engine.compatibility_word_boundary = false;
    engine.compatibility_collect = true;
    engine.automatic = Automatic::Off;
    engine.automatic = match policy {
        0 => Automatic::Off,
        1 => Automatic::Fixture,
        _ => match model() {
            Ok(model) => Automatic::Model(model),
            Err(_) => return MODEL_ASSET,
        },
    };
    OK
}

fn status(error: keyrill_core::session::Error) -> u32 {
    match error {
        keyrill_core::session::Error::Stale => STALE,
        keyrill_core::session::Error::Invalidated => STATE,
    }
}

fn decode_event(event: &NativeEvent) -> Result<(Event, Mode, Context), u32> {
    if event.len > 8 {
        return Err(BOUNDS);
    }
    let mode = match event.mode {
        1 => Mode::En,
        2 => Mode::Ru,
        _ => return Err(BAD_TAG),
    };
    let context = match event.context {
        0 => Context::Boundary,
        1 => Context::Ineligible,
        _ => return Err(BAD_TAG),
    };
    if event.role != 1 && event.len != 0 {
        return Err(BAD_TAG);
    }
    let input = match event.role {
        1 => Event::Text(event.units[..event.len as usize].to_vec()),
        2 => Event::Backspace,
        3 => Event::Escape,
        4 => Event::Manual,
        5 => Event::Reverse,
        6 => Event::Finish,
        _ => return Err(BAD_TAG),
    };
    Ok((input, mode, context))
}

fn propose(
    engine: &mut Engine,
    input: &Event,
    mode: Mode,
    context: Context,
    automatic: bool,
) -> Result<keyrill_core::session::Plan, keyrill_core::session::Error> {
    if !automatic {
        return engine
            .session
            .propose_without_automatic(input, mode, context);
    }
    match &engine.automatic {
        Automatic::Model(model) => {
            let en = engine.dictionaries[0].current();
            let ru = engine.dictionaries[1].current();
            engine.session.propose_with_inference(
                input,
                mode,
                context,
                model,
                Dictionaries {
                    en: en.as_deref(),
                    ru: ru.as_deref(),
                },
                Policy::default(),
            )
        }
        _ => engine.session.propose(input, mode, context),
    }
}

// Callers provide live, uniquely borrowed handles. No callback is made while this
// borrow exists; every export releases it before returning to Objective-C++.
unsafe fn with_engine(pointer: *mut Engine, f: impl FnOnce(&mut Engine) -> u32) -> u32 {
    if pointer.is_null() {
        return NULL;
    }
    // SAFETY: the exported caller contract requires a live aligned unique handle
    let engine = unsafe { &mut *pointer };
    if engine.poisoned {
        return POISONED;
    }
    match catch_unwind(AssertUnwindSafe(|| f(engine))) {
        Ok(result) => result,
        Err(_) => {
            engine.poisoned = true;
            engine.session.reset();
            engine.compatibility_pending = None;
            POISONED
        }
    }
}

/// Return the major/minor ABI version. No allocation or panicking work occurs.
#[unsafe(no_mangle)]
pub extern "C" fn kr_abi_version() -> u32 {
    ABI
}

/// Allocate a session with the OFF policy.
///
/// # Safety
/// `out` must be writable and aligned for one handle pointer; it must not alias a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_engine_new(abi: u32, out: *mut *mut Engine) -> u32 {
    if out.is_null() {
        return NULL;
    }
    // SAFETY: caller supplies a valid writable pointer-sized output object
    unsafe {
        out.write(std::ptr::null_mut());
    }
    if !compatible_abi(abi) {
        return VERSION;
    }
    match catch_unwind(|| {
        Box::new(Engine {
            session: Session::default(),
            poisoned: false,
            automatic: Automatic::Off,
            dictionaries: [Snapshot::default(), Snapshot::default()],
            backend: Backend::Unselected,
            compatibility_boundary: false,
            compatibility_word_boundary: false,
            compatibility_collect: true,
            compatibility_pending: None,
        })
    }) {
        Ok(engine) => {
            // SAFETY: same output contract; allocation is transferred exactly once
            unsafe {
                out.write(Box::into_raw(engine));
            }
            OK
        }
        Err(_) => POISONED,
    }
}

/// Free once, after all callbacks have drained. Null is a no-op.
///
/// # Safety
/// A non-null pointer must be the live allocation returned by `kr_engine_new`, with no aliases.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_engine_free(pointer: *mut Engine) {
    if !pointer.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: caller transfers the original unique allocation exactly once
            drop(unsafe { Box::from_raw(pointer) });
        }));
    }
}

/// Clear native lifecycle-dependent state; poisoning is permanent.
///
/// # Safety
/// `pointer` must meet the live unique serialized handle contract, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_reset(pointer: *mut Engine) -> u32 {
    // SAFETY: forwarded handle contract; no native callbacks occur in this closure
    unsafe {
        with_engine(pointer, |engine| {
            engine.session.reset();
            engine.compatibility_pending = None;
            engine.compatibility_boundary = false;
            engine.compatibility_word_boundary = false;
            engine.compatibility_collect = true;
            OK
        })
    }
}

/// Select OFF, the test FIXTURE, or the frozen MODEL policy while idle, outside input callbacks.
///
/// # Safety
/// `pointer` must meet the live unique serialized handle contract, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_set_automatic_policy(pointer: *mut Engine, policy: u32) -> u32 {
    // SAFETY: forwarded handle contract; validation performs no native callbacks
    unsafe { with_engine(pointer, |engine| set_policy(engine, policy, embedded_model)) }
}

/// Select the APPLE or PC Russian map while idle, outside input callbacks.
///
/// # Safety
/// `pointer` must meet the live unique serialized handle contract, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_set_keyboard(pointer: *mut Engine, keyboard: u32) -> u32 {
    // SAFETY: forwarded handle contract; validation performs no native callbacks
    unsafe {
        with_engine(pointer, |engine| {
            let keyboard = match keyboard {
                0 => Keyboard::Apple,
                1 => Keyboard::Pc,
                _ => return BAD_TAG,
            };
            engine
                .session
                .set_keyboard(keyboard)
                .map_or_else(status, |()| OK)
        })
    }
}

/// Publish a complete bounded UTF-8 protection source, retaining last-good on parse failure.
/// Prepare accepted updates outside input callbacks; an outstanding proposal rejects publication.
///
/// # Safety
/// The handle must be live and uniquely borrowed for this serialized call, or null. Non-null
/// `bytes` with `len <= MAX_SOURCE_BYTES` must address that many readable bytes without aliasing
/// the handle. Null and oversized arguments are checked without dereferencing the buffer; null
/// with zero length is valid empty data. No input pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_set_dictionary_utf8(
    pointer: *mut Engine,
    language: u32,
    bytes: *const u8,
    len: u32,
) -> u32 {
    // SAFETY: forwarded handle contract; candidate bytes are borrowed only inside this closure
    unsafe {
        with_engine(pointer, |engine| {
            let index = match language {
                1 => 0,
                2 => 1,
                _ => return BAD_TAG,
            };
            if engine.session.has_pending() {
                return STATE;
            }
            if len as usize > MAX_SOURCE_BYTES {
                return BOUNDS;
            }
            let source = if len == 0 {
                &[]
            } else {
                if bytes.is_null() {
                    return NULL;
                }
                // SAFETY: nonzero len is bounded; caller guarantees readable nonaliasing bytes
                std::slice::from_raw_parts(bytes, len as usize)
            };
            engine.dictionaries[index]
                .replace(source)
                .map_or(DICTIONARY_DATA, |()| OK)
        })
    }
}

/// Explicitly make a protection source missing, without reviving editing authority.
/// Apply outside input callbacks; an outstanding proposal rejects the operation.
///
/// # Safety
/// `pointer` must meet the live unique serialized handle contract, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_clear_dictionary(pointer: *mut Engine, language: u32) -> u32 {
    // SAFETY: forwarded handle contract; no native callbacks occur
    unsafe {
        with_engine(pointer, |engine| {
            let index = match language {
                1 => 0,
                2 => 1,
                _ => return BAD_TAG,
            };
            if engine.session.has_pending() {
                return STATE;
            }
            engine.dictionaries[index] = Snapshot::default();
            OK
        })
    }
}

/// Write the skipped-line count of a language's current source; a missing source is STATE.
///
/// # Safety
/// `pointer` must meet the live unique serialized handle contract, or be null. A non-null `out`
/// must be aligned and writable for one `u32` and must not alias the handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_dictionary_skipped_lines(
    pointer: *mut Engine,
    language: u32,
    out: *mut u32,
) -> u32 {
    if out.is_null() {
        return NULL;
    }
    // SAFETY: forwarded handle contract; `out` is written once after validation
    unsafe {
        with_engine(pointer, |engine| {
            let index = match language {
                1 => 0,
                2 => 1,
                _ => return BAD_TAG,
            };
            let Some(source) = engine.dictionaries[index].current() else {
                return STATE;
            };
            out.write(source.skipped() as u32);
            OK
        })
    }
}

/// Copy a proposal without installing its portable state.
///
/// # Safety
/// Handle, event and output must be live, aligned, nonaliasing C objects with their declared sizes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_plan_event(
    pointer: *mut Engine,
    event: *const NativeEvent,
    out: *mut NativePlan,
) -> u32 {
    if event.is_null() || out.is_null() {
        return NULL;
    }
    // SAFETY: forwarded handle and object contracts; all native data is copied
    unsafe {
        with_engine(pointer, |engine| {
            let event = &*event;
            let (input, mode, context) = match decode_event(event) {
                Ok(decoded) => decoded,
                Err(error) => return error,
            };
            if engine.backend == Backend::Compatibility {
                return STATE;
            }
            let proposal = propose(engine, &input, mode, context, true);
            let plan = match proposal {
                Ok(plan) => plan,
                Err(error) => return status(error),
            };
            engine.backend = Backend::Composition;
            let mut copied = NativePlan {
                plan_id: plan.id,
                action: match plan.action {
                    Action::Pass => 0,
                    Action::Mark => 1,
                    Action::Finish => 2,
                    Action::Unavailable => 3,
                },
                disposition: match plan.disposition {
                    Disposition::Pass => 0,
                    Disposition::Consumed => 1,
                    Disposition::Replan => 2,
                },
                expected_len: plan.expected.len() as u32,
                mark_len: plan.text.len() as u32,
                expected: [0; 128],
                mark: [0; 128],
            };
            copied.expected[..plan.expected.len()].copy_from_slice(&plan.expected);
            copied.mark[..plan.text.len()].copy_from_slice(&plan.text);
            out.write(copied);
            OK
        })
    }
}

/// Acknowledge only the latest token; refused and unknown outcomes invalidate the session.
///
/// # Safety
/// `pointer` must meet the live unique serialized handle contract, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_commit(pointer: *mut Engine, token: u64, outcome: u32) -> u32 {
    // SAFETY: forwarded handle contract
    unsafe {
        with_engine(pointer, |engine| {
            let outcome = match outcome {
                0 => Outcome::Verified,
                1 => Outcome::Refused,
                2 => Outcome::Unknown,
                _ => return BAD_TAG,
            };
            if engine.backend == Backend::Compatibility {
                return STATE;
            }
            engine
                .session
                .acknowledge(token, outcome)
                .map_or_else(status, |()| OK)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) unsafe fn apply_event(engine: *mut Engine, event: &NativeEvent) -> NativePlan {
        // SAFETY: the test retains the unique live handle and distinct local C objects
        unsafe {
            for attempt in 0..2 {
                let mut out = std::mem::MaybeUninit::<NativePlan>::uninit();
                assert_eq!(kr_plan_event(engine, event, out.as_mut_ptr()), OK);
                let plan = out.assume_init();
                assert_eq!(kr_commit(engine, plan.plan_id, 0), OK);
                if plan.disposition != 2 {
                    return plan;
                }
                assert_eq!((attempt, plan.action), (0, 2));
            }
            unreachable!("a second replan is a protocol error")
        }
    }

    pub(super) unsafe fn type_text(engine: *mut Engine, mode: u32, text: &str) -> NativePlan {
        // SAFETY: the handle remains live and serialized; every event is a distinct local object
        unsafe {
            text.encode_utf16()
                .map(|unit| {
                    let mut units = [0; 8];
                    units[0] = unit;
                    apply_event(
                        engine,
                        &NativeEvent {
                            role: 1,
                            mode,
                            context: 0,
                            len: 1,
                            units,
                        },
                    )
                })
                .last()
                .unwrap()
        }
    }

    #[test]
    fn dictionary_buffers_configuration_and_failed_asset_are_transactional() {
        use keyrill_core::vocabulary::Membership;
        use std::sync::Arc;

        // SAFETY: inputs are null where allowed or backed by live distinct C objects/buffers.
        // No call overlaps another Rust borrow; each allocated handle is freed exactly once.
        unsafe {
            let mut engine = std::ptr::null_mut();
            assert_eq!(kr_abi_version(), 0x0000_0001);
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_set_automatic_policy(std::ptr::null_mut(), 0), NULL);
            assert_eq!(kr_clear_dictionary(std::ptr::null_mut(), 1), NULL);
            assert_eq!(
                kr_set_dictionary_utf8(std::ptr::null_mut(), 1, std::ptr::null(), 0),
                NULL
            );
            assert_eq!(kr_set_automatic_policy(engine, 3), BAD_TAG);
            assert_eq!(
                kr_set_dictionary_utf8(engine, 0, std::ptr::null(), 0),
                BAD_TAG
            );
            assert_eq!(kr_clear_dictionary(engine, 3), BAD_TAG);
            assert_eq!(kr_set_dictionary_utf8(engine, 1, std::ptr::null(), 1), NULL);
            assert_eq!(
                kr_set_dictionary_utf8(engine, 1, std::ptr::null(), MAX_SOURCE_BYTES as u32 + 1),
                BOUNDS
            );
            let mut source = b"cgbcjr".to_vec();
            assert_eq!(
                kr_set_dictionary_utf8(engine, 1, source.as_ptr(), source.len() as u32),
                OK
            );
            let old = (*engine).dictionaries[0].current().unwrap();
            source.fill(b'x');
            let original: Vec<_> = "cgbcjr".encode_utf16().collect();
            assert_eq!(old.lookup(&original), Membership::Recognized);
            for invalid in [b"a\0b".as_slice(), b"/encoding=latin1\nword"] {
                assert_eq!(
                    kr_set_dictionary_utf8(engine, 1, invalid.as_ptr(), invalid.len() as u32),
                    DICTIONARY_DATA
                );
                assert!(Arc::ptr_eq(
                    &old,
                    &(*engine).dictionaries[0].current().unwrap()
                ));
            }
            let event = NativeEvent {
                role: 1,
                mode: 1,
                context: 0,
                len: 1,
                units: [97; 8],
            };
            let mut out = std::mem::MaybeUninit::<NativePlan>::uninit();
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            let pending = out.assume_init_read();
            assert_eq!(
                kr_set_dictionary_utf8(engine, 1, source.as_ptr(), source.len() as u32),
                STATE
            );
            assert_eq!(kr_clear_dictionary(engine, 1), STATE);
            assert_eq!(kr_set_automatic_policy(engine, 2), STATE);
            assert!(Arc::ptr_eq(
                &old,
                &(*engine).dictionaries[0].current().unwrap()
            ));
            assert_eq!(kr_commit(engine, pending.plan_id, 0), OK);
            assert_eq!(kr_set_automatic_policy(engine, 2), STATE);
            assert_eq!(kr_set_dictionary_utf8(engine, 1, source.as_ptr(), 0), OK);
            assert!((*engine).dictionaries[0].current().unwrap().is_empty());
            assert_eq!(kr_set_dictionary_utf8(engine, 2, std::ptr::null(), 0), OK);
            assert_eq!(kr_clear_dictionary(engine, 1), OK);
            assert!((*engine).dictionaries[0].current().is_none());
            assert!((*engine).dictionaries[1].current().unwrap().is_empty());
            assert_eq!(old.lookup(&original), Membership::Recognized);
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            assert_eq!(kr_commit(engine, out.assume_init_read().plan_id, 2), OK);
            assert_eq!(kr_clear_dictionary(engine, 2), OK);
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), STATE);
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(kr_set_automatic_policy(engine, 1), OK);
            assert_eq!(
                with_engine(engine, |engine| set_policy(engine, 2, || Err(
                    ModelError::Magic
                ))),
                MODEL_ASSET
            );
            let plan = type_text(engine, 1, "ghbdtn ");
            assert_eq!(
                &plan.mark[..plan.mark_len as usize],
                "ghbdtn ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(
                with_engine(engine, |_| panic!("synthetic policy containment test")),
                POISONED
            );
            assert_eq!(kr_set_automatic_policy(engine, 0), POISONED);
            assert_eq!(
                kr_set_dictionary_utf8(engine, 1, std::ptr::null(), 0),
                POISONED
            );
            assert_eq!(kr_clear_dictionary(engine, 1), POISONED);
            kr_engine_free(engine);

            assert_eq!(kr_engine_new(0x0001_0000, &mut engine), VERSION);
            assert!(engine.is_null());
            assert_eq!(kr_engine_new(ABI - 1, &mut engine), VERSION);
            assert!(engine.is_null());
        }
    }

    #[test]
    fn keyboard_selection_and_skipped_lines_follow_the_configuration_contract() {
        let units = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        let event = |role, mode| NativeEvent {
            role,
            mode,
            context: 0,
            len: 0,
            units: [0; 8],
        };
        // SAFETY: the test exclusively retains the opaque allocation; events, outputs and
        // source buffers are live distinct locals for each serialized call and never retained
        unsafe {
            let mut engine = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_set_keyboard(std::ptr::null_mut(), 0), NULL);
            for (keyboard, converted) in [(None, "\\krf"), (Some(1), "`krf"), (Some(0), "\\krf")] {
                if let Some(keyboard) = keyboard {
                    assert_eq!(kr_set_keyboard(engine, keyboard), OK);
                    assert_eq!(kr_set_keyboard(engine, keyboard), OK);
                }
                assert_eq!(kr_reset(engine), OK);
                type_text(engine, 2, "ёлка");
                assert_eq!(kr_set_keyboard(engine, 1), STATE);
                let manual = apply_event(engine, &event(4, 2));
                assert_eq!(&manual.mark[..manual.mark_len as usize], units(converted));
                let reverse = apply_event(engine, &event(5, 2));
                assert_eq!(&reverse.mark[..reverse.mark_len as usize], units("ёлка"));
                apply_event(engine, &event(6, 2));
            }
            for tag in [2, u32::MAX] {
                assert_eq!(kr_set_keyboard(engine, tag), BAD_TAG);
            }

            // Pending and owned states refuse, and reset keeps the selection
            assert_eq!(kr_set_keyboard(engine, 1), OK);
            let mut out = std::mem::MaybeUninit::<NativePlan>::uninit();
            let mut letter = event(1, 2);
            letter.len = 1;
            letter.units[0] = 0x451;
            assert_eq!(kr_plan_event(engine, &letter, out.as_mut_ptr()), OK);
            let pending = out.assume_init_read();
            assert_eq!(kr_set_keyboard(engine, 0), STATE);
            assert_eq!(kr_commit(engine, pending.plan_id, 0), OK);
            assert_eq!(kr_set_keyboard(engine, 0), STATE);
            assert_eq!(kr_reset(engine), OK);
            type_text(engine, 2, "ё");
            let manual = apply_event(engine, &event(4, 2));
            assert_eq!(&manual.mark[..manual.mark_len as usize], units("`"));
            let plan = apply_event(engine, &event(3, 2));
            assert_eq!((plan.action, plan.disposition, plan.mark_len), (2, 1, 1));
            assert_eq!(plan.mark[0], 0x451);
            type_text(engine, 2, "ё");
            let plan = apply_event(engine, &event(3, 2));
            assert_eq!((plan.action, plan.disposition, plan.mark_len), (2, 0, 1));

            let mut skipped = 77;
            assert_eq!(kr_dictionary_skipped_lines(engine, 1, &mut skipped), STATE);
            assert_eq!(skipped, 77);
            let mut removed = "ёж".as_bytes().to_vec();
            removed[0] = b'#';
            let mut source = b"keep\nbad/*\n/future\n\xff\n".to_vec();
            source.extend_from_slice(&removed);
            assert_eq!(
                kr_set_dictionary_utf8(engine, 1, source.as_ptr(), source.len() as u32),
                OK
            );
            assert_eq!(kr_dictionary_skipped_lines(engine, 1, &mut skipped), OK);
            assert_eq!(skipped, 3);
            assert_eq!((*engine).dictionaries[0].current().unwrap().len(), 1);
            assert_eq!(
                kr_dictionary_skipped_lines(engine, 1, std::ptr::null_mut()),
                NULL
            );
            assert_eq!(
                kr_dictionary_skipped_lines(std::ptr::null_mut(), 1, &mut skipped),
                NULL
            );
            assert_eq!(
                kr_dictionary_skipped_lines(engine, 3, &mut skipped),
                BAD_TAG
            );
            assert_eq!(kr_dictionary_skipped_lines(engine, 2, &mut skipped), STATE);
            assert_eq!(
                kr_set_dictionary_utf8(engine, 1, b"\0a".as_ptr(), 2),
                DICTIONARY_DATA
            );
            assert_eq!(kr_dictionary_skipped_lines(engine, 1, &mut skipped), OK);
            assert_eq!(skipped, 3);
            assert_eq!(kr_set_dictionary_utf8(engine, 1, std::ptr::null(), 0), OK);
            assert_eq!(kr_dictionary_skipped_lines(engine, 1, &mut skipped), OK);
            assert_eq!(skipped, 0);

            assert_eq!(
                with_engine(engine, |_| panic!("synthetic keyboard containment test")),
                POISONED
            );
            assert_eq!(kr_set_keyboard(engine, 0), POISONED);
            assert_eq!(
                kr_dictionary_skipped_lines(engine, 1, &mut skipped),
                POISONED
            );
            kr_engine_free(engine);
        }
    }

    #[test]
    fn model_policy_bridge_preserves_configuration_context_and_receipts() {
        // SAFETY: the test exclusively retains the opaque allocation; all text buffers and event
        // objects are readable/aligned for the complete serialized call and are never retained
        unsafe {
            let mut engine = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_set_automatic_policy(engine, 2), OK);
            let plan = type_text(engine, 1, "cgbcjr ");
            assert_eq!(
                &plan.mark[..plan.mark_len as usize],
                "cgbcjr ".encode_utf16().collect::<Vec<_>>()
            );
            let manual = apply_event(
                engine,
                &NativeEvent {
                    role: 4,
                    mode: 1,
                    context: 0,
                    len: 0,
                    units: [0; 8],
                },
            );
            assert_eq!(
                &manual.mark[..manual.mark_len as usize],
                "список ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(kr_set_dictionary_utf8(engine, 1, std::ptr::null(), 0), OK);
            assert_eq!(kr_set_dictionary_utf8(engine, 2, std::ptr::null(), 0), OK);
            for (mode, raw, corrected) in [(1, "cgbcjr ", "список "), (2, "цщкдв ", "world ")]
            {
                assert_eq!(kr_reset(engine), OK);
                let plan = type_text(engine, mode, raw);
                assert_eq!(
                    &plan.mark[..plan.mark_len as usize],
                    corrected.encode_utf16().collect::<Vec<_>>()
                );
                let source = "cgbcjr\nцщкдв\n".as_bytes();
                assert_eq!(
                    kr_set_dictionary_utf8(engine, 1, source.as_ptr(), source.len() as u32),
                    OK
                );
                let reverse = apply_event(
                    engine,
                    &NativeEvent {
                        role: 5,
                        mode,
                        context: 0,
                        len: 0,
                        units: [0; 8],
                    },
                );
                assert_eq!(
                    &reverse.mark[..reverse.mark_len as usize],
                    raw.encode_utf16().collect::<Vec<_>>()
                );
                assert_eq!(kr_set_dictionary_utf8(engine, 1, std::ptr::null(), 0), OK);
            }
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(kr_clear_dictionary(engine, 2), OK);
            let plan = type_text(engine, 2, "цщкдв ");
            assert_eq!(
                &plan.mark[..plan.mark_len as usize],
                "цщкдв ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(kr_set_dictionary_utf8(engine, 2, std::ptr::null(), 0), OK);
            type_text(engine, 1, "hello world ");
            let event = NativeEvent {
                role: 1,
                mode: 1,
                context: 0,
                len: 1,
                units: [121; 8],
            };
            let mut out = std::mem::MaybeUninit::<NativePlan>::uninit();
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            let finish = out.assume_init_read();
            assert_eq!((finish.action, finish.disposition), (2, 2));
            assert_eq!(kr_commit(engine, finish.plan_id, 0), OK);
            assert_eq!(kr_set_automatic_policy(engine, 2), OK);
            let preserved = type_text(engine, 1, "yfv ");
            assert_eq!(
                &preserved.mark[..preserved.mark_len as usize],
                "yfv ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(kr_reset(engine), OK);
            type_text(engine, 1, "hello world ");
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            assert_eq!(kr_commit(engine, out.assume_init_read().plan_id, 0), OK);
            assert_eq!(kr_set_automatic_policy(engine, 0), OK);
            assert_eq!(kr_set_automatic_policy(engine, 2), OK);
            let neutral = type_text(engine, 1, "yfv ");
            assert_eq!(
                &neutral.mark[..neutral.mark_len as usize],
                "нам ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(kr_set_automatic_policy(engine, 0), OK);
            let disabled = type_text(engine, 1, "cgbcjr ");
            assert_eq!(
                &disabled.mark[..disabled.mark_len as usize],
                "cgbcjr ".encode_utf16().collect::<Vec<_>>()
            );
            kr_engine_free(engine);
        }
    }

    #[test]
    fn abi_null_version_bounds_stale_and_poison_contracts() {
        // SAFETY: all non-null pointers refer to live distinct local objects; the
        // handle is freed once after the final call. No native callbacks occur.
        unsafe {
            let mut engine = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI + 1, &mut engine), VERSION);
            assert!(engine.is_null());
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_reset(std::ptr::null_mut()), NULL);
            let mut event = NativeEvent {
                role: 1,
                mode: 1,
                context: 0,
                len: 9,
                units: [97; 8],
            };
            let mut out = std::mem::MaybeUninit::<NativePlan>::uninit();
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), BOUNDS);
            event.len = 1;
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            let plan = out.assume_init();
            assert_eq!(plan.mark[..plan.mark_len as usize], [97]);
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(kr_commit(engine, plan.plan_id, 0), STALE);
            assert_eq!(
                with_engine(engine, |_| panic!("synthetic containment test")),
                POISONED
            );
            assert_eq!(kr_reset(engine), POISONED);
            kr_engine_free(engine);
            kr_engine_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn copied_plans_manual_boundary_reverse_and_unknown_effect() {
        // SAFETY: local C objects are live, aligned and distinct; the opaque
        // allocation is accessed serially and freed once after all calls
        unsafe {
            let mut engine = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_set_automatic_policy(engine, 3), BAD_TAG);
            assert_eq!(kr_set_automatic_policy(engine, 1), OK);
            let mut event = NativeEvent {
                role: 1,
                mode: 1,
                context: 0,
                len: 1,
                units: [0; 8],
            };
            let mut out = std::mem::MaybeUninit::<NativePlan>::uninit();
            for unit in "ghbdtn".encode_utf16().chain([32]) {
                event.units[0] = unit;
                assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
                let p = out.assume_init_read();
                assert_eq!(p.action, 1);
                if unit == 32 {
                    assert_eq!(
                        &p.mark[..p.mark_len as usize],
                        "привет ".encode_utf16().collect::<Vec<_>>()
                    );
                }
                assert_eq!(kr_commit(engine, p.plan_id, 0), OK);
            }
            event.role = 5;
            event.len = 0;
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            let reversed = out.assume_init_read();
            assert_eq!(
                &reversed.mark[..reversed.mark_len as usize],
                "ghbdtn ".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(kr_commit(engine, reversed.plan_id, 2), OK);
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), STATE);
            assert_eq!(kr_reset(engine), OK);
            event.role = 1;
            event.len = 1;
            event.units[0] = 120;
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            assert_eq!(kr_commit(engine, out.assume_init_read().plan_id, 0), OK);
            event.role = 2;
            event.len = 0;
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), OK);
            let empty = out.assume_init_read();
            assert_eq!((empty.action, empty.mark_len), (1, 0));
            assert_eq!(kr_commit(engine, empty.plan_id, 1), OK);
            assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), STATE);
            kr_engine_free(engine);
        }
    }

    #[test]
    fn invalid_native_tags_never_produce_a_usable_plan() {
        // SAFETY: all pointers are either explicitly null or live local objects;
        // the handle has one owner and no callback can reenter these tests
        unsafe {
            assert_eq!(kr_engine_new(ABI, std::ptr::null_mut()), NULL);
            let mut engine = std::ptr::null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            let mut out = std::mem::MaybeUninit::<NativePlan>::uninit();
            let mut event = NativeEvent {
                role: 1,
                mode: 1,
                context: 0,
                len: 1,
                units: [97; 8],
            };
            assert_eq!(
                kr_plan_event(engine, std::ptr::null(), out.as_mut_ptr()),
                NULL
            );
            assert_eq!(kr_plan_event(engine, &event, std::ptr::null_mut()), NULL);
            for (role, mode, context, len) in [
                (0, 1, 0, 0),
                (7, 1, 0, 0),
                (1, 0, 0, 1),
                (1, 1, 2, 1),
                (4, 1, 0, 1),
            ] {
                event.role = role;
                event.mode = mode;
                event.context = context;
                event.len = len;
                assert_eq!(kr_plan_event(engine, &event, out.as_mut_ptr()), BAD_TAG);
            }
            assert_eq!(kr_commit(engine, 1, 3), BAD_TAG);
            kr_engine_free(engine);
        }
    }
}
