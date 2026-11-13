#!/usr/bin/env python3
"""Run the offline evaluator against the inference probe and check the release gates."""

import argparse
import importlib.util
from pathlib import Path
import shutil
import sys

# Loading model/tools/evaluate.py as a module would otherwise leave __pycache__ in model/tools
sys.dont_write_bytecode = True

# Floors were set about two points below the original policy's lower split; the limits stay
# unchanged when tuning the policy. Typo bounds stay under one false correction per 10,000
# tokens. See docs/engine.md#evaluation.
RECALL_FLOORS = {
    "wrong/en": {"token_recall": 0.58, "first_error_recall": 0.45},
    "wrong/ru": {"token_recall": 0.44, "first_error_recall": 0.45},
}
FALSE_BOUNDS = {
    "right replays": (["right/en", "right/ru"], "false_corrections", 2),
    "wrong-layout miscorrections": (["wrong/en", "wrong/ru"], "miscorrections", 2),
    "corpus words and names": ([f"negative/{kind}/{lang}" for kind in ["known", "oov", "names"]
                                for lang in ["en", "ru"]], "false_corrections", 2),
    "transliterations": (["negative/translit/ru"], "false_corrections", 3),
}
TYPO_BOUND_PER_10000 = 1.0

root = Path(__file__).resolve().parent.parent
reference = root / "model"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--split", choices=["dev", "test"], required=True)
parser.add_argument("--probe", type=Path, default=root / "target/release/inference-probe")
parser.add_argument("--output", type=Path, default=root / "target/evaluation")
parser.add_argument("policy", nargs="*", help="NAME=VALUE policy overrides")
args = parser.parse_args()
output = args.output.resolve()
for directory in [output / "data", output / ".cache/evaluation", output / "docs/results"]:
    directory.mkdir(parents=True, exist_ok=True)
for name in ["evaluation-sources.json", "model-manifest.json"]:
    shutil.copyfile(reference / "data" / name, output / "data" / name)
for corpus in (reference / "corpora").glob("*.conllu"):
    shutil.copyfile(corpus, output / ".cache/evaluation" / corpus.name)
# evaluate.py reads its sources and writes its report under ROOT; pointing ROOT at a copy in
# target/ keeps the tracked model/ tree unchanged
spec = importlib.util.spec_from_file_location("keyrill_reference_evaluation", reference / "tools/evaluate.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.ROOT = output
metrics = module.evaluate(args.split, args.probe.resolve(), args.policy)["metrics"]
for key, value in metrics.items():
    if "token_recall" in value:
        print(f"{args.split} {key:<16} tokens {value['error_tokens']:>6} recall {value['token_recall']:6.1%} "
              f"words {value['word_recall']:6.1%} first {value['first_error_recall']:6.1%} "
              f"miscorrected {value.get('miscorrections', 0):>3} false {value.get('false_corrections', 0):>3}")
    else:
        print(f"{args.split} {key:<20} tokens {value.get('unchanged_tokens') or value['tokens']:>7} "
              f"scored {value.get('reached_scoring', '-'):>6} false {value.get('false_corrections', 0):>3}")
failures = []
for key, floors in RECALL_FLOORS.items():
    for name, floor in floors.items():
        if metrics[key][name] < floor:
            failures.append(f"{key} {name} {metrics[key][name]:.1%} < {floor:.0%}")
for label, (keys, name, bound) in FALSE_BOUNDS.items():
    total = sum(metrics[key].get(name, 0) for key in keys)
    if total > bound:
        failures.append(f"{label}: {total} > {bound}")
for lang in ["en", "ru"]:
    rate = metrics[f"negative/typo/{lang}"]["false_per_10000"]
    if rate > TYPO_BOUND_PER_10000:
        failures.append(f"typo/{lang}: {rate:.2f} per 10,000 > {TYPO_BOUND_PER_10000}")
if failures:
    sys.exit(f"{args.split} evaluation gate failed: " + "; ".join(failures))
