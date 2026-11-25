//! Bounded PE architecture and export inspection without loading executable code.
use crate::Target;
use std::collections::BTreeSet;
use std::io::{self, ErrorKind};

type Result<T> = io::Result<T>;
const MAX_SECTIONS: usize = 96;
const MAX_EXPORTS: usize = 128;
const MAX_EXPORT_NAME_BYTES: usize = 128;

fn invalid() -> io::Error {
    io::Error::new(
        ErrorKind::InvalidData,
        "invalid PE image or target architecture",
    )
}

fn bytes(image: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    image
        .get(offset..offset.checked_add(length).ok_or_else(invalid)?)
        .ok_or_else(invalid)
}

fn word(image: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes(image, offset, 2)?.try_into().map_err(|_| invalid())?,
    ))
}

fn dword(image: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes(image, offset, 4)?.try_into().map_err(|_| invalid())?,
    ))
}

struct Section {
    rva: u32,
    length: u32,
    raw: u32,
}

struct Image<'a> {
    bytes: &'a [u8],
    sections: Vec<Section>,
}

impl Image<'_> {
    fn offset(&self, rva: u32, length: usize) -> Result<usize> {
        let section = self
            .sections
            .iter()
            .find(|section| {
                rva.checked_sub(section.rva)
                    .is_some_and(|offset| offset < section.length)
            })
            .ok_or_else(invalid)?;
        let relative = rva - section.rva;
        if length > (section.length - relative) as usize {
            return Err(invalid());
        }
        let offset = section.raw.checked_add(relative).ok_or_else(invalid)? as usize;
        bytes(self.bytes, offset, length)?;
        Ok(offset)
    }

    fn name(&self, rva: u32) -> Result<String> {
        let offset = self.offset(rva, 1)?;
        let remaining = &self.bytes[offset..];
        let length = remaining
            .iter()
            .take(MAX_EXPORT_NAME_BYTES + 1)
            .position(|&byte| byte == 0)
            .ok_or_else(invalid)?;
        self.offset(rva, length + 1)?;
        let value = &remaining[..length];
        if value.is_empty() || !value.is_ascii() {
            return Err(invalid());
        }
        Ok(std::str::from_utf8(value)
            .map_err(|_| invalid())?
            .to_owned())
    }
}

struct Header {
    optional: usize,
    optional_bytes: usize,
    export_directory: usize,
}

fn header(image: &[u8], target: Target) -> Result<Header> {
    if bytes(image, 0, 2)? != b"MZ" {
        return Err(invalid());
    }
    let signature = dword(image, 0x3c)? as usize;
    if bytes(image, signature, 4)? != b"PE\0\0" {
        return Err(invalid());
    }
    let header = signature.checked_add(4).ok_or_else(invalid)?;
    bytes(image, header, 20)?;
    if word(image, header)? != target.pe_machine() {
        return Err(invalid());
    }
    let optional_bytes = usize::from(word(image, header + 16)?);
    let optional = header + 20;
    let export_directory = match target {
        Target::X64 => 112,
        Target::X86 => 96,
    };
    if optional_bytes < export_directory + 8 || word(image, optional)? != target.pe_magic() {
        return Err(invalid());
    }
    bytes(image, optional, optional_bytes)?;
    let count = usize::from(word(image, header + 2)?);
    if count == 0 || count > MAX_SECTIONS {
        return Err(invalid());
    }
    bytes(
        image,
        optional + optional_bytes,
        count.checked_mul(40).ok_or_else(invalid)?,
    )?;
    Ok(Header {
        optional,
        optional_bytes,
        export_directory,
    })
}

/// Validate the PE target architecture.
///
/// # Errors
/// Returns an error for a malformed image or wrong target.
pub fn validate(image: &[u8], target: Target) -> Result<()> {
    header(image, target).map(|_| ())
}

/// Read exported symbol names from a validated PE image.
///
/// # Errors
/// Returns an error for malformed export tables or wrong target.
pub fn exports(image: &[u8], target: Target) -> Result<BTreeSet<String>> {
    let Header {
        optional,
        optional_bytes,
        export_directory,
    } = header(image, target)?;
    if dword(image, optional + export_directory - 4)? == 0 {
        return Err(invalid());
    }
    let signature = dword(image, 0x3c)? as usize;
    let count = usize::from(word(image, signature + 6)?);
    let mut sections = Vec::new();
    let table = optional + optional_bytes;
    for index in 0..count {
        let offset = table
            .checked_add(index.checked_mul(40).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        bytes(image, offset, 40)?;
        sections.push(Section {
            rva: dword(image, offset + 12)?,
            length: dword(image, offset + 16)?,
            raw: dword(image, offset + 20)?,
        });
    }
    let parsed = Image {
        bytes: image,
        sections,
    };
    let export_rva = dword(image, optional + export_directory)?;
    let export_bytes = dword(image, optional + export_directory + 4)?;
    if export_rva == 0 || export_bytes < 40 {
        return Err(invalid());
    }
    let export = parsed.offset(export_rva, export_bytes as usize)?;
    let functions = dword(image, export + 20)? as usize;
    let names = dword(image, export + 24)? as usize;
    if names > functions || functions == 0 || names == 0 || functions > MAX_EXPORTS {
        return Err(invalid());
    }
    let pointers = parsed.offset(
        dword(image, export + 32)?,
        names.checked_mul(4).ok_or_else(invalid)?,
    )?;
    let ordinals = parsed.offset(
        dword(image, export + 36)?,
        names.checked_mul(2).ok_or_else(invalid)?,
    )?;
    let addresses = parsed.offset(
        dword(image, export + 28)?,
        functions.checked_mul(4).ok_or_else(invalid)?,
    )?;
    let mut result = BTreeSet::new();
    for index in 0..names {
        let ordinal = usize::from(word(image, ordinals + index * 2)?);
        if ordinal >= functions || dword(image, addresses + ordinal * 4)? == 0 {
            return Err(invalid());
        }
        let name = parsed.name(dword(image, pointers + index * 4)?)?;
        if !result.insert(name) {
            return Err(invalid());
        }
    }
    Ok(result)
}

/// Validate the production COM and frozen core C ABI export boundary.
///
/// # Errors
/// Rejects missing COM entry points, private test exports and unrecognized exports.
pub fn production(image: &[u8], target: Target) -> Result<()> {
    let actual = exports(image, target)?;
    let allowed = [
        "DllCanUnloadNow",
        "DllGetClassObject",
        "kr_abi_version",
        "kr_engine_new",
        "kr_engine_free",
        "kr_reset",
        "kr_set_automatic_policy",
        "kr_set_keyboard",
        "kr_set_dictionary_utf8",
        "kr_clear_dictionary",
        "kr_dictionary_skipped_lines",
        "kr_plan_event",
        "kr_commit",
        "kr_dictionary_parse_utf8",
        "kr_dictionary_free",
        "kr_adopt_dictionaries",
        "kr_prepare_model",
        "kr_vocabulary_new",
        "kr_vocabulary_free",
        "kr_vocabulary_configure",
        "kr_vocabulary_configure_default",
        "kr_vocabulary_observe",
        "kr_adopt_vocabulary",
        "kr_plan_compatibility_event",
        "kr_commit_compatibility",
    ];
    if !actual.contains("DllCanUnloadNow")
        || !actual.contains("DllGetClassObject")
        || actual.iter().any(|name| !allowed.contains(&name.as_str()))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "production DLL export boundary violated",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set16(image: &mut [u8], offset: usize, value: u16) {
        image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn set32(image: &mut [u8], offset: usize, value: u32) {
        image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn fixture(target: Target) -> Vec<u8> {
        let mut image = vec![0; 1024];
        image[..2].copy_from_slice(b"MZ");
        set32(&mut image, 0x3c, 64);
        image[64..68].copy_from_slice(b"PE\0\0");
        set16(&mut image, 68, target.pe_machine());
        set16(&mut image, 70, 1);
        set16(&mut image, 84, 120);
        set16(&mut image, 88, target.pe_magic());
        let directory = match target {
            Target::X64 => 200,
            Target::X86 => 184,
        };
        set32(&mut image, directory - 4, 1);
        set32(&mut image, directory, 0x1000);
        set32(&mut image, directory + 4, 40);
        set32(&mut image, 220, 0x1000);
        set32(&mut image, 224, 512);
        set32(&mut image, 228, 512);
        set32(&mut image, 532, 1);
        set32(&mut image, 536, 1);
        set32(&mut image, 540, 0x1040);
        set32(&mut image, 544, 0x1044);
        set32(&mut image, 548, 0x1048);
        set32(&mut image, 576, 0x2000);
        set32(&mut image, 580, 0x1050);
        image[592..605].copy_from_slice(b"DllCanUnload\0");
        image
    }

    #[test]
    fn reads_literal_x64_exports_and_rejects_truncation() {
        let image = fixture(Target::X64);
        assert_eq!(
            exports(&image, Target::X64).unwrap(),
            BTreeSet::from(["DllCanUnload".to_owned()])
        );
        for length in 0..605 {
            assert!(
                exports(&image[..length], Target::X64).is_err(),
                "truncation {length}"
            );
        }
    }

    #[test]
    fn validates_both_machine_magic_pairs_and_rejects_cross_bitness() {
        for target in [Target::X64, Target::X86] {
            let image = fixture(target);
            assert!(validate(&image, target).is_ok());
            assert_eq!(
                exports(&image, target).unwrap(),
                BTreeSet::from(["DllCanUnload".to_owned()])
            );
            let other = if target == Target::X64 {
                Target::X86
            } else {
                Target::X64
            };
            assert!(validate(&image, other).is_err());
            assert!(exports(&image, other).is_err());
            let mut wrong_magic = image.clone();
            set16(&mut wrong_magic, 88, other.pe_magic());
            assert!(validate(&wrong_magic, target).is_err());
            assert!(exports(&wrong_magic, target).is_err());
            for length in 0..605 {
                assert!(
                    exports(&image[..length], target).is_err(),
                    "{target:?} truncation {length}"
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_headers_offsets_and_name_tables() {
        for (offset, value) in [
            (0x3c, u32::MAX),
            (200, u32::MAX),
            (204, u32::MAX),
            (536, 2),
            (540, u32::MAX),
            (544, u32::MAX),
            (548, u32::MAX),
            (576, 0),
            (580, 0),
            (584, 1),
        ] {
            let mut image = fixture(Target::X64);
            set32(&mut image, offset, value);
            assert!(exports(&image, Target::X64).is_err(), "offset {offset}");
        }
        let mut image = fixture(Target::X64);
        set16(&mut image, 68, 0xaa64);
        assert!(exports(&image, Target::X64).is_err());
    }

    #[test]
    fn bounds_section_export_and_symbol_name_work() {
        let mut image = fixture(Target::X64);
        image.resize(8192, 0);
        set16(&mut image, 70, MAX_SECTIONS as u16);
        assert!(validate(&image, Target::X64).is_ok());
        set16(&mut image, 70, MAX_SECTIONS as u16 + 1);
        assert!(validate(&image, Target::X64).is_err());

        let mut image = fixture(Target::X64);
        image.resize(2048, 0);
        set32(&mut image, 224, 1536);
        set32(&mut image, 532, MAX_EXPORTS as u32);
        assert!(exports(&image, Target::X64).is_ok());
        set32(&mut image, 532, MAX_EXPORTS as u32 + 1);
        assert!(exports(&image, Target::X64).is_err());

        set32(&mut image, 532, 1);
        image[592..592 + MAX_EXPORT_NAME_BYTES].fill(b'A');
        image[592 + MAX_EXPORT_NAME_BYTES] = 0;
        assert!(exports(&image, Target::X64).is_ok());
        image[592 + MAX_EXPORT_NAME_BYTES] = b'A';
        image[593 + MAX_EXPORT_NAME_BYTES] = 0;
        assert!(exports(&image, Target::X64).is_err());
    }

    #[test]
    #[ignore = "requires explicit production and test-api DLL artifacts"]
    fn compiled_dll_feature_exports_are_separate() {
        for (variable, private) in [
            ("KEYRILL_PRODUCTION_DLL", false),
            ("KEYRILL_TEST_DLL", true),
        ] {
            let path = std::env::var_os(variable).expect("explicit DLL path required");
            let image = std::fs::read(path).unwrap();
            let target = std::env::var("KEYRILL_TARGET_ARCH")
                .unwrap_or_else(|_| "x64".to_owned())
                .parse()
                .unwrap();
            let names = exports(&image, target).unwrap();
            assert!(names.contains("DllCanUnloadNow"));
            assert!(names.contains("DllGetClassObject"));
            assert_eq!(
                names.iter().any(|name| name.starts_with("KeyrillTest")),
                private
            );
        }
    }
}
