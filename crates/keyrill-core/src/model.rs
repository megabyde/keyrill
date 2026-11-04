//! Embedded reference EN/RU word frequencies and order-2 character chains.
//!
//! `assets/model.bin` is produced by `tools/pack-model.py`, which documents the layout. Values
//! are log10 probabilities stored as integer micro-units; dividing by 10^6 reproduces the
//! correctly rounded parse of the frozen six-decimal source text exactly. Corpus frequencies
//! are not calibrated layout-error probabilities.

use crate::session::Mode;

static EMBEDDED: &[u8] = include_bytes!("../../../assets/model.bin");

const MAGIC: &[u8; 8] = b"KRMODEL\0";
const VERSION: u32 = 1;
const LANGUAGES: u32 = 2;
const SCALE: f64 = 1_000_000.0;
const SOURCE_SHA256: [u8; 32] = [
    0x82, 0xb2, 0x2d, 0xd4, 0x05, 0x5c, 0x4b, 0xdb, 0x70, 0x3d, 0xf8, 0xa8, 0x1b, 0x1a, 0x15, 0x47,
    0x4b, 0x5b, 0x9b, 0xe2, 0x2a, 0x64, 0x52, 0x5d, 0x41, 0x6f, 0x53, 0x60, 0x93, 0x81, 0xac, 0x27,
];
/// Longest stored word in UTF-16 units.
pub const MAX_MODEL_WORD: usize = 32;

/// Structural reason an asset was rejected; carries no asset content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelError {
    /// The asset ends before a declared section.
    Truncated,
    /// Wrong magic bytes.
    Magic,
    /// Unsupported format version.
    Version,
    /// Source identity differs from the frozen model (consistency, not authenticity).
    SourceHash,
    /// Unexpected number of languages.
    LanguageCount,
    /// Alphabet differs from the frozen language table.
    Alphabet,
    /// Entry count differs from the frozen language table.
    WordCount,
    /// Word offsets are not increasing, in bounds and within the word length limit.
    Offsets,
    /// A word contains an index outside its alphabet.
    Word,
    /// Words are not strictly increasing in UTF-16 order.
    Order,
    /// Bytes remain after the last section.
    TrailingBytes,
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Truncated => "model asset is truncated",
            Self::Magic => "not a Keyrill model asset",
            Self::Version => "unsupported model format version",
            Self::SourceHash => "model source differs from the frozen model",
            Self::LanguageCount => "unexpected number of model languages",
            Self::Alphabet => "model alphabet differs from the frozen table",
            Self::WordCount => "model word count differs from the frozen table",
            Self::Offsets => "invalid model word offsets",
            Self::Word => "model word uses an index outside its alphabet",
            Self::Order => "model words are not in UTF-16 order",
            Self::TrailingBytes => "model asset has trailing bytes",
        })
    }
}

impl std::error::Error for ModelError {}

/// One language's frequency table and character chain, borrowed from a validated asset.
#[derive(Clone, Copy)]
pub struct LanguageModel<'a> {
    alphabet: &'a [u8],
    offsets: &'a [u8],
    words: &'a [u8],
    frequencies: &'a [u8],
    chain: &'a [u8],
}

/// Validated EN and RU models.
#[derive(Clone, Copy)]
pub struct Model<'a> {
    en: LanguageModel<'a>,
    ru: LanguageModel<'a>,
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ModelError> {
        if length > self.bytes.len() {
            return Err(ModelError::Truncated);
        }
        let (head, tail) = self.bytes.split_at(length);
        self.bytes = tail;
        Ok(head)
    }

    fn take_array(&mut self, count: usize, width: usize) -> Result<&'a [u8], ModelError> {
        self.take(count.checked_mul(width).ok_or(ModelError::Truncated)?)
    }

    fn u32(&mut self) -> Result<u32, ModelError> {
        u32_at(self.take(4)?, 0).ok_or(ModelError::Truncated)
    }
}

fn u16_at(bytes: &[u8], index: usize) -> Option<u16> {
    let at = index.checked_mul(2)?;
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(bytes: &[u8], index: usize) -> Option<u32> {
    let at = index.checked_mul(4)?;
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn value_at(bytes: &[u8], index: usize) -> Option<f64> {
    let at = index.checked_mul(4)?;
    let micro = i32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?);
    Some(f64::from(micro) / SCALE)
}

impl<'a> LanguageModel<'a> {
    fn parse(
        reader: &mut Reader<'a>,
        expected_alphabet: &str,
        expected_count: u32,
    ) -> Result<Self, ModelError> {
        let letters = reader.u32()? as usize;
        if letters != expected_alphabet.encode_utf16().count() {
            return Err(ModelError::Alphabet);
        }
        let alphabet = reader.take_array(letters, 2)?;
        if !expected_alphabet
            .encode_utf16()
            .eq((0..letters).filter_map(|i| u16_at(alphabet, i)))
        {
            return Err(ModelError::Alphabet);
        }
        let count = reader.u32()?;
        if count != expected_count {
            return Err(ModelError::WordCount);
        }
        let count = count as usize;
        let offsets = reader.take_array(count.checked_add(1).ok_or(ModelError::Truncated)?, 4)?;
        if u32_at(offsets, 0) != Some(0) {
            return Err(ModelError::Offsets);
        }
        let mut end = 0;
        for i in 1..=count {
            let next = u32_at(offsets, i).ok_or(ModelError::Truncated)? as usize;
            if next <= end || next - end > MAX_MODEL_WORD {
                return Err(ModelError::Offsets);
            }
            end = next;
        }
        let words = reader.take(end)?;
        if words.iter().any(|&index| usize::from(index) >= letters) {
            return Err(ModelError::Word);
        }
        let frequencies = reader.take_array(count, 4)?;
        let size = letters + 2;
        let chain = reader.take_array(size * size * size, 4)?;
        let model = Self {
            alphabet,
            offsets,
            words,
            frequencies,
            chain,
        };
        for i in 1..count {
            let (previous, current) = (model.word(i - 1), model.word(i));
            let ordered = previous
                .zip(current)
                .is_some_and(|(p, c)| model.units(p).lt(model.units(c)));
            if !ordered {
                return Err(ModelError::Order);
            }
        }
        Ok(model)
    }

    fn letters(&self) -> usize {
        self.alphabet.len() / 2
    }

    fn word(&self, index: usize) -> Option<&'a [u8]> {
        let start = u32_at(self.offsets, index)? as usize;
        let end = u32_at(self.offsets, index.checked_add(1)?)? as usize;
        self.words.get(start..end)
    }

    fn units(&self, word: &'a [u8]) -> impl Iterator<Item = u16> + 'a {
        let alphabet = self.alphabet;
        word.iter()
            .map(move |&index| u16_at(alphabet, usize::from(index)).unwrap_or(u16::MAX))
    }

    fn index(&self, unit: u16) -> Option<usize> {
        (0..self.letters()).find(|&i| u16_at(self.alphabet, i) == Some(unit))
    }

    /// Stored log10 corpus frequency of an exact lowercase word, or `None` if absent.
    /// No case folding or normalization is applied.
    pub fn frequency(&self, word: &[u16]) -> Option<f64> {
        if word.is_empty() || word.len() > MAX_MODEL_WORD {
            return None;
        }
        let (mut low, mut high) = (0, self.offsets.len() / 4 - 1);
        while low < high {
            let middle = low + (high - low) / 2;
            match self.units(self.word(middle)?).cmp(word.iter().copied()) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return value_at(self.frequencies, middle),
            }
        }
        None
    }

    /// log10 likelihood of the word followed by the end marker under the order-2 chain,
    /// summed left to right from the two start markers. `None` if the word is outside 1..=32
    /// units or has a unit outside this language's alphabet.
    pub fn log_likelihood(&self, word: &[u16]) -> Option<f64> {
        if word.is_empty() || word.len() > MAX_MODEL_WORD {
            return None;
        }
        let letters = self.letters();
        let size = letters + 2;
        let (mut a, mut b) = (size - 1, size - 1);
        let mut total = 0.0;
        for next in word
            .iter()
            .map(|&unit| self.index(unit))
            .chain([Some(letters)])
        {
            let c = next?;
            total += value_at(self.chain, (a * size + b) * size + c)?;
            (a, b) = (b, c);
        }
        Some(total)
    }
}

impl<'a> Model<'a> {
    /// Validate format and frozen identity metadata. External bytes still need provenance
    /// verification; the header is not payload authentication. Lookups use checked slices.
    ///
    /// # Errors
    ///
    /// The first structural [`ModelError`] found, from the header through the trailing bytes.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ModelError> {
        let mut reader = Reader { bytes };
        if reader.take(MAGIC.len())? != MAGIC {
            return Err(ModelError::Magic);
        }
        if reader.u32()? != VERSION {
            return Err(ModelError::Version);
        }
        if reader.take(32)? != SOURCE_SHA256 {
            return Err(ModelError::SourceHash);
        }
        if reader.u32()? != LANGUAGES {
            return Err(ModelError::LanguageCount);
        }
        let en = LanguageModel::parse(&mut reader, "abcdefghijklmnopqrstuvwxyz", 89_656)?;
        let ru = LanguageModel::parse(&mut reader, "абвгдеёжзийклмнопрстуфхцчшщъыьэюя", 241_624)?;
        if !reader.bytes.is_empty() {
            return Err(ModelError::TrailingBytes);
        }
        Ok(Self { en, ru })
    }

    /// Validate the asset compiled into this crate. Validation walks the whole asset, so
    /// callers should construct it once, outside input callbacks, and keep it.
    ///
    /// ```
    /// use keyrill_core::model::Model;
    /// use keyrill_core::session::Mode;
    ///
    /// # fn main() -> Result<(), keyrill_core::model::ModelError> {
    /// let model = Model::embedded()?;
    /// let hello: Vec<u16> = "hello".encode_utf16().collect();
    /// assert!(model.language(Mode::En).frequency(&hello).is_some());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// A [`ModelError`] only if the build embedded a corrupt asset; `tools/pack-model.py --check`
    /// guards against that.
    pub fn embedded() -> Result<Model<'static>, ModelError> {
        Model::parse(EMBEDDED)
    }

    /// The model for the language a mode types.
    pub fn language(&self, mode: Mode) -> &LanguageModel<'a> {
        match mode {
            Mode::En => &self.en,
            Mode::Ru => &self.ru,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn model() -> Model<'static> {
        Model::embedded().unwrap()
    }

    #[test]
    fn embedded_counts_and_table_boundaries() {
        let model = model();
        for (mode, count, first, last) in [
            (Mode::En, 89_656, "a", "zzz"),
            (Mode::Ru, 241_624, "а", "ёё"),
        ] {
            let language = model.language(mode);
            assert_eq!(language.offsets.len() / 4 - 1, count);
            assert!(language.frequency(&utf16(first)).is_some());
            assert!(language.frequency(&utf16(last)).is_some());
        }
    }

    #[test]
    fn known_frequencies_are_exact_source_values() {
        let model = model();
        for (mode, word, expected) in [
            (Mode::En, "a", -1.64_f64),
            (Mode::En, "the", -1.27),
            (Mode::En, "hello", -4.28),
            (Mode::En, "zzz", -6.79),
            (Mode::Ru, "а", -2.14),
            (Mode::Ru, "привет", -3.87),
            // ё sorts after я in UTF-16 order but follows е in the alphabet
            (Mode::Ru, "еж", -6.06),
            (Mode::Ru, "ёж", -6.26),
            (Mode::Ru, "ёлка", -5.99),
            (Mode::Ru, "ёё", -6.8),
        ] {
            let found = model.language(mode).frequency(&utf16(word));
            assert_eq!(found.map(f64::to_bits), Some(expected.to_bits()), "{word}");
        }
    }

    #[test]
    fn absent_words_and_bounds() {
        let model = model();
        let en = model.language(Mode::En);
        for word in ["ghbdtn", "The", "привет", "", "hello!"] {
            assert_eq!(en.frequency(&utf16(word)), None, "{word}");
        }
        assert_eq!(en.frequency(&[0xd83d, 0xde00]), None);
        assert_eq!(en.frequency(&utf16(&"a".repeat(MAX_MODEL_WORD + 1))), None);
        assert_eq!(model.language(Mode::Ru).frequency(&utf16("hello")), None);
    }

    #[test]
    fn chain_likelihoods_match_reference_summation_bit_for_bit() {
        // Expected bit patterns are left-to-right f64 sums of the frozen source text
        let model = model();
        for (mode, word, expected) in [
            (Mode::En, "the", 0xbff6_602b_8412_48d7_u64),
            (Mode::En, "hello", 0xc01d_6efa_a476_6c6e),
            (Mode::En, "ghbdtn", 0xc030_c09f_5169_7f20),
            (Mode::Ru, "привет", 0xc015_f3c9_abb0_1c92),
            (Mode::Ru, "ёлка", 0xc020_0e1f_3a57_eaa3),
            (Mode::Ru, "руддщ", 0xc027_bc6f_9b99_4e1a),
        ] {
            let found = model.language(mode).log_likelihood(&utf16(word));
            assert_eq!(found.map(f64::to_bits), Some(expected), "{word}");
        }
        let en = model.language(Mode::En);
        assert_eq!(en.log_likelihood(&[]), None);
        assert_eq!(en.log_likelihood(&utf16("Hello")), None);
        assert_eq!(en.log_likelihood(&utf16("привет")), None);
        assert_eq!(en.log_likelihood(&[97; MAX_MODEL_WORD + 1]), None);
    }

    #[test]
    fn every_stored_value_reconstructs_its_six_decimal_text() {
        let model = model();
        for mode in [Mode::En, Mode::Ru] {
            let language = model.language(mode);
            for bytes in [language.frequencies, language.chain] {
                let (chunks, rest) = bytes.as_chunks::<4>();
                assert!(rest.is_empty());
                for &chunk in chunks {
                    let micro = i32::from_le_bytes(chunk);
                    let sign = if micro < 0 { "-" } else { "" };
                    let magnitude = micro.unsigned_abs();
                    let text = format!(
                        "{sign}{}.{:06}",
                        magnitude / 1_000_000,
                        magnitude % 1_000_000
                    );
                    assert_eq!(
                        (f64::from(micro) / SCALE).to_bits(),
                        text.parse::<f64>().unwrap().to_bits()
                    );
                }
            }
        }
    }

    // Byte offsets into the embedded asset: header, then the English alphabet of 26 units
    const EN_COUNT: usize = 48 + 4 + 52;
    const EN_OFFSETS: usize = EN_COUNT + 4;
    const EN_WORDS: usize = EN_OFFSETS + (89_656 + 1) * 4;

    fn mutated(at: usize, bytes: &[u8]) -> Result<(), ModelError> {
        let mut copy = EMBEDDED.to_vec();
        copy[at..at + bytes.len()].copy_from_slice(bytes);
        Model::parse(&copy).map(|_| ())
    }

    #[test]
    fn malformed_assets_are_rejected_with_exact_errors() {
        use ModelError::*;
        let cases: [(usize, &[u8], ModelError); 14] = [
            (0, b"X", Magic),
            (8, &2u32.to_le_bytes(), Version),
            (12, &[0], SourceHash),
            (44, &3u32.to_le_bytes(), LanguageCount),
            (48, &0u32.to_le_bytes(), Alphabet),
            (48, &65u32.to_le_bytes(), Alphabet),
            (52 + 2, &u16::from(b'a').to_le_bytes(), Alphabet),
            (52, &0u16.to_le_bytes(), Alphabet),
            (EN_COUNT, &89_655u32.to_le_bytes(), WordCount),
            (EN_OFFSETS, &1u32.to_le_bytes(), Offsets),
            (EN_OFFSETS + 4, &0u32.to_le_bytes(), Offsets),
            (EN_OFFSETS + 4, &40u32.to_le_bytes(), Offsets),
            (EN_WORDS, &[26], Word),
            // "a" becomes "b", which sorts after the following "aa"
            (EN_WORDS, &[1], Order),
        ];
        for (at, bytes, error) in cases {
            assert_eq!(mutated(at, bytes).err(), Some(error), "offset {at}");
        }
        assert_eq!(
            mutated(EN_COUNT, &u32::MAX.to_le_bytes()).err(),
            Some(WordCount)
        );
    }

    #[test]
    fn truncated_and_extended_assets_are_rejected() {
        for length in [
            0,
            7,
            8,
            11,
            44,
            47,
            51,
            EN_OFFSETS + 3,
            EN_WORDS + 1,
            EMBEDDED.len() - 1,
        ] {
            assert_eq!(
                Model::parse(&EMBEDDED[..length]).err(),
                Some(ModelError::Truncated),
                "{length}"
            );
        }
        let mut extended = EMBEDDED.to_vec();
        extended.push(0);
        assert_eq!(
            Model::parse(&extended).err(),
            Some(ModelError::TrailingBytes)
        );
    }
}
