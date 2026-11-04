# Model source and evaluation

Build-time data and tools behind `assets/model.bin`. Nothing here is read at runtime. Every file
is byte-pinned: the top-level `tools/pack-model.py` checks the model source by hash,
`evaluate.py` checks the corpora against `data/evaluation-sources.json`, and
`data/model-manifest.json` records the hashes of the generator and its locked requirements. Don't
reformat, re-encode or trim anything here; `.gitattributes` keeps Git from converting line endings
or whitespace.

| Path | Contents |
|---|---|
| `data/model.inc.gz` | Word frequencies and order-2 character chains for EN and RU, gzip-compressed |
| `data/model-manifest.json` | Generator, requirements, Python and source hashes; package versions; model parameters |
| `data/model-notice.txt` | CC BY-SA 4.0 attribution for the wordfreq-derived data |
| `data/wordfreq-code-license.txt` | wordfreq's Apache-2.0 code notice; it doesn't change the data license |
| `data/evaluation-sources.json` | Pinned upstream URL, size and SHA-256 of every corpus file |
| `corpora/` | UD English EWT and UD Russian Taiga dev/test splits with their READMEs and licenses |
| `tools/build_model.py` | Generates `model.inc` from wordfreq 3.1.1 |
| `tools/model-requirements.{in,txt}` | Hash-locked Python environment for the generator |
| `tools/evaluate.py` | Session replay and negative-set evaluator, run by the top-level `tools/evaluate-model.py`, which also checks the gates |

The vocabulary is wordfreq's unfiltered usage data, so it includes profanity and slurs. That is
deliberate: the engine never suggests a word, it only decides whether to read the user's own
keystrokes in the other layout, and a filtered word typed in the wrong layout would stay
uncorrected.

To inspect the source, decompress it and compare the plaintext SHA-256 with `model_sha256` in the
manifest; the compressed file's hash is a different value.

```console
$ python3 -c 'import gzip,pathlib; p=pathlib.Path("model/data/model.inc.gz"); p.with_suffix("").write_bytes(gzip.decompress(p.read_bytes()))'
$ shasum -a 256 model/data/model.inc
```

To regenerate, create a separate virtual environment, install `tools/model-requirements.txt` with
`--require-hashes`, and run `tools/build_model.py`. It writes `model.inc` and `short-words.json`
into `data/`; compress the first with gzip and compare hashes. A different Python version changes
the manifest even when the model bytes agree. Then update the pinned hashes in
the top-level `tools/pack-model.py` and rerun the evaluation.

The evaluator talks to `tools/inference-probe` over stdin, one response line per request line,
with four hex digits per UTF-16 unit and `-` for empty. The first argument picks the protocol;
optional `NAME=VALUE` arguments override fields of the default `Policy`, such as
`minimum_zipf=3.5`, for parameter sweeps on the dev split.

| Protocol | Request line | Response line |
|---|---|---|
| `word` | `en\|ru WORD_HEX CONTEXT_HEX` | `REASON<TAB>LOG10_SCORE<TAB>REPLACEMENT_HEX` |
| `session` | `MODE:HEX` segments separated by spaces | The document after typing every segment, as hex |

`session` types each segment unit by unit in its mode (`en` or `ru`) through a fresh `Session`,
acting as an adapter that verifies every plan, starts a word only after whitespace and finishes
and resets the session when the mode changes, and finishes the word at the end. The evaluation
generates its typos and transliterations from the pinned corpora at run time, so it adds no data
files and needs no network.

## Licenses

The derived frequency and character data is CC BY-SA 4.0, from wordfreq 3.1.1 by Robyn Speer;
keep `model-notice.txt` and its source credits with the data and any binary that embeds it.
EWT and Taiga are CC BY-SA 4.0 with their own READMEs and license files in `corpora/`. The EWT
README links to files in its upstream repository that aren't copied here; see the
[pinned EWT source](https://github.com/UniversalDependencies/UD_English-EWT/tree/4c89b5833a70aa5ed3a00bad2f23f57992cc7df8).
