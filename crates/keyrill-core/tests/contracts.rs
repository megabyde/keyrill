//! Reference vectors and reproducible UTF-16/session properties.
//!
//! `fixtures/engine-vectors.jsonl` pins the output of the mapping, the vocabulary parser,
//! context detection and inference for fixed requests (the protocol is in `docs/engine.md`). A
//! change in behavior, or a score that moves beyond the tolerance, fails here, and each test also
//! counts its vectors so that a fixture which lost lines cannot pass by testing less. Expected
//! values are not edited to make a failure pass.

use keyrill_core::inference::{self, Dictionaries, Policy, Request};
use keyrill_core::mapping::{self, Keyboard, Mapping};
use keyrill_core::model::Model;
use keyrill_core::session::{Action, Context, Event, MAX_WORD, Mode, Outcome, Session};
use keyrill_core::vocabulary::{Membership, Vocabulary};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};

#[test]
fn reference_mapping_vectors() {
    let mut checked = 0;
    for line in include_str!("fixtures/engine-vectors.jsonl").lines() {
        let vector: serde_json::Value = serde_json::from_str(line).unwrap();
        let request = vector["request"].as_str().unwrap();
        let parts: Vec<_> = request.split_whitespace().collect();
        if parts[0] != "map" {
            continue;
        }
        let keyboard = match parts[1] {
            "0" => Keyboard::Apple,
            "1" => Keyboard::Pc,
            _ => unreachable!(),
        };
        let mode = match parts[2] {
            "0" => Mapping::Manual,
            "1" => Mapping::EnToRu,
            "2" => Mapping::RuToEn,
            _ => unreachable!(),
        };
        let decode = |hex: &str| -> Vec<u16> {
            if hex == "-" {
                return Vec::new();
            }
            (0..hex.len())
                .step_by(4)
                .map(|i| u16::from_str_radix(&hex[i..i + 4], 16).unwrap())
                .collect()
        };
        let expected = decode(vector["expected"].as_str().unwrap());
        assert_eq!(
            mapping::convert(&decode(parts[3]), keyboard, mode),
            expected,
            "{request}"
        );
        checked += 1;
    }
    assert_eq!(checked, 90);
}

#[test]
fn reference_vocabulary_vectors() {
    let mut checked = 0;
    for line in include_str!("fixtures/engine-vectors.jsonl").lines() {
        let vector: serde_json::Value = serde_json::from_str(line).unwrap();
        let request = vector["request"].as_str().unwrap();
        let parts: Vec<_> = request.split_whitespace().collect();
        if parts[0] != "vocab" {
            continue;
        }
        let decode = |hex: &str| -> Vec<u16> {
            if hex == "-" {
                return Vec::new();
            }
            (0..hex.len())
                .step_by(4)
                .map(|i| u16::from_str_radix(&hex[i..i + 4], 16).unwrap())
                .collect()
        };
        let source = String::from_utf16(&decode(parts[1])).unwrap();
        let result = match Vocabulary::parse(source.as_bytes()) {
            Ok(v) => {
                let tag = match v.lookup(&decode(parts[2])) {
                    Membership::Absent => 0,
                    Membership::Recognized => 1,
                    Membership::Rare => 2,
                    Membership::Forbidden => 3,
                };
                format!("1\t{}\t{tag}", v.len())
            }
            Err(_) => "0\t0\t0".to_owned(),
        };
        assert_eq!(result, vector["expected"].as_str().unwrap(), "{request}");
        checked += 1;
    }
    assert_eq!(checked, 120);
}

#[test]
fn reference_context_and_inference_vectors() {
    let model = Model::embedded().unwrap();
    let decode = |hex: &str| -> Vec<u16> {
        if hex == "-" {
            return Vec::new();
        }
        (0..hex.len())
            .step_by(4)
            .map(|i| u16::from_str_radix(&hex[i..i + 4], 16).unwrap())
            .collect()
    };
    let mode = |tag: &str| match tag {
        "1" => Some(Mode::En),
        "2" => Some(Mode::Ru),
        "0" => None,
        _ => unreachable!(),
    };
    let mut checked_context = 0;
    let mut checked_inference = 0;
    let mut max_score_difference = 0.0_f64;
    for line in include_str!("fixtures/engine-vectors.jsonl").lines() {
        let vector: serde_json::Value = serde_json::from_str(line).unwrap();
        let request = vector["request"].as_str().unwrap();
        let parts: Vec<_> = request.split_whitespace().collect();
        if parts[0] == "context" {
            let context = inference::detect_context(&model, &decode(parts[1]));
            assert_eq!(
                context,
                mode(vector["expected"].as_str().unwrap()),
                "{request}"
            );
            checked_context += 1;
        } else if parts[0] == "infer" {
            let en = Vocabulary::parse(String::from_utf16(&decode(parts[7])).unwrap().as_bytes())
                .unwrap();
            let ru = Vocabulary::parse(String::from_utf16(&decode(parts[8])).unwrap().as_bytes())
                .unwrap();
            let policy = Policy {
                error_prior: parts[4].parse().unwrap(),
                false_correction_cost: parts[5].parse().unwrap(),
                minimum_zipf: parts[6].parse().unwrap(),
                ..Policy::default()
            };
            let word = decode(parts[9]);
            let decision = inference::infer(
                &model,
                Dictionaries {
                    en: Some(&en),
                    ru: Some(&ru),
                    learned: &[],
                },
                policy,
                Request {
                    word: &word,
                    active: mode(parts[2]),
                    context: mode(parts[3]),
                    keyboard: if parts[1] == "0" {
                        Keyboard::Apple
                    } else {
                        Keyboard::Pc
                    },
                    explicit_intent: false,
                },
            );
            let expected: Vec<_> = vector["expected"].as_str().unwrap().split('\t').collect();
            assert_eq!(
                decision.reason as u8,
                expected[0].parse::<u8>().unwrap(),
                "{request}"
            );
            let replacement = decision.replacement.as_ref().map_or_else(
                || "-".to_owned(),
                |text| text.iter().map(|u| format!("{u:04x}")).collect(),
            );
            assert_eq!(replacement, expected[2], "{request}");
            let expected_score: f64 = expected[1].parse().unwrap();
            // Absolute 1e-10 plus relative 1e-12
            let difference = (decision.score - expected_score).abs();
            assert!(
                difference <= 1e-10 + 1e-12 * expected_score.abs(),
                "score {request}: {difference}"
            );
            max_score_difference = max_score_difference.max(difference);
            checked_inference += 1;
        }
    }
    assert_eq!((checked_context, checked_inference), (7, 1000));
    eprintln!("inference vectors: maximum absolute score difference {max_score_difference}");
}

proptest! {
    #![proptest_config(Config { cases: 512, rng_seed: RngSeed::Fixed(0x4b455952494c4c), ..Config::default() })]

    #[test]
    fn raw_utf16_mapping_preserves_length_and_unsupported_units(units in proptest::collection::vec(any::<u16>(), 0..=128)) {
        let converted = mapping::convert(&units, Keyboard::Apple, Mapping::Manual);
        prop_assert_eq!(converted.len(), units.len());
        for (before, after) in units.into_iter().zip(converted) {
            if !mapping::is_letter(before) { prop_assert_eq!(before, after); }
        }
    }

    // Ids grow strictly across resets and invalidation, so a late acknowledgement of an expired
    // plan can never match a later one; plans stay within one word and its Space. Codes divisible
    // by 3 reset before acknowledging, code 7 reports Unknown, and a failed proposal resets
    #[test]
    fn arbitrary_event_schedules_never_resurrect_a_reset_token(events in proptest::collection::vec(0u8..12, 0..=256)) {
        let mut session = Session::default();
        session.set_fixtures(true).unwrap();
        let mut previous = 0;
        for code in events {
            let event = match code {
                0 => Event::Manual, 1 => Event::Reverse, 2 => Event::Backspace,
                3 => Event::Escape, 4 => Event::Finish, 5 => Event::Text(vec![32]),
                6 => Event::Text(vec![0xd800]), 7 => Event::Text(vec![0x301]),
                _ => Event::Text(vec![97]),
            };
            if let Ok(plan) = session.propose(&event, Mode::En, Context::Boundary) {
                prop_assert!(plan.id > previous);
                prop_assert!(plan.text.len() <= MAX_WORD + 1);
                prop_assert!(plan.expected.len() <= MAX_WORD + 1);
                if plan.action == Action::Mark {
                    prop_assert!(plan.text.iter().all(|&u| mapping::is_letter(u) || u == 32 || b"[];',./`\\{}:\"<>~|".contains(&(u as u8))), "owned units remain in the defined map");
                }
                previous = plan.id;
                if code % 3 == 0 {
                    session.reset();
                    prop_assert!(session.acknowledge(plan.id, Outcome::Verified).is_err());
                } else {
                    session.acknowledge(plan.id, if code == 7 { Outcome::Unknown } else { Outcome::Verified }).unwrap();
                }
            } else {
                session.reset();
            }
        }
    }
}
