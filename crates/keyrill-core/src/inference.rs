//! Conservative reference ranking policy, independent of native editing authority.

use crate::mapping::{self, Keyboard, Mapping};
use crate::model::{MAX_MODEL_WORD, Model};
use crate::session::Mode;
use crate::vocabulary::{self, Membership, Vocabulary};

/// Reference scoring parameters. These are ranking hypotheses, not calibrated probabilities.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// Assumed layout-error prior, strictly between zero and one.
    pub error_prior: f64,
    /// Cost of a false automatic change, positive and finite.
    pub false_correction_cost: f64,
    /// Minimum candidate corpus Zipf score, finite and in 0..=9.
    pub minimum_zipf: f64,
    /// A known source word yields to the other reading only below this Zipf score, in 0..=9.
    pub known_ceiling: f64,
    /// Zipf margin by which the other reading must exceed a yielding known word, in 0..=9.
    pub known_margin: f64,
    /// Margin for a two-letter known word without target-language context, in 0..=9.
    pub short_known_margin: f64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            error_prior: 0.001,
            false_correction_cost: 100.0,
            minimum_zipf: 3.0,
            known_ceiling: 3.0,
            known_margin: 2.5,
            short_known_margin: 3.5,
        }
    }
}

impl Policy {
    /// Whether all numeric policy values are finite and in their supported domains.
    pub fn is_valid(&self) -> bool {
        self.error_prior.is_finite()
            && self.error_prior > 0.0
            && self.error_prior < 1.0
            && self.false_correction_cost.is_finite()
            && self.false_correction_cost > 0.0
            && [
                self.minimum_zipf,
                self.known_ceiling,
                self.known_margin,
                self.short_known_margin,
            ]
            .iter()
            .all(|value| (0.0..=9.0).contains(value))
    }
}

/// Ready protection sources. `None` means missing; an explicitly empty vocabulary is ready.
#[derive(Clone, Copy, Default)]
pub struct Dictionaries<'a> {
    /// English personal source.
    pub en: Option<&'a Vocabulary>,
    /// Russian personal source.
    pub ru: Option<&'a Vocabulary>,
}

/// One bounded word supplied by a caller that independently validated token eligibility.
#[derive(Clone, Copy)]
pub struct Request<'a> {
    /// Complete word, without delimiter or normalization.
    pub word: &'a [u16],
    /// Active custom input mode; unknown modes are ineligible.
    pub active: Option<Mode>,
    /// Bounded prior language evidence, if available.
    pub context: Option<Mode>,
    /// Defined physical map variant.
    pub keyboard: Keyboard,
    /// An explicit manual/reversal choice forbids reinference at this boundary.
    pub explicit_intent: bool,
}

/// Decision reasons, numbered as in the reference vectors and the probe output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Reason {
    /// Candidate passed the reference ranking threshold.
    Correction = 0,
    /// Token, mode, case or policy is ineligible.
    Ineligible = 1,
    /// Personal original-word protection.
    Personal = 2,
    /// Original is a retained general lexical word.
    Known = 3,
    /// Mapped candidate is forbidden by a personal declaration.
    Forbidden = 4,
    /// Candidate is absent, too weak, or below the length-adjusted threshold.
    Weak = 5,
    /// A configured protection source has never loaded successfully.
    DictionariesMissing = 6,
    /// Preserve explicit manual/reversal intent.
    ExplicitIntent = 7,
}

/// A correction proposal; never grants native write authority.
#[derive(Clone, Debug)]
pub struct Decision {
    /// Eligibility/protection/ranking result.
    pub reason: Reason,
    /// Reference log10 ranking score; zero when no score was evaluated.
    pub score: f64,
    /// Exact case-preserving replacement only for an accepted correction.
    pub replacement: Option<Vec<u16>>,
}

impl Decision {
    fn abstain(reason: Reason) -> Self {
        Self {
            reason,
            score: 0.0,
            replacement: None,
        }
    }
}

fn script(word: &[u16]) -> Option<Mode> {
    if word.is_empty() {
        return None;
    }
    if word.iter().all(|&u| matches!(u, 0x41..=0x5a | 0x61..=0x7a)) {
        return Some(Mode::En);
    }
    if word
        .iter()
        .all(|&u| matches!(u, 0x410..=0x44f | 0x401 | 0x451))
    {
        return Some(Mode::Ru);
    }
    None
}

// log10 of the per-word rate of punctuation from these keys before, inside and after the letters
// of English words, measured on the UD English EWT dev split
const PROVISIONAL_LOG10: [f64; 3] = [-2.3, -1.6, -1.0];

/// Whether `unit` is typed in English mode by a key that types a Russian letter: `[ ] ; ' , .`,
/// the ё key of `keyboard` and their shifted forms. In English mode such a unit can start or
/// continue a word, which keeps its typed form unless the whole word reads as Russian.
pub fn is_provisional(unit: u16, keyboard: Keyboard) -> bool {
    let yo: &[u8] = match keyboard {
        Keyboard::Apple => b"\\|",
        Keyboard::Pc => b"`~",
    };
    u8::try_from(unit).is_ok_and(|byte| b"[];',.{}:\"<>".contains(&byte) || yo.contains(&byte))
}

fn membership(dictionaries: Dictionaries<'_>, word: &[u16]) -> Membership {
    let en = dictionaries
        .en
        .map_or(Membership::Absent, |v| v.lookup(word));
    let ru = dictionaries
        .ru
        .map_or(Membership::Absent, |v| v.lookup(word));
    match (en, ru) {
        (Membership::Forbidden, _) | (_, Membership::Forbidden) => Membership::Forbidden,
        (Membership::Recognized, _) | (_, Membership::Recognized) => Membership::Recognized,
        (Membership::Rare, _) | (_, Membership::Rare) => Membership::Rare,
        _ => Membership::Absent,
    }
}

/// Apply the frozen policy to an eligible owned word; caller still controls all native effects.
///
/// ```
/// use keyrill_core::inference::{infer, Dictionaries, Policy, Reason, Request};
/// use keyrill_core::mapping::Keyboard;
/// use keyrill_core::model::Model;
/// use keyrill_core::session::Mode;
/// use keyrill_core::vocabulary::Vocabulary;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let model = Model::embedded()?;
/// let empty = Vocabulary::parse(b"")?;
/// let ready = Dictionaries { en: Some(&empty), ru: Some(&empty) };
/// let word: Vec<u16> = "ghbdtn".encode_utf16().collect();
/// let request = Request {
///     word: &word,
///     active: Some(Mode::En),
///     context: None,
///     keyboard: Keyboard::Apple,
///     explicit_intent: false,
/// };
///
/// let decision = infer(&model, ready, Policy::default(), request);
/// assert_eq!(decision.reason, Reason::Correction);
/// let replacement = decision.replacement.ok_or("no replacement")?;
/// assert_eq!(String::from_utf16(&replacement)?, "привет");
///
/// // Without both personal sources the engine abstains.
/// let missing = infer(&model, Dictionaries::default(), Policy::default(), request);
/// assert_eq!(missing.reason, Reason::DictionariesMissing);
/// # Ok(())
/// # }
/// ```
pub fn infer(
    model: &Model<'_>,
    dictionaries: Dictionaries<'_>,
    policy: Policy,
    request: Request<'_>,
) -> Decision {
    if request.explicit_intent {
        return Decision::abstain(Reason::ExplicitIntent);
    }
    let word = request.word;
    let Some(active) = request.active else {
        return Decision::abstain(Reason::Ineligible);
    };
    let keyboard = request.keyboard;
    let provisional = active == Mode::En && word.iter().any(|&unit| is_provisional(unit, keyboard));
    // Provisional units carry no English evidence: the source reading is the letters alone, and
    // only a word with them at its end alone has an English core that can be a known word
    let letters: Vec<u16> = word
        .iter()
        .copied()
        .filter(|&unit| !provisional || !is_provisional(unit, keyboard))
        .collect();
    let core = if provisional {
        let end = word
            .iter()
            .rposition(|&unit| !is_provisional(unit, keyboard))
            .map_or(0, |at| at + 1);
        &word[..end]
    } else {
        word
    };
    if word.len() > MAX_MODEL_WORD || script(&letters) != Some(active) || !policy.is_valid() {
        return Decision::abstain(Reason::Ineligible);
    }
    let (target, mapping) = match active {
        Mode::En => (Mode::Ru, Mapping::EnToRu),
        Mode::Ru => (Mode::En, Mapping::RuToEn),
    };
    let candidate = mapping::convert(word, keyboard, mapping);
    // Case is judged on the letters as they read in the target layout
    let cased = if provisional { &candidate } else { word };
    let lower = vocabulary::lowercase(cased);
    let upper = vocabulary::uppercase(cased);
    let initial_capital = cased.first() == upper.first() && cased.get(1..) == lower.get(1..);
    if (cased.len() > 1 && *cased == upper) || (*cased != lower && !initial_capital) {
        return Decision::abstain(Reason::Ineligible);
    }
    if dictionaries.en.is_none() || dictionaries.ru.is_none() {
        return Decision::abstain(Reason::DictionariesMissing);
    }
    let original_membership = membership(dictionaries, word);
    let core_membership = membership(dictionaries, core);
    if [original_membership, core_membership]
        .iter()
        .any(|m| matches!(m, Membership::Recognized | Membership::Rare))
    {
        return Decision::abstain(Reason::Personal);
    }
    let candidate_valid = script(&candidate) == Some(target);
    let frequency = candidate_valid
        .then(|| {
            model
                .language(target)
                .frequency(&vocabulary::lowercase(&candidate))
        })
        .flatten();
    let known = (original_membership != Membership::Forbidden && script(core) == Some(active))
        .then(|| {
            model
                .language(active)
                .frequency(&vocabulary::lowercase(core))
        })
        .flatten();
    let mut ratio = None;
    if let Some(source) = known {
        let margin = if word.len() == 2 && request.context != Some(target) {
            policy.short_known_margin
        } else {
            policy.known_margin
        };
        ratio = frequency
            .map(|frequency| frequency - source)
            .filter(|&ratio| {
                word.len() > 1 && source + 9.0 < policy.known_ceiling && ratio >= margin
            });
        if ratio.is_none() {
            return Decision::abstain(Reason::Known);
        }
    }
    if !candidate_valid {
        return Decision::abstain(Reason::Ineligible);
    }
    if membership(dictionaries, &candidate) == Membership::Forbidden {
        return Decision::abstain(Reason::Forbidden);
    }
    let Some(frequency) = frequency else {
        return Decision::abstain(Reason::Weak);
    };
    if frequency + 9.0 < policy.minimum_zipf {
        return Decision::abstain(Reason::Weak);
    }
    // A rare known word is word-level evidence; its character chain says nothing more
    if let Some(score) = ratio {
        return Decision {
            reason: Reason::Correction,
            score,
            replacement: Some(candidate),
        };
    }
    let Some(mut source_likelihood) = model
        .language(active)
        .log_likelihood(&vocabulary::lowercase(&letters))
    else {
        return Decision::abstain(Reason::Ineligible);
    };
    if provisional {
        let first = word
            .iter()
            .position(|&unit| !is_provisional(unit, keyboard));
        for (at, &unit) in word.iter().enumerate() {
            if is_provisional(unit, keyboard) {
                source_likelihood += PROVISIONAL_LOG10[if first.is_some_and(|first| at < first) {
                    0
                } else if at < core.len() {
                    1
                } else {
                    2
                }];
            }
        }
    }
    let context_factor: f64 = match request.context {
        Some(context) if context == target => 19.0,
        Some(_) => 1.0 / 19.0,
        None => 1.0,
    };
    let score = frequency - source_likelihood
        + (policy.error_prior / (1.0 - policy.error_prior)).log10()
        + context_factor.log10();
    let short_penalty = match word.len() {
        1 => 4.0,
        2 => 2.0,
        3 => 1.0,
        _ => 0.0,
    };
    if score >= policy.false_correction_cost.log10() + short_penalty {
        Decision {
            reason: Reason::Correction,
            score,
            replacement: Some(candidate),
        }
    } else {
        Decision {
            reason: Reason::Weak,
            score,
            replacement: None,
        }
    }
}

/// Infer context from the last two recognized letter runs, skipping nonletters but stopping at
/// LF/CR, within 256 UTF-16 units. This consumes supplied context, never reads a document, and
/// does not define automatic token eligibility or authorize a replacement range.
pub fn detect_context(model: &Model<'_>, prefix: &[u16]) -> Option<Mode> {
    let start = prefix.len().saturating_sub(256);
    let mut cursor = prefix.len();
    let mut language = None;
    for _ in 0..2 {
        while cursor > start && !mapping::is_letter(prefix[cursor - 1]) {
            if matches!(prefix[cursor - 1], 10 | 13) {
                return None;
            }
            cursor -= 1;
        }
        let end = cursor;
        while cursor > start && mapping::is_letter(prefix[cursor - 1]) {
            cursor -= 1;
        }
        if cursor == end || (cursor == start && start > 0 && mapping::is_letter(prefix[start - 1]))
        {
            return None;
        }
        let word = &prefix[cursor..end];
        let mode = script(word)?;
        if word.len() > MAX_MODEL_WORD
            || model
                .language(mode)
                .frequency(&vocabulary::lowercase(word))
                .is_none()
            || language.is_some_and(|previous| previous != mode)
        {
            return None;
        }
        language = Some(mode);
    }
    language
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuated_context_preserves_source_language_protection() {
        let model = Model::embedded().unwrap();
        for (prefix, expected) in [
            ("hello, world : ", Some(Mode::En)),
            ("привет - мир! ", Some(Mode::Ru)),
            ("hello\nworld ", None),
            ("hello мир ", None),
        ] {
            assert_eq!(
                detect_context(&model, &prefix.encode_utf16().collect::<Vec<_>>()),
                expected,
                "{prefix}"
            );
        }
        let empty = Vocabulary::parse(b"").unwrap();
        let word: Vec<_> = "ghbdtn".encode_utf16().collect();
        let request = Request {
            word: &word,
            active: Some(Mode::En),
            context: None,
            keyboard: Keyboard::Apple,
            explicit_intent: false,
        };
        let dictionaries = Dictionaries {
            en: Some(&empty),
            ru: Some(&empty),
        };
        let neutral = infer(&model, dictionaries, Policy::default(), request);
        let source = infer(
            &model,
            dictionaries,
            Policy::default(),
            Request {
                context: detect_context(
                    &model,
                    &"hello, world : ".encode_utf16().collect::<Vec<_>>(),
                ),
                ..request
            },
        );
        assert!((neutral.score - source.score - 19.0_f64.log10()).abs() < 1e-12);
        for raw in ["A", "B", "Ф"] {
            let word: Vec<_> = raw.encode_utf16().collect();
            let mode = script(&word).unwrap();
            assert_eq!(
                infer(
                    &model,
                    dictionaries,
                    Policy::default(),
                    Request {
                        word: &word,
                        active: Some(mode),
                        ..request
                    }
                )
                .reason,
                Reason::Known
            );
        }
        let word: Vec<_> = "Абаж".encode_utf16().collect();
        assert_eq!(
            infer(
                &model,
                dictionaries,
                Policy::default(),
                Request {
                    word: &word,
                    active: Some(Mode::Ru),
                    ..request
                }
            )
            .reason,
            Reason::Ineligible
        );
    }

    #[test]
    fn missing_dictionaries_abstain_unless_intent_is_explicit() {
        let model = Model::embedded().unwrap();
        let word: Vec<_> = "ghbdtn".encode_utf16().collect();
        let request = Request {
            word: &word,
            active: Some(Mode::En),
            context: None,
            keyboard: Keyboard::Apple,
            explicit_intent: false,
        };
        let policy = Policy::default();
        assert_eq!(
            infer(&model, Dictionaries::default(), policy, request).reason,
            Reason::DictionariesMissing
        );
        assert_eq!(
            infer(
                &model,
                Dictionaries::default(),
                policy,
                Request {
                    explicit_intent: true,
                    ..request
                }
            )
            .reason,
            Reason::ExplicitIntent
        );
    }

    #[test]
    fn rare_known_words_and_provisional_letters_yield_only_to_frequent_readings() {
        let model = Model::embedded().unwrap();
        let empty = Vocabulary::parse(b"").unwrap();
        let dictionaries = Dictionaries {
            en: Some(&empty),
            ru: Some(&empty),
        };
        let decide = |raw: &str, active, context, keyboard| {
            let word: Vec<_> = raw.encode_utf16().collect();
            let decision = infer(
                &model,
                dictionaries,
                Policy::default(),
                Request {
                    word: &word,
                    active: Some(active),
                    context,
                    keyboard,
                    explicit_intent: false,
                },
            );
            let replacement = decision
                .replacement
                .map(|units| String::from_utf16(&units).unwrap());
            (decision.reason, replacement)
        };
        let corrected = |text: &str| (Reason::Correction, Some(text.to_owned()));
        let apple = Keyboard::Apple;
        for (raw, active, context, keyboard, expected) in [
            // Rare lexicon entries yield; a two-letter one needs a wider margin or context
            ("ghb", Mode::En, None, apple, corrected("при")),
            ("ult", Mode::En, Some(Mode::En), apple, corrected("где")),
            ("yf", Mode::En, None, apple, corrected("на")),
            ("ещ", Mode::Ru, None, apple, corrected("to")),
            ("uj", Mode::En, None, apple, (Reason::Known, None)),
            ("uj", Mode::En, Some(Mode::En), apple, (Reason::Known, None)),
            ("uj", Mode::En, Some(Mode::Ru), apple, corrected("го")),
            // Known words at or above the ceiling keep their protection
            ("lf", Mode::En, Some(Mode::Ru), apple, (Reason::Known, None)),
            ("шт", Mode::Ru, Some(Mode::En), apple, (Reason::Known, None)),
            // Keys that type Russian letters can begin, continue and end a word
            ("[jhjij", Mode::En, None, apple, corrected("хорошо")),
            (";bpym", Mode::En, None, apple, corrected("жизнь")),
            ("gj'njve", Mode::En, None, apple, corrected("поэтому")),
            ("pyf.", Mode::En, None, apple, corrected("знаю")),
            ("{jhjij", Mode::En, None, apple, corrected("Хорошо")),
            ("\"nj", Mode::En, None, apple, corrected("Это")),
            ("\\krf", Mode::En, None, apple, corrected("ёлка")),
            ("`krf", Mode::En, None, Keyboard::Pc, corrected("ёлка")),
            ("`krf", Mode::En, None, apple, (Reason::Ineligible, None)),
            (
                "\\krf",
                Mode::En,
                None,
                Keyboard::Pc,
                (Reason::Ineligible, None),
            ),
            // An English word before trailing punctuation stays protected
            ("it.", Mode::En, None, apple, (Reason::Known, None)),
            (
                "he,",
                Mode::En,
                Some(Mode::Ru),
                apple,
                (Reason::Known, None),
            ),
            ("hello,", Mode::En, None, apple, (Reason::Known, None)),
            ("ghbdtn,", Mode::En, None, apple, (Reason::Weak, None)),
            ("don't", Mode::En, None, apple, (Reason::Weak, None)),
            // Punctuation alone, capitals and Russian mode are not provisional words
            ("...", Mode::En, None, apple, (Reason::Ineligible, None)),
            ("'", Mode::En, None, apple, (Reason::Ineligible, None)),
            ("[JHJIJ", Mode::En, None, apple, (Reason::Ineligible, None)),
            ("[jhjij", Mode::Ru, None, apple, (Reason::Ineligible, None)),
        ] {
            assert_eq!(decide(raw, active, context, keyboard), expected, "{raw}");
        }
    }

    #[test]
    fn dictionary_membership_alone_never_promotes_a_weak_candidate() {
        let model = Model::embedded().unwrap();
        let en = Vocabulary::parse(b"").unwrap();
        let word: Vec<_> = "qqqqqq".encode_utf16().collect();
        let ru = Vocabulary::parse("йййййй\n".as_bytes()).unwrap();
        let request = Request {
            word: &word,
            active: Some(Mode::En),
            context: None,
            keyboard: Keyboard::Apple,
            explicit_intent: false,
        };
        assert_eq!(
            infer(
                &model,
                Dictionaries {
                    en: Some(&en),
                    ru: Some(&ru)
                },
                Policy::default(),
                request
            )
            .reason,
            Reason::Weak
        );
    }
}
