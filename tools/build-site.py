#!/usr/bin/env python3
"""Build the documentation site into target/site/book.

The sources stay where GitHub shows them. The site has two books, English from site/ and Russian
from site/ru/; each Russian chapter sits beside its English source as X.ru.md. This script copies
the chapters each SUMMARY.md lists into a staging directory under their paths within the book,
which drop the .ru suffix, so both books publish the same page paths. Four things must change for
mdBook: a link between two Russian chapters loses the suffix, a link to a chapter of the other book
points at its published page, a link to a file outside both books becomes a GitHub URL, and a
`math` fence, which only GitHub renders, becomes a MathJax display block. The script stops on a link to a path that does not exist.
mdBook titles the GitHub alert boxes in English, so the Russian pages get the titles translated,
and each page's header gets a link to the same page in the other book.
"""

import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "target/site"
BOOK = SITE / "book"
BLOB = "https://github.com/megabyde/keyrill/blob/main/"

# Configuration directory, chapter suffix, published subdirectory
BOOKS = (("site", ".md", ""), ("site/ru", ".ru.md", "ru"))

CHAPTER = re.compile(r"\]\(([^)]+\.md)\)")
LINK = re.compile(r"\]\(([^)#\s]+)(#[^)\s]*)?\)")
MATH = re.compile(r"^```math\n(.*?)^```$", re.DOTALL | re.MULTILINE)

# Alert box titles by published subdirectory
ALERT_TITLES = {"ru": {"Note": "Примечание", "Tip": "Совет", "Important": "Важно",
                       "Warning": "Внимание", "Caution": "Осторожно"}}
ALERT_TITLE = re.compile(r'(<p class="blockquote-tag-title">.*?</svg>)(\w+)(</p>)', re.DOTALL)
# Header link to the other book: label, title and language of the page it opens
SWITCH = {"": ("RU", "Русская версия", "ru"), "ru": ("EN", "English version", "en")}
HEADER = '<div class="right-buttons">'


def chapters(config):
    """Return the repository paths of a book's chapters, skipping the site's own pages."""
    summary = (ROOT / config / "SUMMARY.md").read_text(encoding="utf-8")
    return [path for path in CHAPTER.findall(summary) if (ROOT / path).is_file()]


def book_path(chapter, suffix):
    """The path of a chapter within its book: the repository path without the language suffix."""
    return chapter.removesuffix(suffix) + ".md" if chapter.endswith(suffix) else chapter


def page(path):
    """The published page of a chapter path within its book."""
    if PurePosixPath(path).name == "README.md":
        return str(PurePosixPath(path).parent / "index.html")
    return path.removesuffix(".md") + ".html"


def rewrite_link(match, chapter, book, pages):
    target, anchor = match.group(1), match.group(2) or ""
    if re.match(r"[a-z]+:", target):
        return match.group(0)
    resolved = os.path.normpath(PurePosixPath(chapter).parent / target)
    for candidate in (f"{resolved}/README.md", resolved):
        if candidate not in pages:
            continue
        other, path = pages[candidate]
        if other == book:
            relative = os.path.relpath(path, PurePosixPath(pages[chapter][1]).parent)
            # mdBook publishes a README.md chapter as index.html and leaves links to it alone
            if PurePosixPath(path).name == "README.md":
                relative = relative.removesuffix("README.md") + "index.html"
            return f"]({relative}{anchor})"
        here = PurePosixPath(book[2]) / pages[chapter][1]
        there = PurePosixPath(other[2]) / page(path)
        return f"]({os.path.relpath(there, here.parent)}{anchor})"
    if (ROOT / resolved).exists():
        return f"]({BLOB}{resolved}{anchor})"
    sys.exit(f"{chapter}: broken link to {target}")


def stage(book, pages):
    config, suffix, subdirectory = book
    staging = SITE / "stage" / (subdirectory or "en")
    source = staging / "src"
    source.mkdir(parents=True)
    shutil.copy(ROOT / config / "book.toml", staging)
    summary = (ROOT / config / "SUMMARY.md").read_text(encoding="utf-8")
    summary = CHAPTER.sub(lambda match: f"]({book_path(match.group(1), suffix)})", summary)
    (source / "SUMMARY.md").write_text(summary, encoding="utf-8")
    shutil.copy(ROOT / config / "api.md", source)
    for chapter, (owner, path) in pages.items():
        if owner != book:
            continue
        text = (ROOT / chapter).read_text(encoding="utf-8")
        text = LINK.sub(lambda match: rewrite_link(match, chapter, book, pages), text)
        # An HTML block keeps Markdown away from the backslashes and underscores of the formula
        text = MATH.sub(lambda match: f"<div>\n\\[\n{match.group(1)}\\]\n</div>\n", text)
        destination = source / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(text, encoding="utf-8")
    return staging


def finish(root, other, switch, titles):
    """Translate a book's alert titles and link each page to its counterpart in the other book."""
    label, title, language = switch
    # The English book's directory also holds the API docs and the Russian book
    skip = {"api", *(book[2] for book in BOOKS if book[2])} if root == BOOK else set()
    for html in root.rglob("*.html"):
        page_path = html.relative_to(root)
        # mdBook serves 404.html at any missing path, so a relative link from it would be wrong
        if page_path.parts[0] in skip or html.name == "404.html":
            continue
        text = html.read_text(encoding="utf-8")
        text = ALERT_TITLE.sub(lambda m: m[1] + titles.get(m[2], m[2]) + m[3], text)
        if HEADER in text:
            if not (other / page_path).is_file():
                sys.exit(f"{html.relative_to(BOOK)}: no counterpart in {other.relative_to(BOOK)}")
            href = os.path.relpath(other / page_path, html.parent)
            text = text.replace(HEADER, f'{HEADER}\n<a href="{href}" title="{title}" '
                                f'aria-label="{title}" hreflang="{language}" lang="{language}" '
                                f'class="language-switch">{label}</a>', 1)
        html.write_text(text, encoding="utf-8")


def main():
    shutil.rmtree(SITE, ignore_errors=True)
    pages = {}
    for book in BOOKS:
        for chapter in chapters(book[0]):
            pages[chapter] = (book, book_path(chapter, book[1]))
    for book in BOOKS:
        staging = stage(book, pages)
        subprocess.run(["mdbook-mermaid", "install", staging], check=True)
        subprocess.run(["mdbook", "build", staging, "--dest-dir", BOOK / book[2]], check=True)
    for book, other in zip(BOOKS, reversed(BOOKS)):
        finish(BOOK / book[2], BOOK / other[2], SWITCH[book[2]], ALERT_TITLES.get(book[2], {}))
    subprocess.run(["cargo", "doc", "--no-deps", "--locked", "-p", "keyrill-core", "-p",
                    "keyrill-ffi"], cwd=ROOT, check=True)
    shutil.copytree(ROOT / "target/doc", BOOK / "api", ignore=shutil.ignore_patterns(".lock"))


if __name__ == "__main__":
    main()
