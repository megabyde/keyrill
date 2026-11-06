//! Portable engine for correcting English/Russian keyboard-layout mistakes.
//!
//! A layout mistake is a word typed with the wrong input mode active: the keys of `привет`
//! pressed in English mode give `ghbdtn`, and the keys of `hello` pressed in Russian mode give
//! `руддщ`. The keys are right and only their reading is wrong, so the fix is to read the same
//! key sequence in the other layout, and the hard part is deciding when to do it. While a word
//! is typed, the native adapter keeps it in the host as an owned composition, marked text that
//! Keyrill inserted and can still replace; at Space the engine decides whether the word reads
//! better in the other layout.
//!
//! The engine decides and proposes; it never reads or edits a document. A native input-method
//! adapter owns the text, performs at most one write per [`session::Plan`], verifies the result
//! and acknowledges it. `ARCHITECTURE.md` at the repository root describes that contract and the
//! safety invariants around it.
//!
//! | Module | Role |
//! |---|---|
//! | [`mapping`] | EN/RU key-position maps over UTF-16 |
//! | [`model`] | Embedded word frequencies and character chains |
//! | [`inference`] | Whether a word was typed in the wrong layout |
//! | [`vocabulary`] | Vim `.add` word lists and immutable snapshots |
//! | [`session`] | Word state machine with acknowledged proposals |
//!
//! # Example
//!
//! Type `ghbdtn` and Space in English mode, acting as an adapter that verifies every write:
//!
//! ```
//! use keyrill_core::inference::{Dictionaries, Policy};
//! use keyrill_core::model::Model;
//! use keyrill_core::session::{Context, Event, Mode, Outcome, Session};
//! use keyrill_core::vocabulary::Vocabulary;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let model = Model::embedded()?;
//! let empty = Vocabulary::parse(b"")?;
//! let dictionaries = Dictionaries { en: Some(&empty), ru: Some(&empty), learned: &[] };
//! let mut session = Session::default();
//!
//! let mut marked = Vec::new();
//! for unit in "ghbdtn ".encode_utf16() {
//!     let event = Event::Text(vec![unit]);
//!     let plan = session.propose_with_inference(
//!         &event, Mode::En, Context::Boundary, &model, dictionaries, Policy::default(),
//!     )?;
//!     // An adapter checks that its composition holds `plan.expected`, writes `plan.text`,
//!     // reads it back, and only then acknowledges.
//!     marked = plan.text.clone();
//!     session.acknowledge(plan.id, Outcome::Verified)?;
//! }
//! assert_eq!(String::from_utf16(&marked)?, "привет ");
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod inference;
pub mod mapping;
pub mod model;
pub mod session;
pub mod vocabulary;
