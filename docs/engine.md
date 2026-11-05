# Engine

`keyrill-core` converts text between layouts, decides whether a typed word is a layout mistake,
parses personal vocabulary and tracks the word session. It makes proposals and never reads or
edits a native document. The C ABI over it is described in [porting](porting.md) and
`include/keyrill.h`.

## Layout mapping

Layouts are US QWERTY and Russian ЙЦУКЕН. Apple's Russian layout and the PC Russian layout differ
on the ё key and are separate `Keyboard` values; the macOS adapter uses the Apple map. A
verified Windows or Linux layout table is the adapter's job, not something to infer from Apple's
Russian-PC layout.

`Mapping::EnToRu` and `RuToEn` convert physical key positions. `Mapping::Manual` converts each
supported letter and leaves punctuation and unsupported UTF-16 units, including unpaired
surrogates, unchanged. It is not an involution; don't test it as one.

A word is a run of EN/RU letters (`mapping::is_letter`). In English mode the keys that type х, ъ,
ж, э, б, ю and ё in Russian (`[ ] ; ' , .`, the ё key for the session's keyboard, `\` on Apple and
`` ` `` on PC, and their shifted forms) are provisional letters: `inference::is_provisional`
units can start or continue a word, so `[jhjij` becomes `хорошо` and `;bpym` becomes `жизнь`.
A word that isn't corrected is committed exactly as typed, so `hello,` stays `hello,` and a lone
`...` or `;` stays punctuation. Such a key is part of the composition rather than passed to the
host, so host behavior tied to the key itself, such as smart quotes or bracket pairing, doesn't
apply to it. In Russian mode these keys are letters and nothing else changes.

## Inference

At Space, `Session::propose_with_inference` evaluates the owned complete word against borrowed
model and vocabulary snapshots and keeps no borrowed input in its pending plan. The default
policy is error prior 0.001, false-change cost 100, minimum candidate Zipf 3, context factor 19
and per-length penalties, plus the known-word gate below. These are uncalibrated ranking
parameters, not probabilities; the numeric domain limits the Zipf values and margins to 0..=9.
The minimum candidate Zipf and the gate values were chosen on the dev split of the
[evaluation](#evaluation).

A typed string that is a word in its own layout's lexicon (Known, reason 3) yields to the other
reading only when it is rare and the other reading is far more frequent: its Zipf score must be
below 3 and the candidate's must exceed it by at least 2.5, or 3.5 for a two-letter word without
context in the target language. A yielding word is corrected on that frequency ratio alone, with
the ratio as its score; the character chain adds nothing for a string the lexicon already has.
So `yf` (Zipf 2.37) becomes `на` (7.25) and `ghb` becomes `при`, while `lf` (3.40) stays `lf`.
Single letters never yield.

For a provisional word the candidate is the whole key sequence read in Russian. Case is judged on
that reading. The English reading is the letters alone: the character chain scores them, and each
provisional unit before, inside or after the letters adds log10 of how often such punctuation
appears there in English words (−2.3, −1.6 and −1.0, measured on the EWT dev split). Only
punctuation at the end leaves an English core, and that core gets known-word protection, so
`it.` doesn't become `шею` and `he,` doesn't become `руб`. A word without letters is ineligible.

The engine abstains on unknown modes, unsupported or mixed-script tokens, mixed case,
multi-letter all-capital words and invalid policy values. A single initial capital is eligible
(`Ghbdtn` becomes `Привет`). Decision reasons are numbered for the reference vectors:

| Reason | Meaning |
|---|---|
| 0 | Correction |
| 1 | Ineligible token |
| 2 | Protected by personal vocabulary |
| 3 | Known word in the source language |
| 4 | Candidate forbidden by personal vocabulary |
| 5 | Evidence too weak |
| 6 | A configured dictionary is missing |
| 7 | Explicit manual or reversal intent |

Personal recognized and rare words are protected. A forbidden entry overrides personal and
general protection, and a forbidden candidate is rejected. Candidate membership doesn't supply a
frequency or justify a correction by itself. Mapped punctuation outside the target alphabet is
ineligible.

Both personal sources must be ready before automatic correction runs. A valid empty source is
ready; a missing one is not, though a host may publish a missing default-path file as empty
(macOS does, with `KR_SOURCE_MISSING_IS_EMPTY`). Manual conversion works regardless. Manual and
reversal intent stop reinference of that word; a typo fix alone doesn't.

### Context

Language context comes from the session's own verified finished words, at most 256 UTF-16 units,
never from reading the document. A finished word plus Space is appended on an acknowledged
finish; a still-reversible Space mark is not. Refusal, unknown effects, reset, mode changes and
unowned host actions clear it. So does printable punctuation, because the host's action on it
isn't acknowledged. The context scan takes the last two known same-language letter runs on the
current line, skipping non-letters and stopping at LF/CR; dropping punctuated context can remove
the protective factor of 19, so change that scan only with a new evaluation.

## Personal vocabulary

The engine reads Vim `.add` word lists ([`:help spell`](https://github.com/vim/vim/blob/master/runtime/doc/spell.txt)),
UTF-8, at most 1 MiB, line by line as Vim reads them. Supported: an initial BOM, LF/CRLF, blank
and comment lines, `/encoding=utf-8`, `/regions=`, and the flags `=`, `?`, `!` and `1` through
`9`. A line whose first byte is `#` is a comment whatever follows, because Vim's `zw`, `zug` and
`zuw` comment out an entry by overwriting that byte. A line that isn't UTF-8, an unknown
directive or flag, a word with a control character and other bad lines are skipped and counted
(`Vocabulary::skipped`). NUL anywhere, oversized input or a leading `/encoding=` other than
`utf-8` rejects the whole file. Unicode whitespace at line edges, including NBSP, is stripped.

A lowercase entry protects lowercase, capitalized and uppercase forms; a cased entry protects its
exact and uppercase forms; `/=` requires exact case. Across matching entries, forbidden (`!`) wins,
then recognized, then rare (`?`). A `/regions=` line and the region flags 1 through 9 are validated
and then dropped, so lookup doesn't depend on locale; the flags are accepted even without a
declared region name. A malformed `/regions=` line is skipped like any other bad line.

`vocabulary::Snapshot::replace` parses the whole candidate before publishing it. Readers keep the
old immutable `Arc` and a failed candidate never partially replaces it. `keyrill-ffi` reads the
files for the adapter; see [macOS](macos.md#dictionaries).

## Model

`assets/model.bin` holds 89,656 English and 241,624 Russian words with log10 frequencies, plus an
order-2 character chain per language. It is derived from wordfreq 3.1.1 large lists. The loader
checks the magic, version, source-hash field, alphabets, counts, offsets, word indices, ordering,
section bounds and trailing bytes, and validates once outside input callbacks. Header checks are
consistency, not authentication: `tools/pack-model.py --check` regenerates every asset from the
pinned source in `model/data/model.inc.gz` and compares bytes. Its docstring specifies the format.

The packer stores every value as a signed integer in micro-units and reproduces all 396,107
source values bit-exactly as f64. It doesn't retrain, recalibrate or renormalize anything. To
regenerate the source itself, see [`model/README.md`](../model/README.md).

## Evaluation

`./tools/verify evaluate` runs `model/tools/evaluate.py` against `tools/inference-probe`,
offline, over the pinned Universal Dependencies dev and test splits of English EWT and Russian
Taiga, and fails if a gate below fails. Reports, including every false correction, go to
`target/evaluation/docs/results/`. No personal dictionary is read. Parameters are chosen on the
dev split; the test split is only reported and gated.

The evaluation types text through the real `Session` the way the macOS adapter drives it: a word
starts only after whitespace or at the start of the document, a write is applied only if the
simulated composition holds exactly the plan's expected text and is acknowledged after it is
applied, and a mode change finishes the word and resets the session. A mismatch, or a passed key
that would land inside the composition, stops the evaluation with an error. Each sentence is one
document, typed in one go with a Space after it.

- **Wrong-layout replay.** Every sentence (`# text`) is typed on the keys of its own Apple layout
  while the other mode is active, so Russian `,` arrives as `^` in English mode and English `,`
  as `б` in Russian mode. Per language: *token recall* is the share of whitespace tokens that
  changed under the wrong layout and come out exactly as intended; *word recall* is the same over
  tokens that are letters only; *first-error recall* is the share of sentences whose first
  wrong token is fixed, the one that matters when a correction switches the input source. A
  token changed to anything else is a *miscorrection*.
- **Right-layout replay.** The same sentences typed correctly, each token in the mode of its
  script, so Latin words in Russian text switch modes. Any change is a false correction.
- **Negative set.** Isolated tokens, each typed as `token ` in a fresh session, so no context
  protects them: corpus words the model knows (`known`) and doesn't know (`oov`), proper nouns
  (`names`), simple Latin transliterations of the Russian words (`translit`), and every
  single-edit keyboard typo of the corpus words, made by deletion, transposition, or
  substituting or inserting a neighboring key, including the keys that are provisional letters
  (`typo`). The report also counts how many tokens reach the scoring step.

Results with the default policy:

| Split | RU token / first-error recall | EN token / first-error recall | Right-layout false | Typo false (per 10,000) | Other negatives false |
|---|---:|---:|---:|---:|---:|
| dev | 49.7% / 50.4% | 61.1% / 47.4% | 0 of 35,031 | 20 EN (0.80), 24 RU (0.60) | 1 translit of 5,197 |
| test | 46.1% / 47.7% | 60.5% / 47.0% | 0 of 33,899 | 17 EN (0.69), 21 RU (0.60) | 0 |

Correct corpus words, known or not, and names still never reach the scoring step, so they guard
against regressions but can't choose a threshold. Typos and transliterations do: about 350 of the
roughly 600,000 typos per split reach scoring, and they are what bounds the minimum candidate
Zipf. The known-word gate can only open for lexicon words, so its whole exposure is the list of
lexicon words whose other reading passes it, which the negative set below pins.

Most remaining misses have one of three causes. A word followed by punctuation never sees Space,
and only Space triggers inference (14% of Russian and 12% of English error tokens on dev). A
typed string is a lexicon word that doesn't yield, such as `lf` for да (16% and 10%). And
English candidates score below the length-adjusted threshold (10% of English).

The gates, in `tools/evaluate-model.py`, apply to both splits:

| Gate | Bound | Why |
|---|---:|---|
| Token recall, RU / EN | ≥ 44% / ≥ 58% | About two points under the lower split |
| First-error recall, RU and EN | ≥ 45% | About two points under the lower split |
| Right-layout false corrections, both languages | ≤ 2 | Measured 0; 2 of about 34,000 tokens is under 1 per 10,000 |
| Wrong-layout miscorrections | ≤ 2 | Measured 0 |
| Known, OOV and name false corrections | ≤ 2 | Measured 0 |
| Transliteration false corrections | ≤ 3 | Measured 1 on dev, 0 on test |
| Typo false corrections, per language | ≤ 1 per 10,000 | Measured 0.60 to 0.80 |

Errors are synthetic, typos and transliterations are generated, every sentence is typed without
Backspace or source switching, and wordfreq's sources may overlap these corpora. None of this
estimates precision on real typing. The release target is fewer than one false automatic
correction per 10,000 eligible correct words. No offline source with a redistribution license
covers real chat, code review, identifiers or Russian social text beyond Taiga; adding one means
pinning it by hash with its license next to the existing corpora.

A policy change needs the evaluation rerun on dev, the reference vectors still passing and the
negative set below unchanged except for cases the change is meant to open. Don't edit expected
values to remove failures.

## Reference tests

`crates/keyrill-core/tests/fixtures/engine-vectors.jsonl` has 1,217 expected outputs: 90 mapping,
120 vocabulary, 7 context and 1,000 inference requests. Inference scores are compared with a
relative tolerance of 1e-12 and an absolute tolerance of 1e-10. The request protocol:

| Request | Response |
|---|---|
| `map K MODE TEXT` | MODE 0 manual, 1 EN→RU, 2 RU→EN; output as UTF-16 hex |
| `vocab SOURCE WORD` | Parse success 0/1, entry count, lookup (0 absent, 1 recognized, 2 rare, 3 forbidden) |
| `context PREFIX` | Language tag |
| `infer K ACTIVE CONTEXT PRIOR COST MINZIPF EN_ADD RU_ADD WORD` | Reason, score, replacement hex or `-` |

Hex strings are UTF-16 units, `-` is empty, keyboards are 0 Apple and 1 PC, languages are
0 unknown, 1 EN, 2 RU. [`negative-words.md`](../crates/keyrill-core/tests/fixtures/negative-words.md)
describes 766 words and model collisions that must never be corrected, run under three contexts
and both keyboards. Inference requests set three policy values; the known-word gate takes its
defaults.
