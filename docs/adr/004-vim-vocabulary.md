# ADR 004: read existing Vim spelling files

> [!TIP]
> **Status:** accepted.

Personal vocabulary comes from the user's existing UTF-8 Vim `.add` files, read in place, read-only,
with last-good snapshots on reload. They are parsed the way Vim reads them: a line whose first byte
is `#` is a comment (Vim's `zw`, `zug` and `zuw` overwrite that byte, which can leave invalid UTF-8
after it), and a line Keyrill can't use is skipped and counted rather than rejecting the file. The
alternatives were an app-specific dictionary, syncing from a remote repository, parsing binary
`.spl` files and running Vim per correction; each adds ownership, licensing, privacy or maintenance
cost with no demonstrated need.

Recognized, rare, forbidden and case distinctions are preserved. Personal membership protects a word
but doesn't give a candidate a frequency. A missing file never blocks typing or manual conversion.
Automatic correction needs a source for both languages; an empty file is a valid source, and so is a
missing file at the default path, since Vim creates it only on the first `zg`. The default path is
where `zg` creates it while `'spellfile'` is empty: `~/.vim/spell/{en,ru}.utf-8.add`, or
`~/vimfiles/spell` on Windows. A missing file at a path the user chose keeps that language missing.
Keyrill never modifies the files.

The OS spell checker's learned lists are read the same way, read-only and in place, so a word taught
with Learn Spelling on macOS or Add to dictionary on Windows is protected without a second list.
They are plain word lists, not Vim syntax, and never required: correction waits only for the two Vim
sources. Excluded words on Windows are forbidden, as with Vim's `zw`.

Revisit if the user's vocabulary workflow changes or a required semantic can't be expressed in this
format.
