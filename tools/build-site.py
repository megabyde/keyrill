#!/usr/bin/env python3
"""Build the documentation site into target/site/book.

The sources stay where GitHub shows them. This script copies the chapters listed in
site/SUMMARY.md into target/site/src under their repository paths, so a relative link between two
chapters means the same thing in both places. Two things must change for mdBook: a link to a file
outside the book becomes a GitHub URL, and a `math` fence, which only GitHub renders, becomes a
MathJax display block. The script stops on a link to a path that does not exist.
"""

import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "target/site"
SOURCE = SITE / "src"
BOOK = SITE / "book"
BLOB = "https://github.com/megabyde/keyrill/blob/main/"

CHAPTER = re.compile(r"\]\(([^)]+\.md)\)")
LINK = re.compile(r"\]\(([^)#\s]+)(#[^)\s]*)?\)")
MATH = re.compile(r"^```math\n(.*?)^```$", re.DOTALL | re.MULTILINE)


def chapters():
    """Return the repository paths of the chapters, skipping the site's own pages."""
    summary = (ROOT / "site/SUMMARY.md").read_text(encoding="utf-8")
    return [path for path in CHAPTER.findall(summary) if (ROOT / path).is_file()]


def rewrite_link(match, chapter, staged):
    target, anchor = match.group(1), match.group(2) or ""
    if re.match(r"[a-z]+:", target):
        return match.group(0)
    resolved = os.path.normpath(PurePosixPath(chapter).parent / target)
    if f"{resolved}/README.md" in staged:
        return f"]({target.rstrip('/')}/index.html{anchor})"
    if resolved in staged:
        # mdBook publishes a README.md chapter as index.html and leaves links to it alone
        if PurePosixPath(resolved).name == "README.md":
            return f"]({target.removesuffix('README.md')}index.html{anchor})"
        return match.group(0)
    if (ROOT / resolved).exists():
        return f"]({BLOB}{resolved}{anchor})"
    sys.exit(f"{chapter}: broken link to {target}")


def stage():
    shutil.rmtree(SITE, ignore_errors=True)
    SOURCE.mkdir(parents=True)
    shutil.copy(ROOT / "site/book.toml", SITE)
    shutil.copy(ROOT / "site/SUMMARY.md", SOURCE)
    shutil.copy(ROOT / "site/api.md", SOURCE)

    staged = set(chapters())
    for chapter in staged:
        text = (ROOT / chapter).read_text(encoding="utf-8")
        text = LINK.sub(lambda match: rewrite_link(match, chapter, staged), text)
        # An HTML block keeps Markdown away from the backslashes and underscores of the formula
        text = MATH.sub(lambda match: f"<div>\n\\[\n{match.group(1)}\\]\n</div>\n", text)
        destination = SOURCE / chapter
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(text, encoding="utf-8")


def main():
    stage()
    subprocess.run(["mdbook-mermaid", "install", SITE], check=True)
    subprocess.run(["mdbook", "build", SITE], check=True)
    subprocess.run(["cargo", "doc", "--no-deps", "--locked", "-p", "keyrill-core", "-p",
                    "keyrill-ffi"], cwd=ROOT, check=True)
    shutil.copytree(ROOT / "target/doc", BOOK / "api", ignore=shutil.ignore_patterns(".lock"))


if __name__ == "__main__":
    main()
