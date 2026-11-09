#!/usr/bin/env python3
"""Check that every code excerpt in docs/learn, English and Russian, still matches its source.

An excerpt is a fenced block whose caption, on the line before a blank line and the fence, links
to the lines it copies:

    [`crates/keyrill-core/src/session.rs:164-182`](../../crates/keyrill-core/src/session.rs#L164-L182)

The block must equal those lines of the file in the working tree, and the link must name the same
path and lines. A Rust, Objective-C, Objective-C++ or C block without a caption fails, so the
walkthrough holds no copy of the source that this check doesn't see.
"""

from pathlib import Path
import posixpath
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
CHAPTERS = sorted((ROOT / "docs/learn").glob("*.md"))
SOURCE_LANGUAGES = {"rust", "c", "objc", "obj-c++"}

FENCE = re.compile(r"^(`{3,})([\w+-]*)\n(.*?)^\1$", re.DOTALL | re.MULTILINE)
CAPTION = re.compile(r"\[`([^`:]+):(\d+)(?:-(\d+))?`\]\(([^)#]+)#L(\d+)(?:-L(\d+))?\)\n\n\Z")


def check(chapter):
    text = chapter.read_text(encoding="utf-8")
    name = chapter.relative_to(ROOT).as_posix()
    errors, count = [], 0
    for fence in FENCE.finditer(text):
        language, body = fence.group(2), fence.group(3)
        line = text.count("\n", 0, fence.start()) + 1
        caption = CAPTION.search(text, 0, fence.start())
        if not caption:
            if language in SOURCE_LANGUAGES:
                errors.append(f"{name}:{line}: {language} block without a source caption")
            continue
        path, first, last, href, href_first, href_last = caption.groups()
        last, href_last = last or first, href_last or href_first
        # Links use forward slashes on every OS, so the target is normalized as a POSIX path
        target = posixpath.normpath(posixpath.join(posixpath.dirname(name), href))
        if target != path or (first, last) != (href_first, href_last):
            errors.append(f"{name}:{line}: caption {path}:{first}-{last} links elsewhere")
            continue
        lines = (ROOT / path).read_text(encoding="utf-8").split("\n")
        if body.split("\n")[:-1] != lines[int(first) - 1 : int(last)]:
            errors.append(f"{name}:{line}: excerpt differs from {path}:{first}-{last}")
        count += 1
    return errors, count


def main():
    errors, count = [], 0
    for chapter in CHAPTERS:
        chapter_errors, chapter_count = check(chapter)
        errors += chapter_errors
        count += chapter_count
    if errors:
        sys.exit("\n".join(errors))
    print(f"{count} excerpts match their sources")


if __name__ == "__main__":
    main()
