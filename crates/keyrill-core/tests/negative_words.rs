//! Correctly typed tokens that automatic correction must leave unchanged. This is a regression
//! set, not a held-out precision estimate; see `fixtures/negative-words.md` for scope and
//! provenance.

use keyrill_core::inference::{self, Dictionaries, Policy, Reason, Request};
use keyrill_core::mapping::Keyboard;
use keyrill_core::model::Model;
use keyrill_core::session::Mode;
use keyrill_core::vocabulary::Vocabulary;
use std::collections::{BTreeMap, BTreeSet};

const CATEGORIES: [&str; 17] = [
    "en_technical",
    "en_names",
    "en_acronyms",
    "en_short",
    "ru_prose",
    "ru_slang",
    "ru_names",
    "ru_short",
    "ru_acronyms",
    "en_in_ru_context",
    "ru_in_en_context",
    "en_in_en_context",
    "ru_in_ru_context",
    "non_letter",
    "en_punctuated",
    "en_trailing_collision",
    "cross_layout_collision",
];

fn mode(tag: &str) -> Mode {
    match tag {
        "en" => Mode::En,
        "ru" => Mode::Ru,
        other => panic!("unknown mode {other}"),
    }
}

fn utf16(value: &serde_json::Value) -> Vec<u16> {
    value.as_str().unwrap().encode_utf16().collect()
}

#[test]
fn negative_words_are_not_changed() {
    let model = Model::embedded().unwrap();
    // Empty but present overlays: protection comes from the frozen model alone
    let empty = Vocabulary::parse(b"").unwrap();
    let dictionaries = Dictionaries {
        en: Some(&empty),
        ru: Some(&empty),
    };
    let mut ids = BTreeSet::new();
    let mut per_category = BTreeMap::new();
    let mut unexpected = Vec::new();
    for line in include_str!("fixtures/negative-words.jsonl").lines() {
        let case: serde_json::Value = serde_json::from_str(line).unwrap();
        let id = case["id"].as_str().unwrap();
        assert!(ids.insert(id.to_owned()), "duplicate id {id}");
        let category = case["category"].as_str().unwrap();
        let category = CATEGORIES
            .iter()
            .find(|&&known| known == category)
            .unwrap_or_else(|| panic!("unknown category in {id}"));
        *per_category.entry(*category).or_insert(0) += 1;
        let word = utf16(&case["word"]);
        assert!(!word.is_empty(), "{id}");
        let prefix = utf16(&case["context"]);
        // A context case whose prefix is not recognized would silently test nothing
        assert_eq!(
            inference::detect_context(&model, &prefix),
            case["context_mode"].as_str().map(mode),
            "context of {id}"
        );
        assert_eq!(case["expect"].as_str().unwrap(), "unchanged", "{id}");
        // Session-owned context can be unavailable whatever the document holds, so every
        // negative must hold without context and under either language, not only its prefix
        for context in [None, Some(Mode::En), Some(Mode::Ru)] {
            for keyboard in [Keyboard::Apple, Keyboard::Pc] {
                let decision = inference::infer(
                    &model,
                    dictionaries,
                    Policy::default(),
                    Request {
                        word: &word,
                        active: Some(mode(case["active"].as_str().unwrap())),
                        context,
                        keyboard,
                        explicit_intent: false,
                    },
                );
                let changed =
                    decision.reason == Reason::Correction || decision.replacement.is_some();
                if changed {
                    unexpected.push(format!(
                        "{id} ({context:?}, {keyboard:?}): {:?}",
                        decision.reason
                    ));
                }
            }
        }
    }
    assert!(unexpected.is_empty(), "unexpected outcomes: {unexpected:?}");
    assert_eq!(ids.len(), 766);
    for category in CATEGORIES {
        assert!(
            per_category.get(category).copied().unwrap_or(0) >= 4,
            "too few cases in {category}"
        );
    }
}
