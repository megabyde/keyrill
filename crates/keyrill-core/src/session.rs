//! The word session: one owned word at a time, advanced only by verified acknowledgements.
//!
//! While a word is typed, the adapter keeps it in the host as an owned composition, marked text
//! that Keyrill inserted and can still replace as a whole. The session is the Rust record of
//! that word; it never touches the host. For each event the adapter asks for a [`Plan`], performs
//! the one native action the plan permits, verifies it and reports the [`Outcome`]. Proposing
//! changes nothing the session has confirmed: the proposed state waits in the session until
//! [`Session::acknowledge`] receives a verified outcome for that plan's id, and a newer proposal
//! makes an older plan stale.
//!
//! ```text
//! adapter                                          Session
//! key event      --- propose_*(event) -------->    plan the event against the confirmed state
//!                <-- Plan { id: n, .. } -------    keep the proposed state as pending
//! the mark still holds `expected`?
//!   no: report Refused, nothing written
//! perform `action` with `text`, at most one write
//! read the result back
//!                --- acknowledge(n, outcome) -->   Verified: adopt the pending state
//!                                                  Refused or Unknown: Invalidated
//! pass the key, consume it or propose it again, as `disposition` says
//! ```
//!
//! The word moves through four states. `ARCHITECTURE.md` uses these names; in the code Composing
//! and BoundaryRetained are one `Owned` state with the `boundary` flag clear or set.
//!
//! ```text
//!                   letter at a boundary: Mark
//!    +------+ ---------------------------------> +-----------+  letter, Backspace,
//!    | Idle |                                    | Composing |  Manual, Reverse: Mark
//!    +------+ <--------------------------------- +-----------+
//!       ^      Backspace to empty: Mark (empty)        |
//!       |      Escape or any other event: Finish       | Space: infer, then Mark the word
//!       |                                              v        and the Space
//!       |      Escape or any other event: Finish  +------------------+
//!       +---------------------------------------- | BoundaryRetained |  Manual, Reverse: Mark
//!              (a letter is then planned again)   +------------------+
//!
//!    Idle passes every other event. Manual or Reverse with nothing to act on is consumed
//!    without a write, in every state.
//!
//!    From any state, a Refused or Unknown outcome, a mode change while a word is owned or an
//!    exhausted token counter leads to Invalidated, which refuses every proposal; reset()
//!    returns to Idle, except after exhaustion.
//! ```
//!
//! A receipt is the text the word had before the session first converted it, automatically at
//! Space or by a manual command. While the same word is owned, Reverse restores it once, and
//! Escape restores it and finishes the word; an edit expires it, and so does finishing. Manual
//! and Reverse also set explicit intent, which stops inference for the rest of that word, so
//! inference never undoes a choice the user made. A Backspace is not intent, and a corrected
//! typo still converts.
//!
//! Language context comes from the session's own finished words, never from the document. A
//! word that ends in its Space and is finished by the letter that starts the next word is
//! appended once that finish is verified, up to 256 UTF-16 units. Any other finish, and any
//! event the host handles, clears it, because the host can then move the caret or edit the
//! text; so do a mode change, a refused or unknown outcome and reset.

use crate::inference::{self, Dictionaries, Policy, Request};
use crate::mapping::{self, Keyboard, Mapping};
use crate::model::Model;

/// Maximum composing word length in UTF-16 units, excluding retained Space.
pub const MAX_WORD: usize = 64;
const MAX_HISTORY: usize = 256;

/// Input mode: the layout Keyrill types in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// English mode.
    En,
    /// Russian mode.
    Ru,
}

/// What the adapter verified before the caret. It matters only when no word is owned: a letter
/// starts a word only at [`Context::Boundary`], so the session never takes over the tail of a
/// token already in the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// Start of document or whitespace.
    Boundary,
    /// Anything else, or a prefix the adapter could not establish.
    Ineligible,
}

/// Input supplied by the native controller, without document offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Text units the key would insert. A single letter, or in English mode a single
    /// provisional unit, starts or continues a word; a single Space takes an owned word to its
    /// boundary; anything else finishes an owned word and goes to the host.
    Text(Vec<u16>),
    /// Delete the last owned unit; at the boundary it finishes the word and goes to the host.
    Backspace,
    /// Restore the original and finish if a receipt exists; otherwise finish as typed and pass.
    Escape,
    /// Convert the owned word key by key to the other layout; a conversion sets explicit intent.
    Manual,
    /// Restore the receipt's text exactly while the same word is owned, with explicit intent.
    Reverse,
    /// Finish the owned word as it stands and let the host handle the event.
    Finish,
}

/// The one native operation a plan permits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// No native mutation.
    Pass,
    /// Set the owned mark to `text`, or start one if none is owned. An empty `text` ends the
    /// composition with nothing in it.
    Mark,
    /// Commit `text` in place of the owned mark, once, and end the composition.
    Finish,
    /// A reserved command (Manual or Reverse) with nothing to act on; nothing is written.
    Unavailable,
}

/// What happens to the key event once the plan's action is verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Let the host handle the event once.
    Pass,
    /// The event is handled; the host does not see it.
    Consumed,
    /// Finish was acknowledged; plan the same event again from Idle.
    Replan,
}

/// Native effect acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Native effect and readback verified, or the plan had no native effect.
    Verified,
    /// Rejected before any native write.
    Refused,
    /// Write attempted but outcome cannot be established.
    Unknown,
}

/// Session protocol errors; no raw text is included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The id is not the pending plan: a later proposal superseded it, it was already
    /// acknowledged, or a reset expired it.
    Stale,
    /// Session invalidated or token counter exhausted.
    Invalidated,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Stale => "plan is not the latest proposal",
            Self::Invalidated => "session is invalidated",
        })
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    /// Composing while `boundary` is clear, BoundaryRetained once the Space has joined the mark.
    Owned {
        /// What the owned mark holds, including the Space once `boundary` is set.
        text: Vec<u16>,
        /// The receipt: the text before the word was first converted; `None` if it was never
        /// converted, or after an edit or a Reverse.
        original: Option<Vec<u16>>,
        boundary: bool,
        /// Set by Manual or Reverse; Space then keeps the word without inference.
        explicit: bool,
        /// Input mode the word was started in.
        mode: Mode,
    },
    Invalidated,
}

/// Automatic correction at Space: the fixture policy, on or off, or the model.
enum Automatic<'a> {
    Fixture(bool),
    Model(&'a Model<'a>, Dictionaries<'a>, Policy),
}

/// What a verified plan does to the language history.
enum HistoryEffect {
    Keep,
    Clear,
    Append(Vec<u16>),
}

/// The proposed state of the latest plan, adopted only by a verified acknowledgement.
struct Pending {
    id: u64,
    state: State,
    history: HistoryEffect,
    mode: Mode,
}

/// Copied proposal for native validation and a single native effect. It owns its text and
/// borrows nothing from the session, the model or the vocabulary snapshots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Latest-only acknowledgement token.
    pub id: u64,
    /// Permitted native operation.
    pub action: Action,
    /// Event disposition after verification.
    pub disposition: Disposition,
    /// The UTF-16 units the owned mark must hold before the action; empty when none is owned.
    pub expected: Vec<u16>,
    /// The UTF-16 units to mark or commit; empty for [`Action::Pass`] and
    /// [`Action::Unavailable`].
    pub text: Vec<u16>,
}

/// One controller's bounded word state; contains no native objects or callbacks. It holds at
/// most one word of [`MAX_WORD`] units plus its Space, the word's receipt, and 256 UTF-16 units
/// of history.
#[derive(Default)]
pub struct Session {
    /// Confirmed state.
    state: State,
    pending: Option<Pending>,
    /// Last issued token; never reset, so no later plan can reuse an expired id.
    serial: u64,
    fixtures: bool,
    /// Finished words in document order, each followed by its Space.
    history: Vec<u16>,
    /// Input mode of the last verified plan; history is consulted only under the same mode.
    mode: Option<Mode>,
    keyboard: Keyboard,
}

impl Session {
    /// Whether a copied proposal is awaiting acknowledgement.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether the session holds an owned word. This is not evidence that the host still shows
    /// the mark, and it grants no native authority.
    pub fn has_word(&self) -> bool {
        matches!(self.state, State::Owned { .. })
    }

    /// Invalidate outstanding tokens and drop all text/receipts on native lifecycle change.
    /// The session returns to Idle with no history; the keyboard and fixture selections stay.
    pub fn reset(&mut self) {
        self.pending = None;
        self.state = State::Idle;
        self.history.clear();
        self.mode = None;
    }

    /// Enable the deterministic fixture policy of [`Session::propose`], which converts only
    /// `ghbdtn` and `руддщ`, while idle.
    ///
    /// # Errors
    ///
    /// [`Error::Invalidated`] if a word is owned or a proposal awaits acknowledgement.
    pub fn set_fixtures(&mut self, enabled: bool) -> Result<(), Error> {
        if self.state != State::Idle || self.pending.is_some() {
            return Err(Error::Invalidated);
        }
        self.fixtures = enabled;
        Ok(())
    }

    /// Select the Russian keyboard map for manual conversion and inference while idle. The
    /// default is [`Keyboard::Apple`], and [`Session::reset`] keeps the selection. A change
    /// keeps the word history, which holds finished document text rather than key positions.
    ///
    /// # Errors
    ///
    /// [`Error::Invalidated`] if a word is owned or a proposal awaits acknowledgement.
    pub fn set_keyboard(&mut self, keyboard: Keyboard) -> Result<(), Error> {
        if self.state != State::Idle || self.pending.is_some() {
            return Err(Error::Invalidated);
        }
        self.keyboard = keyboard;
        Ok(())
    }

    /// Construct a proposal using only the opt-in fixture policy.
    /// Confirmed state and context remain unchanged until acknowledgement.
    ///
    /// # Errors
    ///
    /// [`Error::Invalidated`] if the session was invalidated or its token counter is exhausted.
    pub fn propose(&mut self, event: &Event, mode: Mode, context: Context) -> Result<Plan, Error> {
        self.propose_using(event, mode, context, Automatic::Fixture(self.fixtures))
    }

    /// Construct a manual-only proposal, ignoring both the model and the fixture policy.
    ///
    /// # Errors
    ///
    /// [`Error::Invalidated`] if the session was invalidated or its token counter is exhausted.
    pub fn propose_without_automatic(
        &mut self,
        event: &Event,
        mode: Mode,
        context: Context,
    ) -> Result<Plan, Error> {
        self.propose_using(event, mode, context, Automatic::Fixture(false))
    }

    /// Construct a model-backed proposal with explicit borrowed protection snapshots and policy.
    /// Only Space evaluates inference, and not after explicit intent. Context comes from this
    /// session's verified finished words; the pending operation retains no model, vocabulary
    /// reference or native callback. See the [crate-level example](crate#example) for a
    /// complete word.
    ///
    /// # Errors
    ///
    /// [`Error::Invalidated`] if the session was invalidated or its token counter is exhausted.
    pub fn propose_with_inference(
        &mut self,
        event: &Event,
        mode: Mode,
        context: Context,
        model: &Model<'_>,
        dictionaries: Dictionaries<'_>,
        policy: Policy,
    ) -> Result<Plan, Error> {
        self.propose_using(
            event,
            mode,
            context,
            Automatic::Model(model, dictionaries, policy),
        )
    }

    fn propose_using(
        &mut self,
        event: &Event,
        mode: Mode,
        context: Context,
        automatic: Automatic<'_>,
    ) -> Result<Plan, Error> {
        if self.state == State::Invalidated {
            return Err(Error::Invalidated);
        }
        // Exhaustion is permanent: reset keeps the counter, so every later proposal fails too
        let Some(id) = self.serial.checked_add(1) else {
            self.state = State::Invalidated;
            self.pending = None;
            self.history.clear();
            self.mode = None;
            return Err(Error::Invalidated);
        };
        self.serial = id;
        let expected = match &self.state {
            State::Owned { text, .. } => text.clone(),
            _ => Vec::new(),
        };
        let (action, disposition, text, next) = self.transition(event, mode, context, automatic);
        // Only a word finished by the letter that starts the next one is appended: the session
        // saw everything around it
        let history = if next == State::Invalidated
            || self.mode.is_some_and(|previous| previous != mode)
            || matches!(event, Event::Finish | Event::Escape)
        {
            HistoryEffect::Clear
        } else if action == Action::Finish && disposition == Disposition::Replan {
            HistoryEffect::Append(text.clone())
        } else if action == Action::Finish
            || (action == Action::Pass && disposition == Disposition::Pass)
        {
            // An unowned host action can navigate or edit outside this history
            HistoryEffect::Clear
        } else {
            HistoryEffect::Keep
        };
        self.pending = Some(Pending {
            id,
            state: next,
            history,
            mode,
        });
        Ok(Plan {
            id,
            action,
            disposition,
            expected,
            text,
        })
    }

    /// Confirm the latest proposal, or drop authority after refusal/unknown effect.
    ///
    /// [`Outcome::Verified`] advances the session to the proposed state. Any other outcome
    /// invalidates the word, its receipt and the language history, and the session refuses
    /// proposals until [`Session::reset`].
    ///
    /// # Errors
    ///
    /// [`Error::Stale`] if `id` is not the pending proposal; state is unchanged.
    pub fn acknowledge(&mut self, id: u64, outcome: Outcome) -> Result<(), Error> {
        if self.pending.as_ref().map(|p| p.id) != Some(id) {
            return Err(Error::Stale);
        }
        let pending = self.pending.take().ok_or(Error::Stale)?;
        if outcome == Outcome::Verified {
            match pending.history {
                HistoryEffect::Keep => (),
                HistoryEffect::Clear => self.history.clear(),
                HistoryEffect::Append(text) => {
                    self.history.extend(text);
                    // Trim at a Space so the window starts at a whole word; this can drop one
                    // word more than the bound requires
                    if self.history.len() > MAX_HISTORY {
                        let minimum = self.history.len() - MAX_HISTORY;
                        let remove = self.history[minimum..]
                            .iter()
                            .position(|&unit| unit == 32)
                            .map_or(self.history.len(), |at| minimum + at + 1);
                        self.history.drain(..remove);
                    }
                }
            }
            self.mode = Some(pending.mode);
            self.state = pending.state;
        } else {
            self.state = State::Invalidated;
            self.history.clear();
            self.mode = None;
        }
        Ok(())
    }

    /// The transition table: the plan's action, disposition and text, and the proposed state.
    /// Reads the confirmed state and changes nothing.
    fn transition(
        &self,
        event: &Event,
        mode: Mode,
        context: Context,
        automatic: Automatic<'_>,
    ) -> (Action, Disposition, Vec<u16>, State) {
        use Action::{Finish, Mark, Pass, Unavailable};
        use Disposition::{Consumed, Replan};
        // In English mode the keys that type Russian letters can start or continue a word
        let letter = match event {
            Event::Text(units)
                if units.len() == 1
                    && (mapping::is_letter(units[0])
                        || (mode == Mode::En
                            && inference::is_provisional(units[0], self.keyboard))) =>
            {
                Some(units[0])
            }
            _ => None,
        };
        match &self.state {
            State::Idle => {
                if let Some(unit) = letter.filter(|_| context == Context::Boundary) {
                    let text = vec![unit];
                    let next = State::Owned {
                        text: text.clone(),
                        original: None,
                        boundary: false,
                        explicit: false,
                        mode,
                    };
                    return (Mark, Consumed, text, next);
                }
                // A reserved command is consumed even with nothing to act on
                let reserved = matches!(event, Event::Manual | Event::Reverse);
                (
                    if reserved { Unavailable } else { Pass },
                    if reserved {
                        Consumed
                    } else {
                        Disposition::Pass
                    },
                    Vec::new(),
                    State::Idle,
                )
            }
            State::Owned {
                text,
                original,
                boundary,
                explicit,
                mode: owned_mode,
            } => {
                // A word is never finished or converted under a mode it was not typed in
                if mode != *owned_mode {
                    return (Pass, Disposition::Pass, Vec::new(), State::Invalidated);
                }
                let mut text = text.clone();
                let mut original = original.clone();
                let mut boundary = *boundary;
                let mut explicit = *explicit;
                match event {
                    Event::Manual => {
                        // Provisional units are owned as Russian letters, so they convert too
                        let converted: Vec<u16> = text
                            .iter()
                            .map(|&unit| {
                                let mapping = if mode == Mode::En
                                    && inference::is_provisional(unit, self.keyboard)
                                {
                                    Mapping::EnToRu
                                } else {
                                    Mapping::Manual
                                };
                                mapping::convert(&[unit], self.keyboard, mapping)[0]
                            })
                            .collect();
                        if converted == text {
                            return (Pass, Consumed, Vec::new(), self.state.clone());
                        }
                        // A second conversion keeps the first receipt, so Reverse returns to
                        // what was typed
                        original = Some(original.unwrap_or_else(|| text.clone()));
                        text = converted;
                        explicit = true;
                    }
                    Event::Reverse => {
                        let Some(raw) = original.take() else {
                            return (Unavailable, Consumed, Vec::new(), self.state.clone());
                        };
                        text = raw;
                        explicit = true;
                    }
                    Event::Escape => {
                        return match original {
                            Some(raw) => (Finish, Consumed, raw, State::Idle),
                            None => (Finish, Disposition::Pass, text, State::Idle),
                        };
                    }
                    Event::Text(units) if units == &[32] && !boundary => {
                        if !explicit {
                            let converted = match automatic {
                                Automatic::Fixture(true) => fixture(&text, mode),
                                Automatic::Fixture(false) => None,
                                Automatic::Model(model, dictionaries, policy) => {
                                    inference::infer(
                                        model,
                                        dictionaries,
                                        policy,
                                        Request {
                                            word: &text,
                                            active: Some(mode),
                                            context: if self.mode == Some(mode) {
                                                inference::detect_context(model, &self.history)
                                            } else {
                                                None
                                            },
                                            keyboard: self.keyboard,
                                            explicit_intent: explicit,
                                        },
                                    )
                                    .replacement
                                }
                            };
                            if let Some(converted) = converted {
                                original = Some(text.clone());
                                text = converted;
                            }
                        }
                        // The Space joins the mark, and the receipt, so that Reverse replaces
                        // the word and its Space as one owned range
                        text.push(32);
                        if let Some(raw) = &mut original {
                            raw.push(32);
                        }
                        boundary = true;
                    }
                    Event::Backspace if !boundary => {
                        text.pop();
                        // An edit expires the conversion receipt
                        original = None;
                        // An empty mark is its own native operation and ends the word
                        if text.is_empty() {
                            return (Mark, Consumed, text, State::Idle);
                        }
                    }
                    Event::Text(_) if letter.is_some() && !boundary && text.len() < MAX_WORD => {
                        text.push(letter.expect("classified letter"));
                        original = None;
                    }
                    // After the boundary a letter starts the next word, so it is planned again
                    // from Idle; any other event, or a letter past MAX_WORD, goes to the host
                    _ => {
                        let disposition = if boundary && letter.is_some() {
                            Replan
                        } else {
                            Disposition::Pass
                        };
                        return (Finish, disposition, text, State::Idle);
                    }
                }
                let next = State::Owned {
                    text: text.clone(),
                    original,
                    boundary,
                    explicit,
                    mode,
                };
                (Mark, Consumed, text, next)
            }
            State::Invalidated => unreachable!("checked by propose"),
        }
    }
}

/// The fixture policy: converts exactly `ghbdtn` and `руддщ`, and nothing else.
fn fixture(text: &[u16], mode: Mode) -> Option<Vec<u16>> {
    let (source, target) = match mode {
        Mode::En => ("ghbdtn", "привет"),
        Mode::Ru => ("руддщ", "hello"),
    };
    (text.iter().copied().eq(source.encode_utf16())).then(|| target.encode_utf16().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(s: &mut Session, e: Event) -> Plan {
        let p = s.propose(&e, Mode::En, Context::Boundary).unwrap();
        s.acknowledge(p.id, Outcome::Verified).unwrap();
        p
    }
    fn type_word(s: &mut Session, word: &str) {
        for u in word.encode_utf16() {
            apply(s, Event::Text(vec![u]));
        }
    }

    fn model_apply(
        session: &mut Session,
        model: &Model<'_>,
        dictionaries: Dictionaries<'_>,
        mode: Mode,
        event: Event,
    ) -> Plan {
        let mut plan = session
            .propose_with_inference(
                &event,
                mode,
                Context::Boundary,
                model,
                dictionaries,
                Policy::default(),
            )
            .unwrap();
        session.acknowledge(plan.id, Outcome::Verified).unwrap();
        if plan.disposition == Disposition::Replan {
            plan = session
                .propose_with_inference(
                    &event,
                    mode,
                    Context::Boundary,
                    model,
                    dictionaries,
                    Policy::default(),
                )
                .unwrap();
            assert_ne!(plan.disposition, Disposition::Replan);
            session.acknowledge(plan.id, Outcome::Verified).unwrap();
        }
        plan
    }

    fn type_model(
        session: &mut Session,
        model: &Model<'_>,
        dictionaries: Dictionaries<'_>,
        mode: Mode,
        text: &str,
    ) -> Plan {
        text.encode_utf16()
            .map(|unit| model_apply(session, model, dictionaries, mode, Event::Text(vec![unit])))
            .last()
            .unwrap()
    }

    #[test]
    fn actual_model_space_and_owned_reversal_in_both_directions() {
        let model = Model::embedded().unwrap();
        let empty = crate::vocabulary::Vocabulary::parse(b"").unwrap();
        let dictionaries = Dictionaries {
            en: Some(&empty),
            ru: Some(&empty),
            learned: &[],
        };
        for (mode, raw, corrected) in [
            (Mode::En, "cgbcjr", "список "),
            (Mode::Ru, "цщкдв", "world "),
        ] {
            let mut session = Session::default();
            // The fixture policy cannot convert these words, so a conversion proves the model ran
            session.set_fixtures(true).unwrap();
            type_model(&mut session, &model, dictionaries, mode, raw);
            // Context is consulted only to start a word; an owned word converts whatever it says
            let boundary = session
                .propose_with_inference(
                    &Event::Text(vec![32]),
                    mode,
                    Context::Ineligible,
                    &model,
                    dictionaries,
                    Policy::default(),
                )
                .unwrap();
            assert_eq!(boundary.expected, raw.encode_utf16().collect::<Vec<_>>());
            assert_eq!(boundary.text, corrected.encode_utf16().collect::<Vec<_>>());
            assert!(matches!(
                &session.state,
                State::Owned {
                    boundary: false,
                    ..
                }
            ));
            assert!(session.history.is_empty());
            session.acknowledge(boundary.id, Outcome::Verified).unwrap();
            let reversed = model_apply(&mut session, &model, dictionaries, mode, Event::Reverse);
            assert_eq!(
                reversed.text,
                format!("{raw} ").encode_utf16().collect::<Vec<_>>()
            );
            model_apply(&mut session, &model, dictionaries, mode, Event::Backspace);
            assert_eq!(session.state, State::Idle);
            assert!(session.history.is_empty());
        }
    }

    #[test]
    fn model_boundary_uses_ready_last_good_snapshots_without_retaining_borrows() {
        use crate::vocabulary::Snapshot;

        let model = Model::embedded().unwrap();
        let mut en = Snapshot::default();
        let mut ru = Snapshot::default();
        ru.replace(b"").unwrap();
        let ru = ru.current().unwrap();
        let mut session = Session::default();
        // A missing source stops automatic correction but not manual conversion
        let missing = Dictionaries {
            en: None,
            ru: Some(&ru),
            learned: &[],
        };
        assert_eq!(
            type_model(&mut session, &model, missing, Mode::En, "cgbcjr ").text,
            "cgbcjr ".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            model_apply(&mut session, &model, missing, Mode::En, Event::Manual).text,
            "список ".encode_utf16().collect::<Vec<_>>()
        );
        session.reset();
        // A rejected candidate cannot change the snapshot a reader already holds
        en.replace(b"cgbcjr").unwrap();
        let protected = en.current().unwrap();
        assert!(en.replace(b"invalid\0").is_err());
        let dictionaries = Dictionaries {
            en: Some(&protected),
            ru: Some(&ru),
            learned: &[],
        };
        assert_eq!(
            type_model(&mut session, &model, dictionaries, Mode::En, "cgbcjr ").text,
            "cgbcjr ".encode_utf16().collect::<Vec<_>>()
        );
        session.reset();
        en.replace(b"").unwrap();
        let ready = en.current().unwrap();
        let dictionaries = Dictionaries {
            en: Some(&ready),
            ru: Some(&ru),
            learned: &[],
        };
        // Of two proposals for one Space only the latest, made with the protecting snapshot,
        // can be acknowledged, so a decision from the earlier snapshot never lands
        type_model(&mut session, &model, dictionaries, Mode::En, "cgbcjr");
        let superseded = session
            .propose_with_inference(
                &Event::Text(vec![32]),
                Mode::En,
                Context::Boundary,
                &model,
                dictionaries,
                Policy::default(),
            )
            .unwrap();
        let latest = session
            .propose_with_inference(
                &Event::Text(vec![32]),
                Mode::En,
                Context::Boundary,
                &model,
                Dictionaries {
                    en: Some(&protected),
                    ru: Some(&ru),
                    learned: &[],
                },
                Policy::default(),
            )
            .unwrap();
        assert_eq!(
            session.acknowledge(superseded.id, Outcome::Verified),
            Err(Error::Stale)
        );
        assert_eq!(latest.text, "cgbcjr ".encode_utf16().collect::<Vec<_>>());
        session.acknowledge(latest.id, Outcome::Verified).unwrap();
    }

    #[test]
    fn explicit_reverse_and_typo_edit_have_distinct_model_boundary_behavior() {
        let model = Model::embedded().unwrap();
        let empty = crate::vocabulary::Vocabulary::parse(b"").unwrap();
        let dictionaries = Dictionaries {
            en: Some(&empty),
            ru: Some(&empty),
            learned: &[],
        };
        // Manual then Reverse is explicit intent and Space keeps the typed word; a Backspace and
        // a retyped letter are not, so that word still converts
        let mut session = Session::default();
        type_model(&mut session, &model, dictionaries, Mode::En, "cgbcjr");
        model_apply(&mut session, &model, dictionaries, Mode::En, Event::Manual);
        model_apply(&mut session, &model, dictionaries, Mode::En, Event::Reverse);
        assert_eq!(
            model_apply(
                &mut session,
                &model,
                dictionaries,
                Mode::En,
                Event::Text(vec![32])
            )
            .text,
            "cgbcjr ".encode_utf16().collect::<Vec<_>>()
        );
        session.reset();
        type_model(&mut session, &model, dictionaries, Mode::En, "cgbcjj");
        model_apply(
            &mut session,
            &model,
            dictionaries,
            Mode::En,
            Event::Backspace,
        );
        assert_eq!(
            type_model(&mut session, &model, dictionaries, Mode::En, "r ").text,
            "список ".encode_utf16().collect::<Vec<_>>()
        );
    }

    #[test]
    fn verified_finished_context_changes_the_real_space_decision() {
        let model = Model::embedded().unwrap();
        let empty = crate::vocabulary::Vocabulary::parse(b"").unwrap();
        let dictionaries = Dictionaries {
            en: Some(&empty),
            ru: Some(&empty),
            learned: &[],
        };
        let mut neutral = Session::default();
        assert_eq!(
            type_model(&mut neutral, &model, dictionaries, Mode::En, "yfv ").text,
            "нам ".encode_utf16().collect::<Vec<_>>()
        );
        let mut source = Session::default();
        assert_eq!(
            type_model(
                &mut source,
                &model,
                dictionaries,
                Mode::En,
                "hello world yfv "
            )
            .text,
            "yfv ".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            source.history,
            "hello world ".encode_utf16().collect::<Vec<_>>()
        );
        let mut target = Session::default();
        assert_eq!(
            type_model(
                &mut target,
                &model,
                dictionaries,
                Mode::En,
                "ghbdtn cgbcjr dfv "
            )
            .text,
            "вам ".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            target.history,
            "привет список ".encode_utf16().collect::<Vec<_>>()
        );
        let mut neutral = Session::default();
        assert_eq!(
            type_model(&mut neutral, &model, dictionaries, Mode::En, "dfv ").text,
            "dfv ".encode_utf16().collect::<Vec<_>>()
        );
        // History holds the reversed word as typed; no lexicon has `ghbdtn`, so there is no
        // context and `dfv` stays
        let mut reversed = Session::default();
        type_model(&mut reversed, &model, dictionaries, Mode::En, "ghbdtn ");
        model_apply(
            &mut reversed,
            &model,
            dictionaries,
            Mode::En,
            Event::Reverse,
        );
        assert_eq!(
            type_model(&mut reversed, &model, dictionaries, Mode::En, "cgbcjr dfv ").text,
            "dfv ".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            reversed.history,
            "ghbdtn список ".encode_utf16().collect::<Vec<_>>()
        );
    }

    #[test]
    fn history_is_acknowledged_bounded_and_expires_with_authority() {
        let model = Model::embedded().unwrap();
        let empty = crate::vocabulary::Vocabulary::parse(b"").unwrap();
        let dictionaries = Dictionaries {
            en: Some(&empty),
            ru: Some(&empty),
            learned: &[],
        };
        // A proposal or a stale acknowledgement never changes history, trimming keeps whole
        // words, and Refused, Unknown, Finish, a mode change in Idle or while owned, reset and
        // token exhaustion all clear it
        let mut session = Session::default();
        type_model(&mut session, &model, dictionaries, Mode::En, "hello ");
        let event = Event::Text(vec![119]);
        let propose = |session: &mut Session| {
            session
                .propose_with_inference(
                    &event,
                    Mode::En,
                    Context::Boundary,
                    &model,
                    dictionaries,
                    Policy::default(),
                )
                .unwrap()
        };
        let stale = propose(&mut session);
        let latest = propose(&mut session);
        assert_eq!(
            (latest.action, latest.disposition),
            (Action::Finish, Disposition::Replan)
        );
        assert!(session.history.is_empty());
        assert_eq!(
            session.acknowledge(stale.id, Outcome::Verified),
            Err(Error::Stale)
        );
        assert!(session.history.is_empty());
        session.acknowledge(latest.id, Outcome::Verified).unwrap();
        assert_eq!(session.history, "hello ".encode_utf16().collect::<Vec<_>>());
        let next = propose(&mut session);
        assert_eq!(
            session.acknowledge(stale.id, Outcome::Verified),
            Err(Error::Stale)
        );
        assert_eq!(session.history, "hello ".encode_utf16().collect::<Vec<_>>());
        session.acknowledge(next.id, Outcome::Verified).unwrap();
        type_model(&mut session, &model, dictionaries, Mode::En, "orld ");
        for _ in 0..60 {
            type_model(&mut session, &model, dictionaries, Mode::En, "hello world ");
            assert!(session.history.len() <= 256);
            for word in session
                .history
                .split(|&unit| unit == 32)
                .filter(|word| !word.is_empty())
            {
                assert!(word == [104, 101, 108, 108, 111] || word == [119, 111, 114, 108, 100]);
            }
        }
        for outcome in [Outcome::Refused, Outcome::Unknown] {
            let plan = propose(&mut session);
            session.acknowledge(plan.id, outcome).unwrap();
            assert!(session.history.is_empty());
            session.reset();
            type_model(&mut session, &model, dictionaries, Mode::En, "hello world ");
        }
        model_apply(&mut session, &model, dictionaries, Mode::En, Event::Finish);
        assert!(session.history.is_empty());
        type_model(&mut session, &model, dictionaries, Mode::En, "hello world ");
        let finish = propose(&mut session);
        session.acknowledge(finish.id, Outcome::Verified).unwrap();
        assert_eq!(session.state, State::Idle);
        assert!(!session.history.is_empty());
        model_apply(
            &mut session,
            &model,
            dictionaries,
            Mode::Ru,
            Event::Text(vec![0x430]),
        );
        assert!(session.history.is_empty());
        assert!(matches!(session.state, State::Owned { mode: Mode::Ru, .. }));
        session.reset();
        type_model(&mut session, &model, dictionaries, Mode::En, "hello world ");
        model_apply(
            &mut session,
            &model,
            dictionaries,
            Mode::Ru,
            Event::Text(vec![0x430]),
        );
        assert!(session.history.is_empty());
        assert_eq!(session.state, State::Invalidated);
        session.reset();
        type_model(&mut session, &model, dictionaries, Mode::En, "hello world ");
        let pending = propose(&mut session);
        session.reset();
        assert!(session.history.is_empty());
        assert_eq!(session.mode, None);
        assert_eq!(
            session.acknowledge(pending.id, Outcome::Verified),
            Err(Error::Stale)
        );
        type_model(&mut session, &model, dictionaries, Mode::En, "hello world ");
        session.serial = u64::MAX;
        assert_eq!(
            session.propose(&Event::Finish, Mode::En, Context::Boundary),
            Err(Error::Invalidated)
        );
        assert!(session.history.is_empty());
    }

    #[test]
    fn english_mode_keys_for_russian_letters_join_the_word_and_otherwise_stay_typed() {
        let model = Model::embedded().unwrap();
        let empty = crate::vocabulary::Vocabulary::parse(b"").unwrap();
        let dictionaries = Dictionaries {
            en: Some(&empty),
            ru: Some(&empty),
            learned: &[],
        };
        for (typed, marked) in [
            ("[jhjij ", "хорошо "),
            (";bpym ", "жизнь "),
            ("\"nj ", "Это "),
            ("\\krf ", "ёлка "),
            ("hello, ", "hello, "),
            ("it. ", "it. "),
            ("... ", "... "),
            ("; ", "; "),
            ("'quoted' ", "'quoted' "),
            // On Apple the backtick is not the ё key: it goes to the host and the word starts at k
            ("`krf ", "krf "),
        ] {
            let mut session = Session::default();
            let plan = type_model(&mut session, &model, dictionaries, Mode::En, typed);
            assert_eq!(plan.action, Action::Mark, "{typed}");
            assert_eq!(String::from_utf16(&plan.text).unwrap(), marked, "{typed}");
        }

        // The backtick is the ё key only on the PC layout
        let mut session = Session::default();
        session.set_keyboard(Keyboard::Pc).unwrap();
        let plan = type_model(&mut session, &model, dictionaries, Mode::En, "`krf ");
        assert_eq!(String::from_utf16(&plan.text).unwrap(), "ёлка ");

        // Russian mode passes these keys to the host and starts no word
        let mut session = Session::default();
        let plan = model_apply(
            &mut session,
            &model,
            dictionaries,
            Mode::Ru,
            Event::Text(vec![u16::from(b'[')]),
        );
        assert_eq!(
            (plan.action, plan.disposition),
            (Action::Pass, Disposition::Pass)
        );
        assert!(!session.has_word());
    }

    #[test]
    fn proposal_is_not_installed_and_latest_token_wins() {
        let mut s = Session::default();
        let a = s
            .propose(&Event::Text(vec![97]), Mode::En, Context::Boundary)
            .unwrap();
        let b = s
            .propose(&Event::Text(vec![98]), Mode::En, Context::Boundary)
            .unwrap();
        assert!(b.expected.is_empty());
        assert_eq!(s.acknowledge(a.id, Outcome::Verified), Err(Error::Stale));
        s.acknowledge(b.id, Outcome::Verified).unwrap();
        assert_eq!(apply(&mut s, Event::Text(vec![99])).expected, [98]);
    }

    #[test]
    fn manual_space_reversal_and_expiry() {
        let mut s = Session::default();
        s.set_fixtures(true).unwrap();
        type_word(&mut s, "ghbdtn");
        assert_eq!(
            apply(&mut s, Event::Manual).text,
            "привет".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            apply(&mut s, Event::Text(vec![32])).text,
            "привет ".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(
            apply(&mut s, Event::Reverse).text,
            "ghbdtn ".encode_utf16().collect::<Vec<_>>()
        );
        let finish = apply(&mut s, Event::Text(vec![120]));
        assert_eq!(
            (finish.action, finish.disposition),
            (Action::Finish, Disposition::Replan)
        );
        assert_eq!(apply(&mut s, Event::Reverse).action, Action::Unavailable);
    }

    #[test]
    fn fixture_opt_in_and_both_directions() {
        for mode in [Mode::En, Mode::Ru] {
            for enabled in [false, true] {
                let mut s = Session::default();
                s.set_fixtures(enabled).unwrap();
                let (raw, corrected) = if mode == Mode::En {
                    ("ghbdtn", "привет ")
                } else {
                    ("руддщ", "hello ")
                };
                for u in raw.encode_utf16().chain([32]) {
                    let p = s
                        .propose(&Event::Text(vec![u]), mode, Context::Boundary)
                        .unwrap();
                    if u == 32 {
                        let expected = if enabled {
                            corrected.to_owned()
                        } else {
                            format!("{raw} ")
                        };
                        assert_eq!(p.text, expected.encode_utf16().collect::<Vec<_>>());
                    }
                    s.acknowledge(p.id, Outcome::Verified).unwrap();
                }
            }
        }
    }

    #[test]
    fn unknown_refused_and_reset_expire_pending_and_receipt() {
        for outcome in [Outcome::Unknown, Outcome::Refused] {
            let mut s = Session::default();
            type_word(&mut s, "x");
            let p = s
                .propose(&Event::Manual, Mode::En, Context::Boundary)
                .unwrap();
            s.acknowledge(p.id, outcome).unwrap();
            assert_eq!(
                s.propose(&Event::Reverse, Mode::En, Context::Boundary),
                Err(Error::Invalidated)
            );
            s.reset();
            assert_eq!(s.acknowledge(p.id, Outcome::Verified), Err(Error::Stale));
            assert_eq!(apply(&mut s, Event::Reverse).action, Action::Unavailable);
        }
    }

    #[test]
    fn bounds_and_technical_prefix_never_start_suffix() {
        let mut s = Session::default();
        assert_eq!(
            s.propose(&Event::Text(vec![97]), Mode::En, Context::Ineligible)
                .unwrap()
                .action,
            Action::Pass
        );
        s.reset();
        type_word(&mut s, &"a".repeat(MAX_WORD));
        let p = apply(&mut s, Event::Text(vec![98]));
        assert_eq!(
            (p.action, p.disposition, p.text.len()),
            (Action::Finish, Disposition::Pass, MAX_WORD)
        );
        assert_eq!(
            s.propose(&Event::Text(vec![99]), Mode::En, Context::Ineligible)
                .unwrap()
                .action,
            Action::Pass
        );
    }

    #[test]
    fn empty_deletion_and_escape_use_owned_text() {
        let units = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        let mut s = Session::default();
        s.set_fixtures(true).unwrap();
        type_word(&mut s, "x");
        let p = apply(&mut s, Event::Backspace);
        assert_eq!(p.action, Action::Mark);
        assert!(p.text.is_empty());
        let idle = apply(&mut s, Event::Escape);
        assert_eq!(
            (idle.action, idle.disposition),
            (Action::Pass, Disposition::Pass)
        );

        // A receipt is restored and Escape is consumed
        for (typed, command, restored) in [
            ("ghbdtn", Some(Event::Manual), "ghbdtn"),
            ("ghbdtn ", None, "ghbdtn "),
        ] {
            type_word(&mut s, typed);
            if let Some(command) = command {
                apply(&mut s, command);
            }
            let p = apply(&mut s, Event::Escape);
            assert_eq!(
                (p.action, p.disposition, p.expected.len()),
                (Action::Finish, Disposition::Consumed, typed.len())
            );
            assert_eq!(p.text, units(restored));
            assert_eq!(s.state, State::Idle);
        }

        // Without a receipt the word finishes as typed and the host gets Escape
        for (typed, edit) in [("test", false), ("test ", false), ("ghbdtnn", true)] {
            type_word(&mut s, typed);
            if edit {
                apply(&mut s, Event::Manual);
                // An edit expires the receipt, so Escape cannot restore the typed word
                apply(&mut s, Event::Backspace);
            }
            let expected = match &s.state {
                State::Owned { text, .. } => text.clone(),
                _ => unreachable!("word is owned"),
            };
            let p = apply(&mut s, Event::Escape);
            assert_eq!(
                (p.action, p.disposition),
                (Action::Finish, Disposition::Pass)
            );
            assert_eq!(p.text, expected);
            assert_eq!(s.state, State::Idle);
            assert!(s.history.is_empty());
        }
        assert_eq!(apply(&mut s, Event::Reverse).action, Action::Unavailable);
    }

    #[test]
    fn keyboard_selects_the_yo_key_for_manual_conversion_while_idle() {
        let units = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        let propose = |s: &mut Session, event: Event| {
            let plan = s.propose(&event, Mode::Ru, Context::Boundary).unwrap();
            s.acknowledge(plan.id, Outcome::Verified).unwrap();
            plan
        };
        let mut s = Session::default();
        for (keyboard, converted) in [
            (None, "\\krf"),
            (Some(Keyboard::Pc), "`krf"),
            (Some(Keyboard::Apple), "\\krf"),
        ] {
            if let Some(keyboard) = keyboard {
                s.set_keyboard(keyboard).unwrap();
            }
            for unit in "ёлка".encode_utf16() {
                propose(&mut s, Event::Text(vec![unit]));
            }
            assert_eq!(s.set_keyboard(Keyboard::Pc), Err(Error::Invalidated));
            assert_eq!(propose(&mut s, Event::Manual).text, units(converted));
            assert_eq!(propose(&mut s, Event::Reverse).text, units("ёлка"));
            let pending = s
                .propose(&Event::Finish, Mode::Ru, Context::Boundary)
                .unwrap();
            assert_eq!(s.set_keyboard(Keyboard::Pc), Err(Error::Invalidated));
            s.acknowledge(pending.id, Outcome::Verified).unwrap();
        }

        // Reset keeps the selection; a change keeps verified history
        s.set_keyboard(Keyboard::Pc).unwrap();
        s.reset();
        type_word(&mut s, "hello ");
        let finish = s
            .propose(&Event::Text(vec![119]), Mode::En, Context::Boundary)
            .unwrap();
        assert_eq!(
            (finish.action, finish.disposition),
            (Action::Finish, Disposition::Replan)
        );
        s.acknowledge(finish.id, Outcome::Verified).unwrap();
        assert_eq!(s.state, State::Idle);
        s.set_keyboard(Keyboard::Apple).unwrap();
        s.set_keyboard(Keyboard::Pc).unwrap();
        assert_eq!(s.history, units("hello "));
        for unit in "ё".encode_utf16() {
            propose(&mut s, Event::Text(vec![unit]));
        }
        assert_eq!(propose(&mut s, Event::Manual).text, units("`"));
    }

    #[test]
    fn mode_change_and_token_exhaustion_cannot_restore_authority() {
        // A command in another mode invalidates instead of converting under the wrong map
        let mut s = Session::default();
        type_word(&mut s, "x");
        assert_eq!(s.set_fixtures(true), Err(Error::Invalidated));
        let p = s
            .propose(&Event::Manual, Mode::Ru, Context::Boundary)
            .unwrap();
        assert_eq!((p.action, p.disposition), (Action::Pass, Disposition::Pass));
        s.acknowledge(p.id, Outcome::Verified).unwrap();
        assert_eq!(
            s.propose(&Event::Reverse, Mode::En, Context::Boundary),
            Err(Error::Invalidated)
        );
        s.reset();
        let old = s
            .propose(&Event::Text(vec![97]), Mode::En, Context::Boundary)
            .unwrap();
        s.serial = u64::MAX;
        assert_eq!(
            s.propose(&Event::Manual, Mode::En, Context::Boundary),
            Err(Error::Invalidated)
        );
        assert_eq!(s.acknowledge(old.id, Outcome::Verified), Err(Error::Stale));
    }

    #[test]
    fn corrected_typo_remains_eligible_and_manual_noop_preserves_first_receipt() {
        let mut s = Session::default();
        s.set_fixtures(true).unwrap();
        type_word(&mut s, "ghbdtx");
        apply(&mut s, Event::Backspace);
        type_word(&mut s, "n");
        assert_eq!(
            apply(&mut s, Event::Text(vec![32])).text,
            "привет ".encode_utf16().collect::<Vec<_>>()
        );
        s.reset();
        // In Russian mode `[` is not part of the word, so a second conversion is a no-op
        let ru = |s: &mut Session, event: Event| {
            let plan = s.propose(&event, Mode::Ru, Context::Boundary).unwrap();
            s.acknowledge(plan.id, Outcome::Verified).unwrap();
            plan
        };
        ru(&mut s, Event::Text("х".encode_utf16().collect()));
        assert_eq!(ru(&mut s, Event::Manual).text, [u16::from(b'[')]);
        let noop = ru(&mut s, Event::Manual);
        assert_eq!(
            (noop.action, noop.disposition),
            (Action::Pass, Disposition::Consumed)
        );
        assert_eq!(
            ru(&mut s, Event::Reverse).text,
            "х".encode_utf16().collect::<Vec<_>>()
        );
    }

    #[test]
    fn english_mode_manual_conversion_maps_provisional_letters_and_reverses_exactly() {
        let units = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        for (keyboard, typed, converted) in [
            (Keyboard::Apple, "[jhjij", "хорошо"),
            (Keyboard::Apple, ";bpym", "жизнь"),
            (Keyboard::Apple, "\"nj", "Это"),
            (Keyboard::Apple, "{jhjij", "Хорошо"),
            (Keyboard::Apple, "\\krf", "ёлка"),
            (Keyboard::Pc, "`krf", "ёлка"),
            (Keyboard::Pc, "~krf", "Ёлка"),
            (Keyboard::Apple, "ghbdtn,", "приветб"),
            (Keyboard::Apple, "[jhjij ", "хорошо "),
        ] {
            let mut s = Session::default();
            s.set_keyboard(keyboard).unwrap();
            type_word(&mut s, typed);
            let manual = apply(&mut s, Event::Manual);
            assert_eq!(
                (manual.action, manual.expected.clone(), manual.text.clone()),
                (Action::Mark, units(typed), units(converted)),
                "{typed}"
            );
            // Converting again maps the Russian letters back to their keys
            assert_eq!(apply(&mut s, Event::Manual).text, units(typed), "{typed}");
            apply(&mut s, Event::Manual);
            assert_eq!(apply(&mut s, Event::Reverse).text, units(typed), "{typed}");
            let finish = apply(&mut s, Event::Finish);
            assert_eq!((finish.action, finish.text), (Action::Finish, units(typed)));
        }

        // Russian mode keeps letter-only manual conversion
        let mut s = Session::default();
        for unit in "хорошо".encode_utf16() {
            let plan = s
                .propose(&Event::Text(vec![unit]), Mode::Ru, Context::Boundary)
                .unwrap();
            s.acknowledge(plan.id, Outcome::Verified).unwrap();
        }
        let manual = s
            .propose(&Event::Manual, Mode::Ru, Context::Boundary)
            .unwrap();
        assert_eq!(manual.text, units("[jhjij"));
    }
}
