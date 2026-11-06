//! Stdin probe that `model/tools/evaluate.py` drives during offline evaluation.
//!
//! The probe reads one request per line and writes one response line, so the evaluator can
//! stream a corpus through the real engine without a native input method. It uses the embedded
//! model, the Apple keyboard and empty dictionaries for both languages; no native text service
//! or private vocabulary is read.
//!
//! ```text
//!   inference-probe word [NAME=VALUE...]
//!     request    MODE WORD PREFIX            one isolated word and the text before it
//!     response   REASON <tab> SCORE <tab> REPLACEMENT
//!   inference-probe session [NAME=VALUE...]
//!     request    MODE:TEXT [MODE:TEXT...]     segments typed in order into one document
//!     response   TEXT                         the document after the last segment
//! ```
//!
//! MODE is `en` or `ru`. Text fields are UTF-16 units, four hex digits each, with `-` for empty.
//! Each NAME=VALUE overrides one ranking policy field. A malformed line ends the run with exit
//! status 2.

use keyrill_core::inference::{self, Dictionaries, Policy, Request};
use keyrill_core::mapping::Keyboard;
use keyrill_core::model::Model;
use keyrill_core::session::{Action, Context, Disposition, Event, Mode, Outcome, Plan, Session};
use keyrill_core::vocabulary::Vocabulary;
use std::io::{self, BufRead, Read, Write};

// A longer request line is a protocol error rather than a reason to buffer without bound
const MAX_LINE_BYTES: usize = 65_536;
const USAGE: &str = "usage: inference-probe word|session [NAME=VALUE...]";

// Hex rather than UTF-8 so unpaired surrogates survive the protocol
fn decode(hex: &str) -> Result<Vec<u16>, &'static str> {
    if hex == "-" {
        return Ok(Vec::new());
    }
    if hex.is_empty() || !hex.len().is_multiple_of(4) || !hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid UTF-16 hex");
    }
    (0..hex.len())
        .step_by(4)
        .map(|i| u16::from_str_radix(&hex[i..i + 4], 16).map_err(|_| "invalid UTF-16 hex"))
        .collect()
}

fn mode(tag: &str) -> Result<Mode, &'static str> {
    match tag {
        "en" => Ok(Mode::En),
        "ru" => Ok(Mode::Ru),
        _ => Err("unknown active mode"),
    }
}

fn policy(overrides: &[String]) -> Result<Policy, Box<dyn std::error::Error>> {
    let mut policy = Policy::default();
    for item in overrides {
        let (name, value) = item.split_once('=').ok_or(USAGE)?;
        let value: f64 = value.parse()?;
        match name {
            "error_prior" => policy.error_prior = value,
            "false_correction_cost" => policy.false_correction_cost = value,
            "minimum_zipf" => policy.minimum_zipf = value,
            "known_ceiling" => policy.known_ceiling = value,
            "known_margin" => policy.known_margin = value,
            "short_known_margin" => policy.short_known_margin = value,
            _ => return Err("unknown policy field".into()),
        }
    }
    if !policy.is_valid() {
        return Err("invalid ranking policy".into());
    }
    Ok(policy)
}

// Tab, line feed, carriage return, Space and no-break space end a word in the replay
fn is_whitespace(unit: u16) -> bool {
    matches!(unit, 9 | 10 | 13 | 32 | 0xa0)
}

/// A simulated document whose last `mark` units are the owned composition.
#[derive(Default)]
struct Document {
    text: Vec<u16>,
    mark: usize,
}

impl Document {
    /// Apply one plan the way an adapter does: a write must find exactly the expected mark, and a
    /// passed key must not land inside a composition. Returns whether to plan the event again.
    fn apply(&mut self, plan: &Plan, event: &Event) -> Result<bool, &'static str> {
        if matches!(plan.action, Action::Mark | Action::Finish) {
            let start = self.text.len() - self.mark;
            if self.text[start..] != plan.expected[..] {
                return Err("plan does not match the simulated composition");
            }
            self.text.truncate(start);
            self.text.extend(&plan.text);
            self.mark = if plan.action == Action::Mark {
                plan.text.len()
            } else {
                0
            };
        }
        match (plan.disposition, event) {
            (Disposition::Replan, _) => return Ok(true),
            (Disposition::Pass, Event::Text(units)) => {
                if self.mark != 0 {
                    return Err("a passed key would land inside the composition");
                }
                self.text.extend(units);
            }
            _ => (),
        }
        Ok(false)
    }
}

/// Type segments unit by unit, acting as an adapter that checks and applies every plan before
/// acknowledging it, takes a word boundary only after whitespace, and finishes and resets the
/// session on a mode change.
fn replay(
    model: &Model<'_>,
    dictionaries: Dictionaries<'_>,
    policy: Policy,
    segments: &[(Mode, Vec<u16>)],
) -> Result<Vec<u16>, Box<dyn std::error::Error>> {
    let mut session = Session::default();
    let mut document = Document::default();
    let mut current = None;
    let events = segments.iter().flat_map(|(mode, units)| {
        units
            .iter()
            .map(move |&unit| (*mode, Event::Text(vec![unit])))
    });
    for (mode, event) in events.chain(segments.last().map(|(mode, _)| (*mode, Event::Finish))) {
        if current.is_some_and(|previous| previous != mode) {
            // The adapter commits its composition as it stands before resetting
            document.mark = 0;
            session.reset();
        }
        current = Some(mode);
        for attempt in 0.. {
            if attempt == 2 {
                return Err("a second replan is a protocol error".into());
            }
            let context =
                if document.mark == 0 && document.text.last().is_none_or(|&u| is_whitespace(u)) {
                    Context::Boundary
                } else {
                    Context::Ineligible
                };
            let plan = session.propose_with_inference(
                &event,
                mode,
                context,
                model,
                dictionaries,
                policy,
            )?;
            let replan = document.apply(&plan, &event)?;
            session.acknowledge(plan.id, Outcome::Verified)?;
            if !replan {
                break;
            }
        }
    }
    document.mark = 0;
    Ok(document.text)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let session = match args.first().map(String::as_str) {
        Some("word") => false,
        Some("session") => true,
        _ => return Err(USAGE.into()),
    };
    let policy = policy(&args[1..])?;
    let model = Model::embedded().map_err(|_| "embedded model invalid")?;
    let empty = Vocabulary::parse(b"").map_err(|_| "empty vocabulary invalid")?;
    let dictionaries = Dictionaries {
        en: Some(&empty),
        ru: Some(&empty),
        learned: &[],
    };
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    loop {
        let mut line = String::new();
        let length = input
            .by_ref()
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_line(&mut line)?;
        if length == 0 {
            break;
        }
        if length > MAX_LINE_BYTES {
            return Err("protocol line exceeds bound".into());
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let units = if session {
            let segments = fields
                .iter()
                .map(|field| {
                    let (tag, hex) = field.split_once(':').ok_or("expected MODE:HEX segments")?;
                    Ok((mode(tag)?, decode(hex)?))
                })
                .collect::<Result<Vec<_>, &'static str>>()?;
            if segments.is_empty() {
                return Err("expected MODE:HEX segments".into());
            }
            replay(&model, dictionaries, policy, &segments)?
        } else {
            if fields.len() != 3 {
                return Err("expected three protocol fields".into());
            }
            let word = decode(fields[1])?;
            let prefix = decode(fields[2])?;
            let decision = inference::infer(
                &model,
                dictionaries,
                policy,
                Request {
                    word: &word,
                    active: Some(mode(fields[0])?),
                    context: inference::detect_context(&model, &prefix),
                    keyboard: Keyboard::Apple,
                    explicit_intent: false,
                },
            );
            write!(
                output,
                "{}\t{:.17}\t",
                decision.reason as u8, decision.score
            )?;
            decision.replacement.unwrap_or_default()
        };
        if units.is_empty() {
            write!(output, "-")?;
        }
        for unit in units {
            write!(output, "{unit:04x}")?;
        }
        writeln!(output)?;
    }
    output.flush()?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("inference-probe: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The replay must refuse a write whose expected text is not the composition, as an adapter
    // does; applying it anyway would score corrections no adapter could make
    #[test]
    fn replay_refuses_a_write_that_does_not_find_the_expected_mark() {
        let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let plan = |action, disposition, expected: &str, text: &str| Plan {
            id: 1,
            action,
            disposition,
            expected: units(expected),
            text: units(text),
        };
        let space = Event::Text(vec![32]);
        let mut document = Document {
            text: units("hello ghbdtn"),
            mark: 6,
        };
        let stale = plan(Action::Mark, Disposition::Consumed, "ghbdtx", "привет ");
        assert_eq!(
            document.apply(&stale, &space),
            Err("plan does not match the simulated composition")
        );
        assert_eq!((document.text, document.mark), (units("hello ghbdtn"), 6));

        let mut document = Document {
            text: units("hello ghbdtn"),
            mark: 6,
        };
        let mark = plan(Action::Mark, Disposition::Consumed, "ghbdtn", "привет ");
        assert_eq!(document.apply(&mark, &space), Ok(false));
        let finish = plan(Action::Finish, Disposition::Replan, "привет ", "привет ");
        assert_eq!(document.apply(&finish, &Event::Text(vec![120])), Ok(true));
        assert_eq!(
            (&document.text, document.mark),
            (&units("hello привет "), 0)
        );

        let mut document = Document {
            text: units("x"),
            mark: 1,
        };
        let pass = plan(Action::Pass, Disposition::Pass, "x", "");
        assert_eq!(
            document.apply(&pass, &Event::Text(vec![33])),
            Err("a passed key would land inside the composition")
        );
    }

    #[test]
    fn protocol_hex_preserves_raw_units_and_rejects_malformed_input() {
        assert_eq!(decode("-"), Ok(Vec::new()));
        assert_eq!(decode("d8000061dc00"), Ok(vec![0xd800, 97, 0xdc00]));
        for invalid in ["", "abc", "zzzz", "éé", "😀", "0061000g"] {
            assert_eq!(decode(invalid), Err("invalid UTF-16 hex"));
        }
    }
}
