# Negative words

`negative-words.jsonl` lists 766 curated correct tokens and model-collision cases that automatic
correction must leave unchanged. `../negative_words.rs` runs every case under no context, English
context and Russian context, with both Apple and PC Russian keyboards, against the frozen model and
present but empty personal overlays.

This is a regression set. It is not a held-out sample, does not estimate precision on real typing,
and was not used to tune any threshold or policy value.

## Cases

| Category | Cases | Content |
| -- | -- | -- |
| `en_technical` | 176 | Commands, languages, tools and hardware-design terms typed as plain letters |
| `en_names` | 51 | Companies, people and places, capitalized |
| `en_acronyms` | 25 | All-capital and capitalized acronyms |
| `en_short` | 56 | One- to three-letter words, abbreviations and units |
| `ru_prose` | 24 | Technical Russian prose words |
| `ru_slang` | 26 | Russian engineering slang and transliterations |
| `ru_names` | 18 | Russian names and places, including ё |
| `ru_short` | 29 | One- and two-letter Russian words and particles |
| `ru_acronyms` | 8 | Russian all-capital abbreviations |
| `en_in_ru_context` | 19 | English terms after a Russian prefix |
| `ru_in_en_context` | 9 | Russian words after an English prefix |
| `en_in_en_context`, `ru_in_ru_context` | 4 + 4 | Same-language prefix controls |
| `non_letter` | 7 | Tokens with digits or punctuation; ineligible before inference |
| `en_punctuated` | 40 | English words, quotes and lone punctuation using the keys that are provisional letters in English mode |
| `en_trailing_collision` | 108 | Every English model word with Zipf ≥ 3 that, followed by one of `, . ; ' [ ] \`, reads as a Russian word with Zipf ≥ 3, such as `it.` for шею |
| `cross_layout_collision` | 162 | Every model word, in either language, whose wrong-layout reading is a target-language word with Zipf ≥ 4.5, except rare entries the known-word gate lets yield |

All tokens except the two collision categories were written for this set from general knowledge;
none come from user input, a personal dictionary or private text. The collision categories are
derived mechanically from `assets/model.bin`; `cross_layout_collision` is shown with the prefix that
favors conversion. The known-word gate lets rare lexicon entries such as `yf`, `ghb` and `nfr` yield
to на, при and так; those entries are wrong-layout artifacts in the frequency lists, not words to
protect, and are not in the set. The derived subsets retain the model data CC-BY-SA-4.0 terms and
[asset notices](../../../../assets/README.md). Prefix cases assert that `detect_context` recognizes
the declared language, so a context case cannot silently degrade to a neutral one.

`expect` is `unchanged` for every case. Add a false-change finding to the set with its
context/keyboard condition. It remains a failing gate until resolved; do not remove the case or
convert it into a passing expected failure.

## What the set constrains

Removing known-word protection fails the test: collision words such as `lf` and `ns` are then
corrected under Russian context. Raising the known-word ceiling or lowering its margin fails the
collision category. No curated case reaches the scoring step: each stops at eligibility, known-word
protection or an absent or infrequent candidate. Changing the false correction cost, minimum
candidate Zipf or short-word penalty therefore does not fail this set. Use model-backed session
regressions and the [evaluation](../../../../docs/engine.md#evaluation) to constrain those values.

## Limits

One author selected the curated tokens. Slang, brand names, transliterations and abbreviations
outside both models remain the likeliest false changes and are only partly covered. Whole-token
eligibility and native collection are separate contracts. These direct inference cases do not prove
that a native adapter cannot acquire a prefix or suffix of a technical token.
