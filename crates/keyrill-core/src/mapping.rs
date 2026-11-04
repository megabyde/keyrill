//! EN/RU maps over the defined key positions; unsupported UTF-16 units pass through unchanged.

const EN: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,.";
const RU: &str = "йцукенгшщзхъфывапролджэячсмитьбю";
const EN_SHIFT: &str = "QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>";
const RU_SHIFT: &str = "ЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ";

/// Physical Russian keyboard variant for the defined partial map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keyboard {
    /// Apple's Russian layout, with ё at the US backslash key.
    #[default]
    Apple,
    /// Russian PC layout, with ё at the US backtick key.
    Pc,
}

/// Mapping intent. Manual mapping preserves source punctuation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mapping {
    /// Convert each supported letter independently.
    Manual,
    /// Convert the defined US physical positions to Russian.
    EnToRu,
    /// Convert the defined Russian physical positions to US.
    RuToEn,
}

/// Whether a unit is one of the initial supported EN/RU letters.
pub fn is_letter(unit: u16) -> bool {
    matches!(unit, 0x41..=0x5a | 0x61..=0x7a | 0x410..=0x44f | 0x401 | 0x451)
}

/// Convert without normalizing, decoding, or replacing unsupported units.
///
/// ```
/// use keyrill_core::mapping::{convert, Keyboard, Mapping};
///
/// let text = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
/// assert_eq!(convert(&text("ghbdtn"), Keyboard::Apple, Mapping::EnToRu), text("привет"));
/// assert_eq!(convert(&text("руддщ"), Keyboard::Apple, Mapping::RuToEn), text("hello"));
/// // Manual mapping converts letters only and keeps punctuation.
/// assert_eq!(convert(&text("Ghbdtn, vbh!"), Keyboard::Apple, Mapping::Manual), text("Привет, мир!"));
/// ```
pub fn convert(text: &[u16], keyboard: Keyboard, mapping: Mapping) -> Vec<u16> {
    text.iter()
        .map(|&unit| convert_unit(unit, keyboard, mapping))
        .collect()
}

fn convert_unit(unit: u16, keyboard: Keyboard, mapping: Mapping) -> u16 {
    if mapping == Mapping::Manual && !is_letter(unit) {
        return unit;
    }
    let extra = match keyboard {
        Keyboard::Apple => ('\\', '|'),
        Keyboard::Pc => ('`', '~'),
    };
    for (en, ru) in EN
        .chars()
        .zip(RU.chars())
        .chain(EN_SHIFT.chars().zip(RU_SHIFT.chars()))
        .chain([(extra.0, 'ё'), (extra.1, 'Ё')])
    {
        if mapping != Mapping::RuToEn && unit == en as u16 {
            return ru as u16;
        }
        if mapping != Mapping::EnToRu && unit == ru as u16 {
            return en as u16;
        }
    }
    unit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_preserves_unsupported_units_and_punctuation() {
        let input: Vec<_> = "Ghbdtn, vbh!"
            .encode_utf16()
            .chain([0xd800, 0xdc00, 0xffff])
            .collect();
        let expected: Vec<_> = "Привет, мир!"
            .encode_utf16()
            .chain([0xd800, 0xdc00, 0xffff])
            .collect();
        assert_eq!(convert(&input, Keyboard::Apple, Mapping::Manual), expected);
    }

    #[test]
    fn defined_physical_positions_round_trip() {
        for keyboard in [Keyboard::Apple, Keyboard::Pc] {
            for unit in EN.encode_utf16().chain(EN_SHIFT.encode_utf16()) {
                let ru = convert(&[unit], keyboard, Mapping::EnToRu);
                assert_eq!(convert(&ru, keyboard, Mapping::RuToEn), [unit]);
            }
        }
    }

    #[test]
    fn unsupported_units_are_preserved_exhaustively() {
        for unit in 0..=u16::MAX {
            if !is_letter(unit) {
                assert_eq!(convert(&[unit], Keyboard::Apple, Mapping::Manual), [unit]);
            }
        }
    }
}
