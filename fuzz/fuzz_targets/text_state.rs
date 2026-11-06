//! Vocabulary parsing, layout mapping and the word session against arbitrary bytes.
//!
//! One input checks three properties:
//!
//! - the whole input as a Vim word list: a candidate that fails to parse leaves the last good
//!   snapshot in place, still recognizing its word;
//! - the input as up to 128 UTF-16 LE units: manual conversion keeps the length, unit for unit,
//!   including unpaired surrogates;
//! - each of up to 256 bytes as one event with an outcome: no schedule of events, refusals and
//!   unknown effects panics, and the newest plan can always be acknowledged.
//!
//! ```text
//!   byte 0, bit 0   context for the whole run: 0 BOUNDARY, 1 INELIGIBLE
//!   each byte b     bit 7: mode (0 EN, 1 RU)
//!                   b % 8: 0 Manual, 1 Reverse, 2 Backspace, 3 Escape, 4 Finish, 5 Space,
//!                          6 and 7 the text unit b
//!                   b % 3: 0 Verified, 1 Refused, 2 Unknown
//! ```
#![no_main]

use keyrill_core::mapping::{self, Keyboard, Mapping};
use keyrill_core::session::{Context, Event, Mode, Outcome, Session};
use keyrill_core::vocabulary::{Membership, Snapshot};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut vocabulary = Snapshot::default();
    vocabulary.replace(b"original").unwrap();
    if vocabulary.replace(bytes).is_err() {
        assert_eq!(
            vocabulary
                .current()
                .unwrap()
                .lookup(&"original".encode_utf16().collect::<Vec<_>>()),
            Membership::Recognized
        );
    }
    let units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .take(128)
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    assert_eq!(
        mapping::convert(&units, Keyboard::Apple, Mapping::Manual).len(),
        units.len()
    );
    let mut session = Session::default();
    let context = if bytes.first().copied().unwrap_or(0) & 1 == 0 {
        Context::Boundary
    } else {
        Context::Ineligible
    };
    for &code in bytes.iter().take(256) {
        let mode = if code & 0x80 == 0 { Mode::En } else { Mode::Ru };
        let event = match code % 8 {
            0 => Event::Manual,
            1 => Event::Reverse,
            2 => Event::Backspace,
            3 => Event::Escape,
            4 => Event::Finish,
            5 => Event::Text(vec![32]),
            _ => Event::Text(vec![u16::from(code)]),
        };
        if let Ok(plan) = session.propose(&event, mode, context) {
            let outcome = match code % 3 {
                0 => Outcome::Verified,
                1 => Outcome::Refused,
                _ => Outcome::Unknown,
            };
            session.acknowledge(plan.id, outcome).unwrap();
        } else {
            // An invalidated session refuses every plan until the adapter resets it
            session.reset();
        }
    }
});
