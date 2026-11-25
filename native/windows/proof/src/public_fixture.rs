//! Compiled public fixture identities and exact UTF-16 document/selection oracles.
use crate::field::{self, Provider, PublicCase, Refusal};

/// Stable public fixture marker required before character inspection.
pub const MARKER: &str = "Keyrill public fixture v1";
/// Public prefix includes an astral character to distinguish UTF-16 offsets.
pub const PREFIX: &str = "Fixture α😀 start: ";
/// Complete public suffix preserved around the test word.
pub const SUFFIX: &str = " Fixture end.";

/// Explicit owned fixture control selected by the observer command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixture {
    /// Browser ordinary single-line input.
    Input,
    /// Browser multiline input.
    Textarea,
    /// Browser plaintext contenteditable.
    Plain,
    /// Browser rich contenteditable.
    Rich,
    /// Independent browser field used for focus transitions.
    FieldB,
    /// Public VS Code text editor.
    VsCode,
    /// Public Word RTF document body.
    Word,
}
impl Fixture {
    /// Parse one exact public fixture control identifier.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "input" => Some(Self::Input),
            "textarea" => Some(Self::Textarea),
            "plain" => Some(Self::Plain),
            "rich" => Some(Self::Rich),
            "field-b" => Some(Self::FieldB),
            "vscode" => Some(Self::VsCode),
            "word" => Some(Self::Word),
            _ => None,
        }
    }
    /// Exact accessible control name for the browser fixture.
    pub fn label(self) -> &'static str {
        match self {
            Self::Input => "Keyrill public fixture v1 input",
            Self::Textarea => "Keyrill public fixture v1 textarea",
            Self::Plain => "Keyrill public fixture v1 plain",
            Self::Rich => "Keyrill public fixture v1 rich",
            Self::FieldB => "Keyrill public fixture v1 field-b",
            Self::VsCode => "keyrill-public-fixture-v1.txt",
            Self::Word => "keyrill-public-fixture-v1.rtf",
        }
    }
    #[cfg(any(windows, test))]
    pub(crate) fn control_name_matches(self, name: &str) -> bool {
        if self == Self::Word {
            return word_provider_name_matches(name);
        }
        name == self.label()
            || (self == Self::VsCode && name == format!("{}, Editor content", self.label()))
    }
    #[cfg(any(windows, test))]
    pub(crate) fn document_name_matches(self, name: &str) -> bool {
        if self == Self::Word {
            return word_provider_name_matches(name);
        }
        name == if self.browser() { MARKER } else { self.label() }
    }
    /// Whether the selector names a local browser logical control.
    pub fn browser(self) -> bool {
        !matches!(self, Self::VsCode | Self::Word)
    }
    /// Exact suffix including the Word provider's terminal paragraph mark.
    pub fn suffix(self) -> &'static str {
        if self == Self::Word {
            " Fixture end.\r"
        } else {
            SUFFIX
        }
    }
}

#[cfg(any(windows, test))]
fn word_provider_name_matches(name: &str) -> bool {
    name == Fixture::Word.label()
        || name == format!("{}  -  Compatibility Mode", Fixture::Word.label())
}

/// Require complete public text, unchanged prefix/suffix, and exact collapsed UTF-16 caret.
///
/// # Errors
/// Refuses incomplete identity, unsafe metadata, changed document, or wrong selection.
pub fn check(
    provider: &mut impl Provider,
    fixture: Fixture,
    case: PublicCase,
) -> Result<(), Refusal> {
    let initial = provider.metadata();
    let revalidate = |provider: &mut dyn Provider| {
        let now = provider.metadata();
        let refusal = field::eligible_public(&now);
        if refusal != Refusal::None {
            Err(refusal)
        } else if now != initial {
            Err(Refusal::Stale)
        } else if !now.text_pattern {
            Err(Refusal::TextUnavailable)
        } else {
            Ok(())
        }
    };
    revalidate(provider)?;
    let prefix: Vec<_> = PREFIX.encode_utf16().collect();
    let word: Vec<_> = case.text().encode_utf16().collect();
    let suffix: Vec<_> = fixture.suffix().encode_utf16().collect();
    let expected: Vec<_> = prefix.iter().chain(&word).chain(&suffix).copied().collect();
    if provider.document(expected.len() + 1)? != expected {
        return Err(Refusal::TextMismatch);
    }
    revalidate(provider)?;
    let before: Vec<_> = prefix.iter().chain(&word).copied().collect();
    provider.public_selection(&before, &[], &suffix)?;
    revalidate(provider)
}

/// Prepare deterministic local browser, VS Code, and Word documents.
/// Existing files must match these public fixture bytes exactly.
///
/// # Errors
/// Returns filesystem failures or a conflicting existing destination.
pub fn generate(directory: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    let plain = format!("{PREFIX}{SUFFIX}");
    let mut rtf = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Consolas;}}\\f0\\fs24 ");
    for unit in plain.encode_utf16() {
        match unit {
            0x20..=0x7e if ![b'{' as u16, b'}' as u16, b'\\' as u16].contains(&unit) => {
                rtf.push(char::from(unit as u8))
            }
            _ => rtf.push_str(&format!("\\u{}?", unit as i16)),
        }
    }
    rtf.push_str("\\par}");
    let files = [
        (
            "keyrill-public-fixture-v1.html",
            include_str!("../../../macos/tests/browser-fixture.html").as_bytes(),
        ),
        ("keyrill-public-fixture-v1.txt", plain.as_bytes()),
        ("keyrill-public-fixture-v1.rtf", rtf.as_bytes()),
    ];
    std::fs::create_dir_all(directory)?;
    for (name, content) in files {
        let path = directory.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => file.write_all(content)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if std::fs::read(path)? != content {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "public fixture destination differs",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{Metadata, NativeFocus, ReadOnly, WindowFacts};
    struct Fake {
        metadata: Metadata,
        text: Vec<u16>,
        before: Vec<u16>,
        selected: Vec<u16>,
        after: Vec<u16>,
        reads: u32,
    }
    impl Provider for Fake {
        fn metadata(&mut self) -> Metadata {
            self.metadata.clone()
        }
        fn document(&mut self, bound: usize) -> Result<Vec<u16>, Refusal> {
            self.reads += 1;
            Ok(self.text.iter().copied().take(bound).collect())
        }
        fn end_caret(&mut self) -> Result<(), Refusal> {
            Err(Refusal::Selection)
        }
        fn public_selection(
            &mut self,
            before: &[u16],
            selected: &[u16],
            after: &[u16],
        ) -> Result<(), Refusal> {
            if before == self.before && selected == self.selected && after == self.after {
                Ok(())
            } else {
                Err(Refusal::Selection)
            }
        }
    }
    fn fake(fixture: Fixture, case: PublicCase) -> Fake {
        let window = WindowFacts {
            complete: true,
            enabled: true,
            ..Default::default()
        };
        let before: Vec<_> = format!("{PREFIX}{}", case.text()).encode_utf16().collect();
        let after: Vec<_> = fixture.suffix().encode_utf16().collect();
        Fake {
            metadata: Metadata {
                refusal: Refusal::None,
                native: NativeFocus {
                    focused: window.clone(),
                    root: window,
                    ..Default::default()
                },
                exact_binding: true,
                control_type: 50004,
                nonpassword: true,
                enabled: true,
                value_readonly: ReadOnly::Writable,
                text_readonly: ReadOnly::Writable,
                text_pattern: true,
                ..Default::default()
            },
            text: before.iter().chain(&after).copied().collect(),
            before,
            selected: Vec::new(),
            after,
            reads: 0,
        }
    }
    #[test]
    fn word_provider_names_accept_exact_public_representations() {
        for name in [
            "keyrill-public-fixture-v1.rtf",
            "keyrill-public-fixture-v1.rtf  -  Compatibility Mode",
        ] {
            assert!(Fixture::Word.control_name_matches(name), "control {name:?}");
            assert!(
                Fixture::Word.document_name_matches(name),
                "document {name:?}"
            );
        }
    }
    #[test]
    fn word_provider_names_reject_unrecognized_representations() {
        for name in [
            "",
            "Page 1",
            "keyrill-public-fixture-v1.rtf - Word",
            "keyrill-public-fixture-v1.rtf - Compatibility Mode",
            "keyrill-public-fixture-v1.rtf  - Compatibility Mode",
            "keyrill-public-fixture-v1.rtf -  Compatibility Mode",
            "keyrill-public-fixture-v1.rtf-Compatibility Mode",
            "keyrill-public-fixture-v1.rtf  -  Compatibility Mode - Word",
            "keyrill-public-fixture-v1.rtf  -  Compatibility Mode extra",
            "keyrill-public-fixture-v1.rtf.backup  -  Compatibility Mode",
            "private-document.rtf  -  Compatibility Mode",
            "keyrill-public-fixture-v1  -  Compatibility Mode",
            "keyrill-public-fixture-v1.txt  -  Compatibility Mode",
            " keyrill-public-fixture-v1.rtf",
            "keyrill-public-fixture-v1.rtf ",
            " keyrill-public-fixture-v1.rtf  -  Compatibility Mode",
            "keyrill-public-fixture-v1.rtf  -  Compatibility Mode ",
            "Keyrill-public-fixture-v1.rtf  -  Compatibility Mode",
            "keyrill-public-fixture-v1.rtf  -  compatibility mode",
            "keyrill-public-fixture-v1.rtf  -  Режим совместимости",
            "keyrill-public-fixture-v1.rtf\0",
            "keyrill-public-fixture-v1.rtf  -  Compatibility Mode\0",
            "keyrill-public-fixture-v1.rtf\u{a0}\u{a0}-  Compatibility Mode",
            "keyrill-public-fixture-v1.rtf  \u{2013}  Compatibility Mode",
        ] {
            assert!(
                !Fixture::Word.control_name_matches(name),
                "control {name:?}"
            );
            assert!(
                !Fixture::Word.document_name_matches(name),
                "document {name:?}"
            );
        }
    }
    #[test]
    fn word_provider_names_are_rejected_by_other_fixtures() {
        let name = "keyrill-public-fixture-v1.rtf  -  Compatibility Mode";
        for fixture in [
            Fixture::Input,
            Fixture::Textarea,
            Fixture::Plain,
            Fixture::Rich,
            Fixture::FieldB,
            Fixture::VsCode,
        ] {
            assert!(!fixture.control_name_matches(name), "control {fixture:?}");
            assert!(!fixture.document_name_matches(name), "document {fixture:?}");
        }
    }
    #[test]
    fn wrong_field_tab_and_private_document_names_refuse_public_identity() {
        for fixture in [
            Fixture::Input,
            Fixture::Textarea,
            Fixture::Plain,
            Fixture::Rich,
            Fixture::FieldB,
            Fixture::VsCode,
            Fixture::Word,
        ] {
            assert!(fixture.control_name_matches(fixture.label()));
            assert!(!fixture.control_name_matches("Password"));
            assert!(!fixture.control_name_matches("private-document.txt"));
            assert!(!fixture.document_name_matches("Private tab"));
            assert!(!fixture.document_name_matches("Keyrill public fixture v1 extra"));
            assert!(!fixture.document_name_matches(""));
        }
        assert!(!Fixture::Input.control_name_matches(Fixture::FieldB.label()));
        assert!(!Fixture::VsCode.document_name_matches(Fixture::Word.label()));
    }
    #[test]
    fn public_document_prefix_suffix_and_utf16_caret_are_exact() {
        assert_eq!(PREFIX.encode_utf16().count(), PREFIX.chars().count() + 1);
        for fixture in [
            Fixture::Input,
            Fixture::Textarea,
            Fixture::Plain,
            Fixture::Rich,
            Fixture::FieldB,
            Fixture::VsCode,
            Fixture::Word,
        ] {
            for case in PublicCase::ALL {
                let mut provider = fake(fixture, case);
                assert_eq!(check(&mut provider, fixture, case), Ok(()));
                provider.text.push(b'x' as u16);
                assert_eq!(
                    check(&mut provider, fixture, case),
                    Err(Refusal::TextMismatch)
                );
                provider.text.pop();
                provider.before.pop();
                assert_eq!(check(&mut provider, fixture, case), Err(Refusal::Selection));
                provider = fake(fixture, case);
                provider.selected.push(b'x' as u16);
                assert_eq!(check(&mut provider, fixture, case), Err(Refusal::Selection));
                provider = fake(fixture, case);
                provider.text[0] = b'X' as u16;
                assert_eq!(
                    check(&mut provider, fixture, case),
                    Err(Refusal::TextMismatch)
                );
            }
        }
    }
    #[test]
    fn excluded_or_unavailable_metadata_never_reads_public_or_secret_text() {
        for failure in [
            Refusal::Binding,
            Refusal::Stale,
            Refusal::Provider,
            Refusal::Password,
            Refusal::ReadOnly,
            Refusal::NativeFocus,
        ] {
            let mut provider = fake(Fixture::Input, PublicCase::Empty);
            provider.metadata.refusal = failure;
            assert_eq!(
                check(&mut provider, Fixture::Input, PublicCase::Empty),
                Err(failure)
            );
            assert_eq!(provider.reads, 0);
        }
        for readonly in [ReadOnly::Unknown, ReadOnly::ReadOnly, ReadOnly::Mixed] {
            let mut provider = fake(Fixture::Input, PublicCase::Empty);
            provider.metadata.value_readonly = readonly;
            assert!(check(&mut provider, Fixture::Input, PublicCase::Empty).is_err());
            assert_eq!(provider.reads, 0);
        }
    }
    #[test]
    fn fixture_generation_is_repeatable_and_preserves_conflicting_files() {
        let directory = std::env::temp_dir().join(format!(
            "keyrill-public-fixture-test-{}",
            std::process::id()
        ));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        std::fs::create_dir(&directory).unwrap();
        let _cleanup = Cleanup(directory.clone());
        generate(&directory).unwrap();
        generate(&directory).unwrap();
        let text = std::fs::read_to_string(directory.join(Fixture::VsCode.label())).unwrap();
        assert_eq!(text, format!("{PREFIX}{SUFFIX}"));
        let rtf = std::fs::read_to_string(directory.join(Fixture::Word.label())).unwrap();
        assert!(rtf.contains("\\u-10179?\\u-8704?"));
        let path = directory.join(Fixture::VsCode.label());
        std::fs::write(&path, b"conflicting public fixture").unwrap();
        assert_eq!(
            generate(&directory).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(path).unwrap(), b"conflicting public fixture");
    }
}
