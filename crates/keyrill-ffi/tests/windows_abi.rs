//! Independent Windows caller of the published C ABI and its original smoke scenarios.
#![cfg(windows)]

use keyrill_ffi as _;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

const ABI: u32 = 3;
const OK: u32 = 0;
const TEXT: u32 = 1;
const ESCAPE: u32 = 3;
const MANUAL: u32 = 4;
const REVERSE: u32 = 5;
const FINISH_REQUEST: u32 = 6;
const PASS: u32 = 0;
const MARK: u32 = 1;
const FINISH: u32 = 2;
const UNAVAILABLE: u32 = 3;
const KEY_PASS: u32 = 0;
const KEY_REPLAN: u32 = 2;

#[repr(C)]
struct Event {
    role: u32,
    mode: u32,
    context: u32,
    len: u32,
    units: [u16; 8],
}

#[repr(C)]
struct Plan {
    plan_id: u64,
    action: u32,
    disposition: u32,
    expected_len: u32,
    mark_len: u32,
    expected: [u16; 128],
    mark: [u16; 128],
}

#[repr(C)]
struct CompatibilityPlan {
    plan_id: u64,
    operation: u32,
    disposition: u32,
    delete_units: u32,
    replacement_len: u32,
    replacement: [u16; 128],
}

unsafe extern "C" {
    fn kr_abi_version() -> u32;
    fn kr_engine_new(abi: u32, out: *mut *mut c_void) -> u32;
    fn kr_engine_free(engine: *mut c_void);
    fn kr_set_automatic_policy(engine: *mut c_void, policy: u32) -> u32;
    fn kr_set_keyboard(engine: *mut c_void, keyboard: u32) -> u32;
    fn kr_prepare_model(abi: u32) -> u32;
    fn kr_dictionary_parse_utf8(abi: u32, bytes: *const u8, len: u32, out: *mut *mut c_void)
    -> u32;
    fn kr_dictionary_free(dictionary: *mut c_void);
    fn kr_adopt_dictionaries(
        engine: *mut c_void,
        english: *const c_void,
        russian: *const c_void,
    ) -> u32;
    fn kr_dictionary_skipped_lines(engine: *mut c_void, language: u32, out: *mut u32) -> u32;
    fn kr_plan_event(engine: *mut c_void, event: *const Event, out: *mut Plan) -> u32;
    fn kr_commit(engine: *mut c_void, token: u64, outcome: u32) -> u32;
}

struct Engine(*mut c_void);

impl Engine {
    fn new(policy: u32, keyboard: u32) -> Self {
        let mut pointer = null_mut();
        // SAFETY: output is aligned writable storage. Success transfers one engine handle.
        assert_eq!(unsafe { kr_engine_new(ABI, &mut pointer) }, OK);
        assert!(!pointer.is_null());
        let engine = Self(pointer);
        // SAFETY: the engine is live and serialized, and configuration precedes all events.
        assert_eq!(unsafe { kr_set_automatic_policy(engine.0, policy) }, OK);
        // SAFETY: the same live engine is idle with no pending proposal.
        assert_eq!(unsafe { kr_set_keyboard(engine.0, keyboard) }, OK);
        engine
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the live handle and every serialized call has returned.
        unsafe { kr_engine_free(self.0) };
    }
}

struct Dictionary(*mut c_void);

impl Dictionary {
    fn parse(bytes: &[u8]) -> Self {
        let mut pointer = null_mut();
        // SAFETY: input is a complete borrowed source and output is writable pointer storage.
        assert_eq!(
            unsafe {
                kr_dictionary_parse_utf8(ABI, bytes.as_ptr(), bytes.len() as u32, &mut pointer)
            },
            OK
        );
        assert!(!pointer.is_null());
        Self(pointer)
    }
}

impl Drop for Dictionary {
    fn drop(&mut self) {
        // SAFETY: no call retains a borrow of this independently owned immutable handle.
        unsafe { kr_dictionary_free(self.0) };
    }
}

#[derive(Default)]
struct Document {
    text: Vec<u16>,
    mark_len: usize,
}

impl Document {
    fn key(&mut self, engine: &Engine, role: u32, mode: u32, unit: u16) {
        let event = Event {
            role,
            mode,
            context: u32::from(self.text.last().is_some_and(|&last| last != 32)),
            len: u32::from(role == TEXT),
            units: [unit, 0, 0, 0, 0, 0, 0, 0],
        };
        for _ in 0..2 {
            let mut plan = Plan {
                plan_id: 0,
                action: 0,
                disposition: 0,
                expected_len: 0,
                mark_len: 0,
                expected: [0; 128],
                mark: [0; 128],
            };
            // SAFETY: all pointers are complete nonaliasing ABI objects on the engine owner.
            assert_eq!(unsafe { kr_plan_event(engine.0, &event, &mut plan) }, OK);
            if matches!(plan.action, MARK | FINISH) {
                assert!(plan.expected_len <= 128 && plan.mark_len <= 128);
                let start = self.text.len().checked_sub(self.mark_len).unwrap();
                assert_eq!(plan.expected_len as usize, self.mark_len);
                assert_eq!(&plan.expected[..self.mark_len], &self.text[start..]);
                self.text.truncate(start);
                self.text
                    .extend_from_slice(&plan.mark[..plan.mark_len as usize]);
                self.mark_len = if plan.action == MARK {
                    plan.mark_len as usize
                } else {
                    0
                };
            } else {
                assert!(matches!(plan.action, PASS | UNAVAILABLE));
            }
            // SAFETY: this is the exact pending token, acknowledged after the document check.
            assert_eq!(unsafe { kr_commit(engine.0, plan.plan_id, 0) }, OK);
            if plan.action == FINISH && plan.disposition == KEY_REPLAN {
                continue;
            }
            if plan.disposition == KEY_PASS && role == TEXT {
                assert_eq!(self.mark_len, 0);
                self.text.push(unit);
            }
            return;
        }
        panic!("second replan violated the ABI protocol");
    }

    fn type_text(&mut self, engine: &Engine, mode: u32, text: &str) {
        for unit in text.encode_utf16() {
            self.key(engine, TEXT, mode, unit);
        }
    }

    fn expect(&self, text: &str, committed: bool) {
        assert_eq!(self.text, text.encode_utf16().collect::<Vec<_>>());
        if committed {
            assert_eq!(self.mark_len, 0);
        }
    }
}

#[test]
fn published_c_layouts_and_version_remain_exact() {
    assert_eq!(size_of::<Event>(), 32);
    assert_eq!(align_of::<Event>(), 4);
    assert_eq!(size_of::<Plan>(), 536);
    assert_eq!(align_of::<Plan>(), 8);
    assert_eq!(std::mem::offset_of!(Plan, expected), 24);
    assert_eq!(size_of::<CompatibilityPlan>(), 280);
    assert_eq!(align_of::<CompatibilityPlan>(), 8);
    // SAFETY: the version query takes no pointers or mutable state.
    assert_eq!(unsafe { kr_abi_version() }, ABI);
    let mut rejected = null_mut();
    // SAFETY: the output points to complete writable storage, even for the rejected version.
    assert_eq!(unsafe { kr_engine_new(ABI + 1, &mut rejected) }, 7);
    assert!(rejected.is_null());
}

#[test]
fn all_eight_original_c_abi_editing_scenarios() {
    for (keyboard, mode, typed, shown, last, expected) in [
        (0, 1, "ghbdtn ", "привет ", FINISH_REQUEST, "привет "),
        (0, 2, "руддщ ", "hello ", FINISH_REQUEST, "hello "),
        (0, 1, "ghbdtn ", "привет ", ESCAPE, "ghbdtn "),
        (0, 1, "test ", "test ", FINISH_REQUEST, "test "),
        (0, 2, "ёлка", "ёлка", MANUAL, "\\krf"),
        (1, 2, "ёлка", "ёлка", MANUAL, "`krf"),
        (1, 1, "`krf", "`krf", MANUAL, "ёлка"),
        (0, 1, "[jhjij", "[jhjij", MANUAL, "хорошо"),
    ] {
        let engine = Engine::new(1, keyboard);
        let mut document = Document::default();
        document.type_text(&engine, mode, typed);
        document.expect(shown, false);
        document.key(&engine, last, mode, 0);
        if document.mark_len != 0 {
            document.key(&engine, FINISH_REQUEST, mode, 0);
        }
        document.expect(expected, true);
    }
}

#[test]
fn immutable_dictionary_ownership_preserves_model_and_reversal() {
    // SAFETY: model preparation requires only the literal published ABI version.
    assert_eq!(unsafe { kr_prepare_model(ABI) }, OK);
    let mut empty = null_mut();
    // SAFETY: NULL plus zero is a valid empty source and output owns writable storage.
    assert_eq!(
        unsafe { kr_dictionary_parse_utf8(ABI, null(), 0, &mut empty) },
        OK
    );
    assert!(!empty.is_null());
    let empty = Dictionary(empty);
    let protected = Dictionary::parse(b"cgbcjr\nbad/*\n");
    let engine = Engine::new(2, 0);
    // SAFETY: all three handles are live, and adoption retains independent dictionary owners.
    assert_eq!(
        unsafe { kr_adopt_dictionaries(engine.0, empty.0, empty.0) },
        OK
    );
    let mut document = Document::default();
    document.type_text(&engine, 1, "cgbcjr ");
    document.expect("список ", false);
    // SAFETY: the engine has acknowledged ownership and no proposal is pending.
    assert_eq!(
        unsafe { kr_adopt_dictionaries(engine.0, protected.0, empty.0) },
        OK
    );
    drop(empty);
    drop(protected);
    let mut skipped = 77;
    // SAFETY: the engine retains adopted snapshots, and output is writable u32 storage.
    assert_eq!(
        unsafe { kr_dictionary_skipped_lines(engine.0, 1, &mut skipped) },
        OK
    );
    assert_eq!(skipped, 1);
    document.key(&engine, REVERSE, 1, 0);
    document.expect("cgbcjr ", false);
    document.key(&engine, FINISH_REQUEST, 1, 0);
    document.type_text(&engine, 1, "cgbcjr ");
    document.expect("cgbcjr cgbcjr ", false);
}
