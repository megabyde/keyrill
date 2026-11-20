use super::{
    BOUNDS, DICTIONARY_DATA, Engine, MODEL_ASSET, NULL, OK, POISONED, STATE, VERSION,
    compatible_abi, embedded_model, with_engine,
};
use keyrill_core::model::{Model, ModelError};
use keyrill_core::vocabulary::{MAX_SOURCE_BYTES, ParseError, Snapshot, Vocabulary};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

/// Opaque immutable dictionary. Its owner must drain all borrows before freeing it.
pub struct Dictionary {
    vocabulary: Arc<Vocabulary>,
}

unsafe fn parse_with(
    abi: u32,
    bytes: *const u8,
    len: u32,
    out: *mut *mut Dictionary,
    parse: impl FnOnce(&[u8]) -> Result<Vocabulary, ParseError>,
) -> u32 {
    if out.is_null() {
        return NULL;
    }
    // SAFETY: the caller supplies a distinct writable, aligned pointer-sized output object.
    unsafe { out.write(std::ptr::null_mut()) };
    if !compatible_abi(abi) {
        return VERSION;
    }
    if len as usize > MAX_SOURCE_BYTES {
        return BOUNDS;
    }
    if len != 0 && bytes.is_null() {
        return NULL;
    }
    match catch_unwind(AssertUnwindSafe(|| {
        let source = if len == 0 {
            &[]
        } else {
            // SAFETY: length is bounded and nonzero; the caller supplies readable bytes.
            unsafe { std::slice::from_raw_parts(bytes, len as usize) }
        };
        parse(source).map(|vocabulary| {
            Box::new(Dictionary {
                vocabulary: Arc::new(vocabulary),
            })
        })
    })) {
        Ok(Ok(dictionary)) => {
            // SAFETY: the output remains writable; transfer the allocation exactly once.
            unsafe { out.write(Box::into_raw(dictionary)) };
            OK
        }
        Ok(Err(_)) => DICTIONARY_DATA,
        Err(_) => POISONED,
    }
}

/// Parse bounded UTF-8 Vim data without an engine, retaining no input buffer.
/// Every failure leaves a non-null output object holding null.
///
/// # Safety
/// `out` must be writable and aligned for a pointer and not alias the source. For a nonzero
/// bounded `len`, `bytes` must address that many readable bytes, or be null. Null plus zero
/// is a valid empty source. Null and oversized inputs are checked before reading any bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_dictionary_parse_utf8(
    abi: u32,
    bytes: *const u8,
    len: u32,
    out: *mut *mut Dictionary,
) -> u32 {
    // SAFETY: the caller's buffer and output contracts are forwarded without retention.
    unsafe { parse_with(abi, bytes, len, out, Vocabulary::parse) }
}

/// Free an immutable dictionary once, after all borrows drain. Null is a no-op.
///
/// # Safety
/// A non-null pointer must be the live allocation returned by `kr_dictionary_parse_utf8`.
/// No call may still borrow it; engines that adopted it hold independent ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_dictionary_free(pointer: *mut Dictionary) {
    if !pointer.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: the caller transfers the original allocation exactly once.
            drop(unsafe { Box::from_raw(pointer) });
        }));
    }
}

/// Adopt both immutable dictionaries together without changing policy or session state.
/// Null means missing; a parsed empty dictionary means ready. A pending proposal refuses adoption.
///
/// # Safety
/// `pointer` must be a live unique serialized engine or null. Each non-null dictionary must be
/// a live allocation from `kr_dictionary_parse_utf8`, kept alive throughout this call. Shared
/// immutable dictionary borrows may overlap; none may overlap freeing its handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_adopt_dictionaries(
    pointer: *mut Engine,
    english: *const Dictionary,
    russian: *const Dictionary,
) -> u32 {
    // SAFETY: the caller supplies a unique engine and live immutable dictionary borrows.
    unsafe {
        with_engine(pointer, |engine| {
            if engine.session.has_pending() || engine.compatibility_pending.is_some() {
                return STATE;
            }
            let dictionaries = [english, russian].map(|dictionary| {
                Snapshot::from(
                    dictionary
                        .as_ref()
                        .map(|value| Arc::clone(&value.vocabulary)),
                )
            });
            engine.dictionaries = dictionaries;
            OK
        })
    }
}

fn prepare_with(abi: u32, model: impl FnOnce() -> Result<Model<'static>, ModelError>) -> u32 {
    if !compatible_abi(abi) {
        return VERSION;
    }
    match catch_unwind(AssertUnwindSafe(model)) {
        Ok(Ok(_)) => OK,
        Ok(Err(_)) => MODEL_ASSET,
        Err(_) => POISONED,
    }
}

/// Prepare the process-wide immutable embedded model without creating or changing an engine.
/// Call off the input thread before enabling MODEL. Concurrent preparation may wait on the cache.
#[unsafe(no_mangle)]
pub extern "C" fn kr_prepare_model(abi: u32) -> u32 {
    prepare_with(abi, embedded_model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{apply_event, type_text};
    use crate::{
        ABI, NativeEvent, NativePlan, kr_commit, kr_dictionary_skipped_lines, kr_engine_free,
        kr_engine_new, kr_plan_event, kr_reset, kr_set_automatic_policy, kr_set_keyboard,
    };
    use keyrill_core::vocabulary::Membership;
    use std::ptr::{null, null_mut};

    fn event(role: u32) -> NativeEvent {
        NativeEvent {
            role,
            mode: 1,
            context: 0,
            len: 0,
            units: [0; 8],
        }
    }

    unsafe fn parse(source: &[u8]) -> *mut Dictionary {
        let mut dictionary = null_mut();
        // SAFETY: source and output are distinct locals, valid for this call.
        unsafe {
            assert_eq!(
                kr_dictionary_parse_utf8(
                    ABI,
                    source.as_ptr(),
                    source.len() as u32,
                    &mut dictionary
                ),
                OK
            );
        }
        assert!(!dictionary.is_null());
        dictionary
    }

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn assert_mark(plan: &NativePlan, text: &str) {
        assert_eq!(&plan.mark[..plan.mark_len as usize], units(text));
    }

    #[test]
    fn parse_checks_arguments_before_bytes_and_never_publishes_failure() {
        // SAFETY: outputs are writable distinct locals; invalid source pointers are only used
        // with arguments that must be refused before reading. Successful handles are freed once.
        unsafe {
            let unreadable = std::ptr::NonNull::<u8>::dangling().as_ptr();
            let mut out = std::ptr::NonNull::<Dictionary>::dangling().as_ptr();
            assert_eq!(kr_dictionary_parse_utf8(ABI, null(), 0, null_mut()), NULL);
            for abi in [ABI - 1, ABI + 1, 0x0001_0000] {
                assert_eq!(
                    kr_dictionary_parse_utf8(abi, unreadable, 1, &mut out),
                    VERSION
                );
                assert!(out.is_null());
            }
            assert_eq!(kr_dictionary_parse_utf8(ABI, null(), 1, &mut out), NULL);
            assert!(out.is_null());
            for source in [null(), unreadable] {
                out = std::ptr::NonNull::<Dictionary>::dangling().as_ptr();
                assert_eq!(
                    kr_dictionary_parse_utf8(ABI, source, MAX_SOURCE_BYTES as u32 + 1, &mut out),
                    BOUNDS
                );
                assert!(out.is_null());
            }
            for source in [b"word\0other".as_slice(), b"/encoding=latin1\nword"] {
                assert_eq!(
                    kr_dictionary_parse_utf8(ABI, source.as_ptr(), source.len() as u32, &mut out),
                    DICTIONARY_DATA
                );
                assert!(out.is_null());
            }
            assert_eq!(kr_dictionary_parse_utf8(ABI, null(), 0, &mut out), OK);
            assert!((*out).vocabulary.is_empty());
            kr_dictionary_free(out);
            kr_dictionary_free(null_mut());
        }
    }

    #[test]
    fn shared_parser_flags_skips_and_owned_bytes_survive_failed_candidate() {
        // SAFETY: the unique engine and immutable handles stay live throughout each call;
        // temporary shared borrows end before freeing handles or modifying their source bytes.
        unsafe {
            let mut source = "\u{feff}/encoding=utf-8\r\n/regions=usgb\nkeep\nNeovim/=\nrare/?\nno/!\nno\nbad/*\n/future\n".as_bytes().to_vec();
            source.extend_from_slice(b"\xff\n");
            let dictionary = parse(&source);
            source.fill(b'x');
            let mut engine = null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_adopt_dictionaries(engine, dictionary, null()), OK);
            let old = (*engine).dictionaries[0].current().unwrap();
            for (word, membership) in [
                ("KEEP", Membership::Recognized),
                ("Neovim", Membership::Recognized),
                ("neovim", Membership::Absent),
                ("rare", Membership::Rare),
                ("no", Membership::Forbidden),
            ] {
                assert_eq!(old.lookup(&units(word)), membership);
            }
            let mut skipped = 77;
            assert_eq!(kr_dictionary_skipped_lines(engine, 1, &mut skipped), OK);
            assert_eq!(skipped, 3);
            let mut failed = null_mut();
            assert_eq!(
                kr_dictionary_parse_utf8(ABI, [0].as_ptr(), 1, &mut failed),
                DICTIONARY_DATA
            );
            assert!(failed.is_null());
            assert!(Arc::ptr_eq(
                &old,
                &(*engine).dictionaries[0].current().unwrap()
            ));
            kr_dictionary_free(dictionary);
            assert_eq!(old.lookup(&units("keep")), Membership::Recognized);
            kr_engine_free(engine);
            assert_eq!(old.lookup(&units("keep")), Membership::Recognized);
        }
    }

    #[test]
    fn adoption_is_pairwise_missing_differs_from_empty_and_pending_token_survives() {
        // SAFETY: all handles are uniquely owned and live, and the event/output objects are
        // distinct locals. Pending adoption is required to reject even without reading handles.
        unsafe {
            let empty = parse(b"");
            let replacement = parse(b"replacement");
            let mut engine = null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_adopt_dictionaries(null_mut(), empty, empty), NULL);
            assert_eq!(kr_adopt_dictionaries(engine, empty, null()), OK);
            let mut skipped = 77;
            assert_eq!(kr_dictionary_skipped_lines(engine, 1, &mut skipped), OK);
            assert_eq!(skipped, 0);
            skipped = 77;
            assert_eq!(kr_dictionary_skipped_lines(engine, 2, &mut skipped), STATE);
            assert_eq!(skipped, 77);
            let old = (*engine).dictionaries[0].current().unwrap();
            let mut letter = event(1);
            letter.len = 1;
            letter.units[0] = b'a' as u16;
            let mut out = std::mem::MaybeUninit::uninit();
            assert_eq!(kr_plan_event(engine, &letter, out.as_mut_ptr()), OK);
            let pending = out.assume_init();
            assert_eq!(
                kr_adopt_dictionaries(engine, replacement, replacement),
                STATE
            );
            assert!(Arc::ptr_eq(
                &old,
                &(*engine).dictionaries[0].current().unwrap()
            ));
            assert!((*engine).dictionaries[1].current().is_none());
            assert_eq!(kr_commit(engine, pending.plan_id, 0), OK);
            assert_eq!(kr_adopt_dictionaries(engine, replacement, replacement), OK);
            assert!(Arc::ptr_eq(
                &(*engine).dictionaries[0].current().unwrap(),
                &(*engine).dictionaries[1].current().unwrap()
            ));
            assert_mark(&apply_event(engine, &event(6)), "a");
            assert_eq!(kr_adopt_dictionaries(engine, null(), null()), OK);
            assert!(
                (*engine)
                    .dictionaries
                    .iter()
                    .all(|source| source.current().is_none())
            );
            kr_dictionary_free(empty);
            kr_dictionary_free(replacement);
            kr_engine_free(engine);
        }
    }

    #[test]
    fn immutable_handoff_and_old_readers_outlive_independent_engines() {
        // SAFETY: the test owns all allocations. Scoped workers borrow the dictionary until
        // join, own their engines, and drain before the dictionary is freed.
        unsafe {
            let dictionary = parse(b"original");
            let replacement = parse(b"replacement");
            let retained = (*dictionary).vocabulary.clone();
            let shared = &*dictionary;
            std::thread::scope(|scope| {
                for _ in 0..2 {
                    scope.spawn(move || {
                        let mut engine = null_mut();
                        assert_eq!(kr_engine_new(ABI, &mut engine), OK);
                        assert_eq!(kr_adopt_dictionaries(engine, shared, shared), OK);
                        let snapshot = (*engine).dictionaries[0].current().unwrap();
                        assert_eq!(snapshot.lookup(&units("original")), Membership::Recognized);
                        kr_engine_free(engine);
                        assert_eq!(snapshot.lookup(&units("original")), Membership::Recognized);
                    });
                }
            });
            let mut first = null_mut();
            let mut second = null_mut();
            assert_eq!(kr_engine_new(ABI, &mut first), OK);
            assert_eq!(kr_engine_new(ABI, &mut second), OK);
            assert_eq!(kr_adopt_dictionaries(first, dictionary, dictionary), OK);
            assert_eq!(kr_adopt_dictionaries(second, dictionary, dictionary), OK);
            kr_dictionary_free(dictionary);
            assert_eq!(kr_adopt_dictionaries(first, replacement, replacement), OK);
            kr_dictionary_free(replacement);
            kr_engine_free(first);
            assert_eq!(
                (*second).dictionaries[0]
                    .current()
                    .unwrap()
                    .lookup(&units("original")),
                Membership::Recognized
            );
            kr_engine_free(second);
            assert_eq!(retained.lookup(&units("original")), Membership::Recognized);
        }
    }

    #[test]
    fn compatibility_proposal_also_blocks_adoption_without_changing_its_token() {
        // SAFETY: the engine, dictionaries and copied proposal are distinct live objects.
        // Compatibility acknowledgement submits no native effects in this portable test.
        unsafe {
            let empty = parse(b"");
            let mut engine = null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            let mut letter = event(1);
            letter.len = 1;
            letter.units[0] = b'a' as u16;
            let mut out = std::mem::MaybeUninit::uninit();
            assert_eq!(
                crate::compatibility::kr_plan_compatibility_event(
                    engine,
                    &letter,
                    0,
                    out.as_mut_ptr()
                ),
                OK
            );
            let pending = out.assume_init();
            assert_eq!(kr_adopt_dictionaries(engine, empty, empty), STATE);
            assert!(
                (*engine)
                    .dictionaries
                    .iter()
                    .all(|source| source.current().is_none())
            );
            assert_eq!(
                crate::compatibility::kr_commit_compatibility(engine, pending.plan_id, 0),
                OK
            );
            assert_eq!(kr_adopt_dictionaries(engine, empty, empty), OK);
            kr_dictionary_free(empty);
            kr_engine_free(engine);
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn adoption_preserves_model_history_manual_intent_and_reversal_receipts() {
        // SAFETY: engines, handles and event objects are live distinct allocations. Every
        // proposal is acknowledged before adoption; all handles are freed exactly once.
        unsafe {
            let empty = parse(b"");
            let protected = parse(b"cgbcjr");
            let mut engine = null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            assert_eq!(kr_prepare_model(ABI), OK);
            assert_eq!(kr_set_automatic_policy(engine, 2), OK);
            assert_eq!(kr_adopt_dictionaries(engine, empty, null()), OK);
            assert_mark(&type_text(engine, 1, "cgbcjr "), "cgbcjr ");
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(kr_adopt_dictionaries(engine, empty, empty), OK);
            assert_mark(&type_text(engine, 1, "cgbcjr "), "список ");
            assert_eq!(kr_adopt_dictionaries(engine, protected, empty), OK);
            assert_mark(&apply_event(engine, &event(5)), "cgbcjr ");
            assert_eq!(kr_reset(engine), OK);
            assert_mark(&type_text(engine, 1, "cgbcjr "), "cgbcjr ");
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(kr_adopt_dictionaries(engine, empty, empty), OK);
            type_text(engine, 1, "hello world ");
            let mut next = event(1);
            next.len = 1;
            next.units[0] = b'y' as u16;
            let mut out = std::mem::MaybeUninit::uninit();
            assert_eq!(kr_plan_event(engine, &next, out.as_mut_ptr()), OK);
            let finish = out.assume_init();
            assert_eq!((finish.action, finish.disposition), (2, 2));
            assert_eq!(kr_commit(engine, finish.plan_id, 0), OK);
            assert_eq!(kr_adopt_dictionaries(engine, empty, empty), OK);
            assert_mark(&type_text(engine, 1, "yfv "), "yfv ");
            assert_eq!(kr_reset(engine), OK);
            assert_eq!(kr_set_automatic_policy(engine, 0), OK);
            assert_eq!(kr_set_keyboard(engine, 1), OK);
            type_text(engine, 1, "`krf");
            assert_mark(&apply_event(engine, &event(4)), "ёлка");
            assert_eq!(kr_adopt_dictionaries(engine, null(), null()), OK);
            assert_mark(&type_text(engine, 1, " "), "ёлка ");
            assert_mark(&apply_event(engine, &event(5)), "`krf ");
            kr_dictionary_free(empty);
            kr_dictionary_free(protected);
            kr_engine_free(engine);
        }
    }

    #[test]
    fn panic_is_contained_without_publishing_or_reviving_engine_authority() {
        // SAFETY: the engine and output are distinct valid locals. The injected parser panic
        // is caught before any allocation is transferred; the engine is never shared.
        unsafe {
            let mut engine = null_mut();
            assert_eq!(kr_engine_new(ABI, &mut engine), OK);
            let mut dictionary = std::ptr::NonNull::<Dictionary>::dangling().as_ptr();
            assert_eq!(
                parse_with(ABI, null(), 0, &mut dictionary, |_| panic!(
                    "parser containment"
                )),
                POISONED
            );
            assert!(dictionary.is_null());
            assert_eq!(kr_reset(engine), OK);
            let empty = parse(b"");
            let mut out = std::mem::MaybeUninit::uninit();
            let reverse = event(5);
            assert_eq!(kr_plan_event(engine, &reverse, out.as_mut_ptr()), OK);
            let pending = out.assume_init_read();
            assert_eq!(kr_commit(engine, pending.plan_id, 2), OK);
            assert_eq!(kr_adopt_dictionaries(engine, empty, empty), OK);
            assert_eq!(kr_plan_event(engine, &reverse, out.as_mut_ptr()), STATE);
            assert_eq!(with_engine(engine, |_| panic!("adoption poison")), POISONED);
            assert_eq!(kr_adopt_dictionaries(engine, empty, empty), POISONED);
            kr_dictionary_free(empty);
            kr_engine_free(engine);
        }
    }

    #[test]
    fn model_preparation_rejects_versions_and_contains_failures() {
        assert_eq!(kr_prepare_model(ABI + 1), VERSION);
        assert_eq!(
            prepare_with(ABI + 1, || panic!("must not prepare")),
            VERSION
        );
        assert_eq!(prepare_with(ABI, || Err(ModelError::Magic)), MODEL_ASSET);
        assert_eq!(prepare_with(ABI, || panic!("model containment")), POISONED);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn model_preparation_is_idempotent() {
        assert_eq!(kr_prepare_model(ABI), OK);
        assert_eq!(
            std::thread::spawn(|| kr_prepare_model(ABI)).join().unwrap(),
            OK
        );
        assert_eq!(kr_prepare_model(ABI), OK);
    }
}
