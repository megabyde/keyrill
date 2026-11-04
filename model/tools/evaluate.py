import argparse
import collections
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
SOURCES = {
    "en": ("UD_English-EWT", "4c89b5833a70aa5ed3a00bad2f23f57992cc7df8", "en_ewt"),
    "ru": ("UD_Russian-Taiga", "23bc6ded75cb717aaf233b8568003fa0897e56a6", "ru_taiga"),
}
ALPHABETS = {"en": "abcdefghijklmnopqrstuvwxyz", "ru": "абвгдеёжзийклмнопрстуфхцчшщъыьэюя"}
OTHER = {"en": "ru", "ru": "en"}
# Apple U.S. and Apple Russian layouts on the ANSI keys, row by row: unshifted, then shifted.
# Read from the system layouts with UCKeyTranslate; Space and Tab are the same in both.
LAYOUTS = {
    "en": ["`1234567890-=", "qwertyuiop[]\\", "asdfghjkl;'", "zxcvbnm,./",
           "~!@#$%^&*()_+", "QWERTYUIOP{}|", "ASDFGHJKL:\"", "ZXCVBNM<>?"],
    "ru": ["]1234567890-=", "йцукенгшщзхъё", "фывапролджэ", "ячсмитьбю/",
           "[!\"№%:,.;()_+", "ЙЦУКЕНГШЩЗХЪЁ", "ФЫВАПРОЛДЖЭ", "ЯЧСМИТЬБЮ?"],
}
KEYS = {lang: {char: (row, column) for row, keys in enumerate(rows) for column, char in enumerate(keys)}
        for lang, rows in LAYOUTS.items()}
TRANSLIT = dict(zip("абвгдеёжзийклмнопрстуфхцчшщъыьэюя",
                    "a b v g d e yo zh z i y k l m n o p r s t u f h ts ch sh sch - y - e yu ya".split()))


def verify_source(path):
    records = json.loads((ROOT / "data/evaluation-sources.json").read_text(encoding="utf-8"))
    matches = [record for record in records if record["file"] == path.name]
    if len(matches) != 1:
        raise RuntimeError(f"missing or duplicate source record: {path.name}")
    record = matches[0]
    payload = path.read_bytes()
    if len(payload) != record["bytes"] or hashlib.sha256(payload).hexdigest() != record["sha256"]:
        raise RuntimeError(f"corpus hash mismatch: {path.name}; remove it and download the pinned source again")
    return record


def download():
    directory = ROOT / ".cache" / "evaluation"
    directory.mkdir(parents=True, exist_ok=True)
    for lang, (repo, commit, prefix) in SOURCES.items():
        for name in ["README.md", "LICENSE.txt", f"{prefix}-ud-dev.conllu", f"{prefix}-ud-test.conllu"]:
            url = f"https://raw.githubusercontent.com/UniversalDependencies/{repo}/{commit}/{name}"
            path = directory / f"{lang}-{name}"
            if not path.exists():
                with urllib.request.urlopen(url, timeout=60) as response:
                    payload = response.read(32 * 1024 * 1024 + 1)
                if len(payload) > 32 * 1024 * 1024:
                    raise RuntimeError("corpus exceeds download limit")
                path.write_bytes(payload)
            if verify_source(path)["url"] != url:
                raise RuntimeError(f"source revision mismatch: {path.name}")


def hex16(text):
    return text.encode("utf-16-be", "surrogatepass").hex() or "-"


def unhex16(text):
    return "" if text == "-" else bytes.fromhex(text).decode("utf-16-be", "surrogatepass")


def corpus(path):
    """Sentence texts and proper-noun forms of one CoNLL-U file."""
    sentences, names = [], []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("# text = "):
            sentences.append(line[len("# text = "):])
            continue
        columns = line.split("\t")
        if len(columns) == 10 and columns[0].isdigit() and columns[3] == "PROPN":
            names.append(columns[1])
    return sentences, names


def wrong_layout(text, lang):
    """The keys for `text` in the `lang` layout, read in the other layout."""
    other = LAYOUTS[OTHER[lang]]
    typed = []
    for char in text:
        key = KEYS[lang].get(char)
        typed.append(other[key[0]][key[1]] if key else char)
    return "".join(typed)


def script(token):
    if any(char.lower() in ALPHABETS["ru"] for char in token):
        return "ru"
    if any(char.lower() in ALPHABETS["en"] for char in token):
        return "en"
    return None


def segments(text, default):
    """Right-layout typing: each token in the mode of its script, spaces in the current one."""
    parts, mode = [], default
    for token in text.split(" "):
        mode = script(token) or mode
        if parts and parts[-1][0] == mode:
            parts[-1][1].append(token)
        else:
            if parts:
                parts[-1][1].append("")
            parts.append((mode, [token]))
    return [(mode, " ".join(tokens)) for mode, tokens in parts]


def neighbors(lang):
    rows = LAYOUTS[lang][1:4]
    near = collections.defaultdict(set)
    for row, keys in enumerate(rows):
        for column, char in enumerate(keys):
            for r, c in [(row, column - 1), (row, column + 1), (row - 1, column), (row - 1, column + 1),
                         (row + 1, column - 1), (row + 1, column)]:
                if 0 <= r < len(rows) and 0 <= c < len(rows[r]):
                    near[char].add(rows[r][c])
    return near


def typos(word, near):
    """Single-edit keyboard typos: deletion, transposition, neighbor substitution or insertion."""
    out = set()
    for i, char in enumerate(word):
        out.add(word[:i] + word[i + 1:])
        if i + 1 < len(word):
            out.add(word[:i] + word[i + 1] + char + word[i + 2:])
        for other in near[char]:
            out.add(word[:i] + other + word[i + 1:])
            out.add(word[:i + 1] + other + word[i + 1:])
    out.discard(word)
    return {typo for typo in out if len(typo) > 1}


def run_probe(probe, kind, lines, policy):
    result = subprocess.run([str(probe), kind, *policy], input="".join(lines),
                            text=True, capture_output=True, check=True)
    output = result.stdout.splitlines()
    if len(output) != len(lines):
        raise RuntimeError("probe result count mismatch")
    return output


def evaluate(split, probe, policy=()):
    texts, names = {}, {}
    for lang, (_, _, prefix) in SOURCES.items():
        path = ROOT / ".cache/evaluation" / f"{lang}-{prefix}-ud-{split}.conllu"
        verify_source(path)
        texts[lang], names[lang] = corpus(path)

    # Whole sentences, typed as running text with a Space after each
    replays = []
    for lang in SOURCES:
        for text in texts[lang]:
            text = text + " "
            typed = wrong_layout(text, lang)
            replays.append(("wrong", lang, text, typed, [(OTHER[lang], typed)]))
            replays.append(("right", lang, text, text, segments(text, lang)))
    output = run_probe(probe, "session", [
        " ".join(f"{mode}:{hex16(units)}" for mode, units in parts if units) + "\n"
        for *_, parts in replays], policy)
    counts = collections.defaultdict(collections.Counter)
    false_corrections = []
    for (kind, lang, intended, typed, _), line in zip(replays, output):
        result = unhex16(line)
        expected, before, after = intended.split(), typed.split(), result.split()
        if not len(expected) == len(before) == len(after):
            raise RuntimeError("replay changed token boundaries")
        tally = counts[f"{kind}/{lang}"]
        tally["sentences"] += 1
        first = None
        for want, had, got in zip(expected, before, after):
            if had == want:
                tally["unchanged_tokens"] += 1
                if got != want:
                    tally["false_corrections"] += 1
                    false_corrections.append({"replay": kind, "language": lang, "typed": had, "output": got})
                continue
            tally["error_tokens"] += 1
            letters = all(char.lower() in ALPHABETS[lang] for char in want)
            tally["error_words"] += letters
            if got == want:
                tally["fixed_tokens"] += 1
                tally["fixed_words"] += letters
            elif got != had:
                tally["miscorrections"] += 1
                false_corrections.append({"replay": kind, "language": lang, "typed": had, "output": got,
                                          "intended": want})
            if first is None:
                first = got == want
        if first is not None:
            tally["sentences_with_errors"] += 1
            tally["first_error_fixed"] += first

    # Correct isolated tokens that must stay as typed, each typed as `token ` in its own mode
    negatives = collections.defaultdict(set)
    for lang in SOURCES:
        alphabet = set(ALPHABETS[lang])
        near = neighbors(lang)
        words = {token for text in texts[lang] for token in text.split()
                 if token.isalpha() and set(token.lower()) <= alphabet}
        negatives[f"corpus/{lang}"] = {(lang, word) for word in words}
        negatives[f"typo/{lang}"] = {(lang, typo) for word in words for typo in typos(word.lower(), near)}
        negatives[f"names/{lang}"] = {(lang, name) for name in names[lang]
                                      if name.isalpha() and set(name.lower()) <= alphabet}
        if lang == "ru":
            for word in words:
                latin = "".join(TRANSLIT[char.lower()] for char in word).replace("-", "")
                if latin:
                    negatives["translit/ru"].add(("en", latin.title() if word[0].isupper() else latin))
    cases = sorted({case for stratum in negatives.values() for case in stratum})
    lines = [f"{mode} {hex16(word)} -\n" for mode, word in cases]
    reasons = {case: line.split("\t") for case, line in zip(cases, run_probe(probe, "word", lines, policy))}
    # With the known-word gate closed, reason 3 is exactly membership in the model's lexicon
    lexicon = {case for case, line in zip(cases, run_probe(probe, "word", lines, [*policy, "known_ceiling=0"]))
               if line.startswith("3\t")}
    for lang in SOURCES:
        words = negatives.pop(f"corpus/{lang}")
        negatives[f"known/{lang}"] = words & lexicon
        negatives[f"oov/{lang}"] = words - lexicon
    cases = sorted({case for stratum in negatives.values() for case in stratum})
    output = run_probe(probe, "session", [f"{mode}:{hex16(word + ' ')}\n" for mode, word in cases], policy)
    changed = {case for case, line in zip(cases, output) if unhex16(line) != case[1] + " "}
    for name, stratum in sorted(negatives.items()):
        tally = counts[f"negative/{name}"]
        tally["tokens"] = len(stratum)
        # Reason 0 or a Weak decision with a computed score
        tally["reached_scoring"] = sum(reasons[case][0] == "0" or
                                       (reasons[case][0] == "5" and float(reasons[case][1]) != 0.0)
                                       for case in stratum)
        tally["false_corrections"] = len(stratum & changed)
        false_corrections += [{"stratum": name, "typed": word, "output": unhex16(reasons[(mode, word)][2])}
                              for mode, word in sorted(stratum & changed)]

    metrics = {}
    for key, value in sorted(counts.items()):
        metric = dict(value)
        if value["error_tokens"]:
            metric["token_recall"] = value["fixed_tokens"] / value["error_tokens"]
            metric["word_recall"] = value["fixed_words"] / value["error_words"]
            metric["first_error_recall"] = value["first_error_fixed"] / value["sentences_with_errors"]
        negatives_ = value["unchanged_tokens"] or value["tokens"]
        metric["false_per_10000"] = value["false_corrections"] / negatives_ * 10000 if negatives_ else None
        metrics[key] = metric
    report = {
        "split": split, "metrics": metrics, "false_corrections": false_corrections, "policy": list(policy),
        "provenance": {
            "evaluator_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "source_manifest_sha256": hashlib.sha256((ROOT / "data/evaluation-sources.json").read_bytes()).hexdigest(),
            "model_manifest_sha256": hashlib.sha256((ROOT / "data/model-manifest.json").read_bytes()).hexdigest(),
            "probe_sha256": hashlib.sha256(probe.read_bytes()).hexdigest(),
        },
        "limitations": [
            "Layout errors are synthetic: whole corpus sentences typed in the other Apple layout.",
            "Typos and transliterations are generated from corpus words; they are not observed typing.",
            "Each sentence is typed straight through; Backspace, manual conversion and source switching are not modeled.",
            "Wordfreq's aggregate sources may overlap these corpora; independent source provenance is unproven.",
            "No private vocabulary or user input is read by this evaluation.",
            "The prior, cost and penalties are ranking hypotheses, not calibrated probabilities.",
        ],
    }
    suffix = "-".join(policy).replace("=", "") or "default"
    output_path = ROOT / "docs/results" / f"session-{split}-{suffix}.json"
    output_path.write_text(json.dumps(report, ensure_ascii=False, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--download", action="store_true")
    parser.add_argument("--split", choices=["dev", "test"], default="dev")
    parser.add_argument("--probe", type=Path, default=ROOT / "build/inference_probe")
    parser.add_argument("policy", nargs="*", help="NAME=VALUE policy overrides")
    args = parser.parse_args()
    if args.download:
        download()
    else:
        print(json.dumps(evaluate(args.split, args.probe, args.policy)["metrics"], indent=1))
