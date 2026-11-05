//! Read-only Vim word-list parsing and immutable last-good vocabulary snapshots.

use std::collections::HashMap;
use std::sync::Arc;

/// Maximum UTF-8 source size per personal word list.
pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;

/// Personal vocabulary membership, without invented corpus frequency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Membership {
    /// No matching case-aware entry.
    Absent,
    /// Legitimate word.
    Recognized,
    /// Rare but legitimate word.
    Rare,
    /// Forbidden word; overrides other matching entries.
    Forbidden,
}

/// Content-free whole-candidate rejection; a failed parse never publishes a partial list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Source exceeds the bounded read contract.
    TooLarge,
    /// NUL is not a word-list character; it marks a binary, UTF-16 or partly written file.
    Nul,
    /// A leading `/encoding=` names an encoding other than UTF-8, or a byte-order mark names one
    /// other than UTF-8 or UTF-16 LE.
    Encoding,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "word list exceeds 1 MiB",
            Self::Nul => "word list contains NUL",
            Self::Encoding => "unsupported word-list encoding",
        })
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Copy)]
enum CaseRule {
    Lower,
    ExactOrUpper,
    Exact,
}

struct Entry {
    word: Vec<u16>,
    uppercase: Vec<u16>,
    case: CaseRule,
    membership: Membership,
}

/// Parsed personal entries. Region declarations and flags are validated but not kept, so lookup
/// is locale-neutral.
#[derive(Default)]
pub struct Vocabulary {
    entries: HashMap<Vec<u16>, Vec<Entry>>,
    count: usize,
    skipped: usize,
}

/// Lowercase only the initial EN/RU alphabets, preserving every other UTF-16 unit.
pub fn lowercase(units: &[u16]) -> Vec<u16> {
    units
        .iter()
        .map(|&u| match u {
            0x41..=0x5a | 0x410..=0x42f => u + 32,
            0x401 => 0x451,
            _ => u,
        })
        .collect()
}

/// Uppercase only the initial EN/RU alphabets, preserving every other UTF-16 unit.
pub fn uppercase(units: &[u16]) -> Vec<u16> {
    units
        .iter()
        .map(|&u| match u {
            0x61..=0x7a | 0x430..=0x44f => u - 32,
            0x451 => 0x401,
            _ => u,
        })
        .collect()
}

impl Vocabulary {
    /// Parse a Vim word list line by line, including BOM, CRLF, case and forbidden flags.
    ///
    /// A line whose first byte is `#` is a comment whatever follows it, because Vim's `zw`, `zug`
    /// and `zuw` comment out an entry by overwriting its first byte. A line that isn't UTF-8, an
    /// unknown `/` directive, an unsupported flag or another invalid line is skipped and counted
    /// in [`Vocabulary::skipped`].
    ///
    /// ```
    /// use keyrill_core::vocabulary::{Membership, Vocabulary};
    ///
    /// # fn main() -> Result<(), keyrill_core::vocabulary::ParseError> {
    /// let words = Vocabulary::parse("# personal\nrustc\nNeovim/=\nteh/!\nbad/*\n".as_bytes())?;
    /// let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    /// assert_eq!(words.lookup(&units("RUSTC")), Membership::Recognized);
    /// assert_eq!(words.lookup(&units("neovim")), Membership::Absent);
    /// assert_eq!(words.lookup(&units("teh")), Membership::Forbidden);
    /// assert_eq!((words.len(), words.skipped()), (3, 1));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// A [`ParseError`] for oversized input, NUL anywhere or a leading non-UTF-8 `/encoding=`;
    /// nothing is partially kept.
    pub fn parse(source: &[u8]) -> Result<Self, ParseError> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(ParseError::TooLarge);
        }
        if source.contains(&0) {
            return Err(ParseError::Nul);
        }
        let source = source.strip_prefix("\u{feff}".as_bytes()).unwrap_or(source);
        let mut result = Self::default();
        'lines: for line in source.split(|&byte| byte == b'\n') {
            if line.first() == Some(&b'#') {
                continue;
            }
            let Ok(line) = std::str::from_utf8(line) else {
                result.skipped += 1;
                continue;
            };
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(encoding) = line.strip_prefix("/encoding=") {
                if result.count != 0 {
                    // Vim ignores a declaration after the first word
                    result.skipped += 1;
                } else if encoding != "utf-8" {
                    return Err(ParseError::Encoding);
                }
                continue;
            }
            if let Some(regions) = line.strip_prefix("/regions=") {
                if regions.is_empty()
                    || regions.len() > 18
                    || !regions.len().is_multiple_of(2)
                    || !regions.bytes().all(|b| b.is_ascii_alphabetic())
                {
                    result.skipped += 1;
                    continue;
                }
                let mut names = Vec::new();
                for pair in regions.as_bytes().as_chunks::<2>().0 {
                    if names.contains(&pair) {
                        result.skipped += 1;
                        continue 'lines;
                    }
                    names.push(pair);
                }
                continue;
            }
            let (word, flags) = line.split_once('/').unwrap_or((line, ""));
            if word.is_empty() || word.chars().any(char::is_control) {
                result.skipped += 1;
                continue;
            }
            let mut membership = Membership::Recognized;
            let mut exact = false;
            for flag in flags.chars() {
                match flag {
                    '=' => exact = true,
                    '?' if membership != Membership::Forbidden => membership = Membership::Rare,
                    '?' | '1'..='9' => (),
                    '!' => membership = Membership::Forbidden,
                    _ => {
                        result.skipped += 1;
                        continue 'lines;
                    }
                }
            }
            result.push(word, membership, exact);
        }
        Ok(result)
    }

    /// Parse a plain word list, one word per line, as OS spell checkers keep learned words: UTF-8
    /// with an optional BOM, or UTF-16 LE with its BOM. Every word gets `membership` and the case
    /// rule of an unflagged Vim entry; `#` and `/` are ordinary characters. A line that doesn't
    /// decode or holds a control character is skipped and counted.
    ///
    /// ```
    /// use keyrill_core::vocabulary::{Membership, Vocabulary};
    ///
    /// # fn main() -> Result<(), keyrill_core::vocabulary::ParseError> {
    /// let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    /// let learned = Vocabulary::parse_words(b"km/h\r\nrustc\n", Membership::Recognized)?;
    /// assert_eq!(learned.lookup(&units("km/h")), Membership::Recognized);
    /// assert_eq!(learned.lookup(&units("Rustc")), Membership::Recognized);
    ///
    /// let mut utf16 = vec![0xff, 0xfe];
    /// utf16.extend("teh\r\n".encode_utf16().flat_map(u16::to_le_bytes));
    /// let excluded = Vocabulary::parse_words(&utf16, Membership::Forbidden)?;
    /// assert_eq!(excluded.lookup(&units("teh")), Membership::Forbidden);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// A [`ParseError`] for oversized input, NUL anywhere, a UTF-16 LE source of odd length or a
    /// big-endian UTF-16 BOM; nothing is partially kept.
    pub fn parse_words(source: &[u8], membership: Membership) -> Result<Self, ParseError> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(ParseError::TooLarge);
        }
        if source.starts_with(&[0xfe, 0xff]) {
            return Err(ParseError::Encoding);
        }
        let mut result = Self::default();
        if let Some(utf16) = source.strip_prefix(&[0xff, 0xfe]) {
            let (units, rest) = utf16.as_chunks::<2>();
            if !rest.is_empty() {
                return Err(ParseError::Encoding);
            }
            let units: Vec<u16> = units.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
            if units.contains(&0) {
                return Err(ParseError::Nul);
            }
            for line in units.split(|&unit| unit == u16::from(b'\n')) {
                match String::from_utf16(line) {
                    Ok(line) => result.push_word(&line, membership),
                    Err(_) => result.skipped += 1,
                }
            }
            return Ok(result);
        }
        if source.contains(&0) {
            return Err(ParseError::Nul);
        }
        let source = source.strip_prefix("\u{feff}".as_bytes()).unwrap_or(source);
        for line in source.split(|&byte| byte == b'\n') {
            match std::str::from_utf8(line) {
                Ok(line) => result.push_word(line, membership),
                Err(_) => result.skipped += 1,
            }
        }
        Ok(result)
    }

    fn push_word(&mut self, line: &str, membership: Membership) {
        let word = line.trim();
        if word.is_empty() {
            return;
        }
        if word.chars().any(char::is_control) {
            self.skipped += 1;
            return;
        }
        self.push(word, membership, false);
    }

    fn push(&mut self, word: &str, membership: Membership, exact: bool) {
        let word: Vec<_> = word.encode_utf16().collect();
        let lower = lowercase(&word);
        let case = if exact {
            CaseRule::Exact
        } else if word == lower {
            CaseRule::Lower
        } else {
            CaseRule::ExactOrUpper
        };
        let entry = Entry {
            uppercase: uppercase(&word),
            word,
            case,
            membership,
        };
        self.entries.entry(lower).or_default().push(entry);
        self.count += 1;
    }

    /// Number of parsed entry lines, retaining duplicate declarations for provenance.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether a valid source explicitly contains no entries.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Number of skipped lines that weren't comments, blank or accepted entries and directives.
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Match exact, lowercase/initial-capital/uppercase or mixed-case rules as declared.
    pub fn lookup(&self, word: &[u16]) -> Membership {
        let lower = lowercase(word);
        let Some(entries) = self.entries.get(&lower) else {
            return Membership::Absent;
        };
        let mut result = Membership::Absent;
        for entry in entries {
            let matched = match entry.case {
                CaseRule::Exact => word == entry.word,
                CaseRule::ExactOrUpper => word == entry.word || word == entry.uppercase,
                CaseRule::Lower => {
                    word == entry.word
                        || word == entry.uppercase
                        || (word.first() == entry.uppercase.first()
                            && word.get(1..) == entry.word.get(1..))
                }
            };
            if matched {
                result = match (result, entry.membership) {
                    (Membership::Forbidden, _) | (_, Membership::Forbidden) => {
                        Membership::Forbidden
                    }
                    (Membership::Recognized, _) | (_, Membership::Recognized) => {
                        Membership::Recognized
                    }
                    _ => Membership::Rare,
                };
            }
        }
        result
    }
}

/// Atomically replaceable immutable vocabulary, initially missing rather than silently empty.
#[derive(Default)]
pub struct Snapshot {
    current: Option<Arc<Vocabulary>>,
}

impl From<Option<Arc<Vocabulary>>> for Snapshot {
    fn from(current: Option<Arc<Vocabulary>>) -> Self {
        Self { current }
    }
}

impl Snapshot {
    /// Clone the current immutable snapshot; absence keeps automatic policy unready.
    pub fn current(&self) -> Option<Arc<Vocabulary>> {
        self.current.clone()
    }

    /// Publish a parsed candidate, skipped lines included. Existing readers retain their prior
    /// snapshot.
    ///
    /// # Errors
    ///
    /// The candidate's [`ParseError`]; the current snapshot is left unchanged.
    pub fn replace(&mut self, source: &[u8]) -> Result<(), ParseError> {
        let candidate = Vocabulary::parse(source)?;
        self.current = Some(Arc::new(candidate));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn word(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn utf16le(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    #[test]
    fn plain_word_lists_take_every_line_literally_in_utf8_and_utf16() {
        let text = "#hashtag\r\n/usr\r\n\r\nw/o\r\niPhone\r\nпривет\r\nbad\u{7}word\r\n𝔘nicode\r\n";
        let utf8 = [b"\xef\xbb\xbf".as_slice(), text.as_bytes()].concat();
        for source in [utf8, utf16le(text)] {
            let v = Vocabulary::parse_words(&source, Membership::Recognized).unwrap();
            assert_eq!((v.len(), v.skipped()), (6, 1));
            for s in [
                "#hashtag",
                "/usr",
                "w/o",
                "iPhone",
                "IPHONE",
                "Привет",
                "𝔘nicode",
            ] {
                assert_eq!(v.lookup(&word(s)), Membership::Recognized, "{s}");
            }
            assert_eq!(v.lookup(&word("iphone")), Membership::Absent);
            assert_eq!(v.lookup(&word("w")), Membership::Absent);
        }
        let excluded = Vocabulary::parse_words(&utf16le("teh"), Membership::Forbidden).unwrap();
        assert_eq!(excluded.lookup(&word("Teh")), Membership::Forbidden);
    }

    #[test]
    fn plain_word_lists_reject_whole_sources_and_skip_undecodable_lines() {
        let mut lone = utf16le("ok\n");
        lone.extend([0x00, 0xd8, b'\n', 0x00]);
        let v = Vocabulary::parse_words(&lone, Membership::Recognized).unwrap();
        assert_eq!((v.len(), v.skipped()), (1, 1));
        let v = Vocabulary::parse_words(b"ok\n\xff\xfe\xfd\n", Membership::Recognized);
        assert_eq!(v.map(|v| (v.len(), v.skipped())), Ok((1, 1)));
        let mut odd = utf16le("ok");
        odd.push(b'x');
        let big_endian = [0xfe, 0xff, 0x00, b'o'];
        let mut too_large = utf16le("");
        too_large.resize(MAX_SOURCE_BYTES + 1, b'a');
        for (source, error) in [
            (odd.as_slice(), ParseError::Encoding),
            (&big_endian, ParseError::Encoding),
            (&utf16le("a\0b"), ParseError::Nul),
            (b"a\0b", ParseError::Nul),
            (&too_large, ParseError::TooLarge),
        ] {
            assert_eq!(
                Vocabulary::parse_words(source, Membership::Recognized).err(),
                Some(error)
            );
        }
    }

    #[test]
    fn case_flags_and_duplicates_are_preserved() {
        let v = Vocabulary::parse(b"/regions=usgb\nhello/?1\nhello/2\niPhone\nExact/=\nbad/?!\n")
            .unwrap();
        assert_eq!(v.len(), 5);
        for s in ["hello", "Hello", "HELLO"] {
            assert_eq!(v.lookup(&word(s)), Membership::Recognized);
        }
        assert_eq!(v.lookup(&word("hElLo")), Membership::Absent);
        assert_eq!(v.lookup(&word("iphone")), Membership::Absent);
        assert_eq!(v.lookup(&word("iPhone")), Membership::Recognized);
        assert_eq!(v.lookup(&word("IPHONE")), Membership::Recognized);
        assert_eq!(v.lookup(&word("Exact")), Membership::Recognized);
        assert_eq!(v.lookup(&word("EXACT")), Membership::Absent);
        assert_eq!(v.lookup(&word("Bad")), Membership::Forbidden);
    }

    #[test]
    fn invalid_candidates_never_replace_last_good_or_mutate_existing_readers() {
        let mut s = Snapshot::default();
        assert!(s.current().is_none());
        s.replace(b"old\n").unwrap();
        let old = s.current().unwrap();
        for (bytes, expected) in [
            (b"new\na\0b".as_slice(), ParseError::Nul),
            (b"# \0".as_slice(), ParseError::Nul),
            (b"/encoding=latin1\nnew".as_slice(), ParseError::Encoding),
            (b"# head\n/encoding=UTF-8".as_slice(), ParseError::Encoding),
        ] {
            assert_eq!(s.replace(bytes), Err(expected));
            assert!(Arc::ptr_eq(&old, &s.current().unwrap()));
        }
        assert_eq!(
            s.replace(&vec![b'a'; MAX_SOURCE_BYTES + 1]),
            Err(ParseError::TooLarge)
        );
        s.replace(b"new").unwrap();
        assert_eq!(old.lookup(&word("old")), Membership::Recognized);
        assert_eq!(
            s.current().unwrap().lookup(&word("old")),
            Membership::Absent
        );
        s.replace(b"").unwrap();
        assert!(s.current().unwrap().is_empty());
    }

    #[test]
    fn vim_commented_out_cyrillic_entry_keeps_the_rest_of_the_file() {
        // zug, zw and zuw overwrite the first byte of the entry's line with '#'
        let mut removed = "привет/!".as_bytes().to_vec();
        removed[0] = b'#';
        assert!(std::str::from_utf8(&removed).is_err());
        let mut source = "ёж\r\n".as_bytes().to_vec();
        source.extend_from_slice(&removed);
        source.extend_from_slice("\r\nПривет\nмир/?\n".as_bytes());
        let v = Vocabulary::parse(&source).unwrap();
        assert_eq!((v.len(), v.skipped()), (3, 0));
        assert_eq!(v.lookup(&word("ЁЖ")), Membership::Recognized);
        assert_eq!(v.lookup(&word("Привет")), Membership::Recognized);
        assert_eq!(v.lookup(&word("привет")), Membership::Absent);
        assert_eq!(v.lookup(&word("Мир")), Membership::Rare);
    }

    #[test]
    fn invalid_lines_are_skipped_and_counted_without_dropping_entries() {
        let source = b"/encoding=utf-8\nkeep\n/unknown\nbad/*\nTCP\\/IP\nzap/!x\n\xd0\n\
            /regions=usus\n/regions=u\n/\nhello\rworld\n/encoding=utf-8\n/encoding=latin1\n\
            \xff# indented\nlast/!";
        let v = Vocabulary::parse(source).unwrap();
        assert_eq!((v.len(), v.skipped()), (2, 12));
        assert_eq!(v.lookup(&word("keep")), Membership::Recognized);
        assert_eq!(v.lookup(&word("last")), Membership::Forbidden);
        for absent in ["bad", "TCP\\", "TCP\\/IP", "TCP/IP", "zap", "hello"] {
            assert_eq!(v.lookup(&word(absent)), Membership::Absent, "{absent}");
        }
        let v = Vocabulary::parse(b"\xff\n").unwrap();
        assert!(v.is_empty());
        assert_eq!(v.skipped(), 1);
    }

    #[test]
    fn bom_crlf_russian_case_and_unknown_units() {
        let v = Vocabulary::parse("\u{feff}/encoding=utf-8\r\nёж/?\r\n".as_bytes()).unwrap();
        for s in ["ёж", "Ёж", "ЁЖ"] {
            assert_eq!(v.lookup(&word(s)), Membership::Rare);
        }
        assert_eq!(v.lookup(&[0xd800]), Membership::Absent);
        assert_eq!(lowercase(&[0x401, 0xd800]), [0x451, 0xd800]);
        assert_eq!(uppercase(&[0x451, 0xdc00]), [0x401, 0xdc00]);
    }

    #[test]
    fn cross_case_precedence_and_supported_text_edges() {
        let v = Vocabulary::parse(b"Hello/!\nhello\n").unwrap();
        for text in ["Hello", "HELLO"] {
            assert_eq!(v.lookup(&word(text)), Membership::Forbidden);
        }
        assert_eq!(v.lookup(&word("hello")), Membership::Recognized);
        assert_eq!(v.skipped(), 0);
        let v = Vocabulary::parse("\u{a0}hello\u{a0}\n/regions=usgb\nMac/9".as_bytes()).unwrap();
        assert_eq!(v.lookup(&word("hello")), Membership::Recognized);
        assert_eq!(v.lookup(&word("Mac")), Membership::Recognized);
    }
}
