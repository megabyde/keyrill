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
            session.reset();
        }
    }
});
