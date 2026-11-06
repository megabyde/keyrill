# ADR 003: precision before recall

Status: accepted principle; scoring parameters are provisional.

Mapping is deterministic; the decision to change a word automatically is probabilistic and must be
conservative. The engine protects general and personal vocabulary and abstains on technical,
mixed-case, mixed-script, unknown and weakly supported tokens. Manual conversion is always
available. Dictionary membership alone never justifies a change. The one place frequencies are
compared across languages is the known-word gate: a typed string that is a rare word in its own
layout (below Zipf 3) yields only to a reading at least 2.5 Zipf more frequent, and a 2-letter word
needs 3.5 without context. That opens junk entries such as `yf` for на without opening real words.

Replaying the pinned test split typed in the wrong layout, the model fixes 46.1% of Russian and
60.5% of English tokens, with no false corrections among 33,899 tokens typed correctly. That doesn't
measure precision on real typing: errors are synthetic, typos and transliterations are generated,
tokens are correlated, and correctly typed corpus words never reach the scoring step, so only the
typo and transliteration strata bound the thresholds. No cloud or LLM dependency is justified.

Change scoring only with versioned calibration data, held-out real and technical text, measured
false-change cost and length, direction and context strata. Don't trade protection for recall
because an aggregate synthetic metric improved.
