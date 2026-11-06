//! Model loader against bounded corruptions of the real asset.
//!
//! Random bytes almost never pass the magic and header checks, so the input is a list of
//! patches to the shipped `model.bin` and every case reaches the deeper validation:
//!
//! ```text
//!   byte 0        flags: bit 0 truncates, bit 1 appends one zero byte
//!   bytes 1..5    truncation length, u32 LE, taken modulo the asset length plus one
//!   then          up to 64 patches of 5 bytes: offset u32 LE (modulo the length), new byte
//! ```
//!
//! Patches apply before truncation and the appended byte. The property: `Model::parse` never
//! panics, and any model it accepts answers finite frequency and log-likelihood values, so a
//! corrupted table cannot feed NaN or infinity into the ranking.
#![no_main]

use keyrill_core::{model::Model, session::Mode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut image = include_bytes!("../../assets/model.bin").to_vec();
    let flags = bytes.first().copied().unwrap_or(0);
    let truncation = bytes
        .get(1..5)
        .map(|n| u32::from_le_bytes(n.try_into().unwrap()) as usize);
    for patch in bytes
        .get(5..)
        .unwrap_or_default()
        .as_chunks::<5>()
        .0
        .iter()
        .take(64)
    {
        let at = u32::from_le_bytes(patch[..4].try_into().unwrap()) as usize % image.len();
        image[at] = patch[4];
    }
    if flags & 1 != 0 {
        image.truncate(truncation.unwrap_or(0) % (image.len() + 1));
    }
    if flags & 2 != 0 {
        image.push(0);
    }
    if let Ok(model) = Model::parse(&image) {
        for mode in [Mode::En, Mode::Ru] {
            // A Latin letter, ё, a lone surrogate and a 33-unit word
            let words: [&[u16]; 4] = [&[97], &[0x451], &[0xd800], &[97; 33]];
            for word in words {
                let language = model.language(mode);
                assert!(language.frequency(word).is_none_or(f64::is_finite));
                assert!(language.log_likelihood(word).is_none_or(f64::is_finite));
            }
        }
    }
});
