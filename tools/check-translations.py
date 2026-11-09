#!/usr/bin/env python3
"""Check that every Russian page translates the current version of its English source.

The English chapters are the ones site/SUMMARY.md lists, plus the site's own SUMMARY.md and
api.md. Each has a Russian counterpart beside it with the suffix .ru.md (in site/ru/ for the site's
pages), whose first line names the source and the git blob hash of the version it translates:

    <!-- translation of docs/engine.md at 3b18e512dba79e4c8caf3db8d1df2a3b7c7b6a3e -->

The check fails on a missing translation, on one whose source has changed since that hash, and on
a Russian page with no English source. Update the translation with its source and write the new
hash from `git hash-object <source>`.
"""

from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
CHAPTER = re.compile(r"\]\(([^)]+\.md)\)")
HEADER = re.compile(r"<!-- translation of (\S+) at ([0-9a-f]{40}) -->")


def sources():
    """Map each English source to the path of its translation."""
    summary = (ROOT / "site/SUMMARY.md").read_text(encoding="utf-8")
    pairs = {path: path.removesuffix(".md") + ".ru.md" for path in CHAPTER.findall(summary) if (ROOT / path).is_file()}
    pairs.update({f"site/{name}": f"site/ru/{name}" for name in ("SUMMARY.md", "api.md")})
    return pairs


def main():
    pairs = sources()
    errors = []
    for source, translation in sorted(pairs.items()):
        path = ROOT / translation
        if not path.is_file():
            errors.append(f"{translation}: missing translation of {source}")
            continue
        header = HEADER.fullmatch(path.read_text(encoding="utf-8").split("\n", 1)[0])
        current = subprocess.run(["git", "hash-object", source], cwd=ROOT, check=True,
                                 capture_output=True, text=True).stdout.strip()
        if not header or header.group(1) != source:
            errors.append(f"{translation}: first line must be <!-- translation of {source} at HASH -->")
        elif header.group(2) != current:
            errors.append(f"{translation}: {source} changed since {header.group(2)[:12]}; "
                          f"update the translation and the hash to {current[:12]}")
    known = set(pairs.values())
    listed = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard",
                             "*.ru.md", "site/ru/*.md"], cwd=ROOT, check=True, capture_output=True,
                            text=True).stdout.split()
    for path in sorted(listed):
        if path not in known:
            errors.append(f"{path}: no English source in site/SUMMARY.md")
    if errors:
        sys.exit("\n".join(errors))
    print(f"{len(pairs)} translations match their sources")


if __name__ == "__main__":
    main()
