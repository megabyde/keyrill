//! Package validation never loads or executes candidate binaries.
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
use crate::{Digest, Target, pe};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
#[cfg(not(windows))]
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

const MAX_JSON_BYTES: u64 = 1024 * 1024;
const MAX_RESOURCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PACKAGE_FILES: usize = 256;
const MAX_PACKAGE_ENTRIES: usize = 2 * MAX_PACKAGE_FILES;

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

fn byte_limit(name: &str) -> u64 {
    if name.ends_with(".dll") || name.ends_with(".exe") {
        MAX_PE_BYTES
    } else if name.ends_with(".json") {
        MAX_JSON_BYTES
    } else {
        MAX_RESOURCE_BYTES
    }
}

fn observed_length(file: &File, limit: u64) -> Result<u64> {
    let length = file.metadata()?.len();
    if length > limit {
        return Err("package file exceeds byte limit".into());
    }
    Ok(length)
}

fn read_observed(
    reader: &mut impl Read,
    length: u64,
    mut consume: impl FnMut(&[u8]),
) -> Result<()> {
    let mut bounded = reader.take(length + 1);
    let mut buffer = [0u8; 16 * 1024];
    let mut total = 0;
    loop {
        let count = bounded.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > length {
            return Err("package file length changed".into());
        }
        consume(&buffer[..count]);
    }
    if total != length {
        return Err("package file length changed".into());
    }
    Ok(())
}

fn read_file(file: &mut File, length: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(usize::try_from(length)?)?;
    read_observed(file, length, |chunk| bytes.extend_from_slice(chunk))?;
    Ok(bytes)
}

fn hash_file(file: &mut File, limit: u64) -> Result<Digest> {
    let length = observed_length(file, limit)?;
    file.rewind()?;
    let mut hash = Sha256::new();
    read_observed(file, length, |chunk| hash.update(chunk))?;
    Ok(Digest(hash.finalize().into()))
}

fn open_entry(path: &Path, directory: bool) -> Result<File> {
    #[cfg(windows)]
    let file = windows::open(path, directory)?;
    #[cfg(not(windows))]
    let file = {
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err("package contains a symbolic link".into());
        }
        OpenOptions::new().read(true).open(path)?
    };
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
        const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x40000;
        const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x400000;
        if metadata.file_attributes()
            & (FILE_ATTRIBUTE_REPARSE_POINT
                | FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0
        {
            return Err("package contains a reparse or unavailable entry".into());
        }
    }
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err("package contains an unexpected entry type".into());
    }
    Ok(file)
}

/// Retain a local Windows file or directory without following any path reparse point.
///
/// # Errors
/// Rejects non-local, reparsed, unavailable or incorrectly typed paths and failed native opens.
#[cfg(windows)]
pub fn retain(path: &Path, directory: bool) -> Result<File> {
    open_entry(path, directory)
}

fn retain_root(directory: &Path) -> Result<(PathBuf, Vec<File>)> {
    let directory = std::path::absolute(directory)?;
    #[cfg(windows)]
    let leases = {
        use std::path::{Component, Prefix};
        if !matches!(directory.components().next(),Some(Component::Prefix(p)) if matches!(p.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
        {
            return Err("release package must use a local filesystem path".into());
        }
        let mut ancestors: Vec<_> = directory.ancestors().collect();
        ancestors.reverse();
        let mut leases = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            leases.push(open_entry(ancestor, true)?);
        }
        leases
    };
    #[cfg(not(windows))]
    let leases = vec![open_entry(&directory, true)?];
    #[cfg(not(windows))]
    let directory = directory.canonicalize()?;
    Ok((directory, leases))
}

/// Exact production executables in each client architecture.
pub const BINARIES: [&str; 4] = [
    "KeyrillTsf.dll",
    "KeyrillSettings.exe",
    "KeyrillRegister.exe",
    "KeyrillSetup.exe",
];
/// Fixed non-executable release resources.
pub const RESOURCES: [&str; 7] = [
    "resources/english.ico",
    "resources/russian.ico",
    "LICENSE",
    "model-notice.txt",
    "wordfreq-code-license.txt",
    "model-provenance.json",
    "licenses/dependencies.json",
];

/// Recognized upstream license-text filenames; their actual case is retained.
pub const LICENSE_NAMES: &[&str] = &[
    "LICENSE",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "LICENSE.txt",
    "LICENSE.md",
    "COPYING",
    "COPYRIGHT",
    "LICENSE-0BSD",
    "UNLICENSE",
    "LICENSE-APACHE.md",
    "LICENSE-MIT.md",
    "LICENSE-ZLIB.md",
    "LICENSE-BOOST",
    "LICENSE-UNICODE",
    "LICENSE-BSD",
    "license-apache-2.0",
    "license-mit",
];

/// A complete unsigned package whose bytes match its bounded manifest.
pub struct Package {
    directory: PathBuf,
    version: String,
    production: [String; 2],
    digest: String,
    files: BTreeMap<String, String>,
}

/// Return whether a string is a canonical lowercase SHA-256 identity.
#[must_use]
pub fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn text<'a>(object: &'a Value, key: &str) -> Result<&'a str> {
    object[key]
        .as_str()
        .ok_or_else(|| "package manifest field missing".into())
}

fn allowed_file(name: &str) -> bool {
    if RESOURCES.contains(&name) {
        return true;
    }
    if let Some((architecture, file)) = name.split_once('/')
        && ["x64", "x86"].contains(&architecture)
        && BINARIES.contains(&file)
    {
        return true;
    }
    let mut parts = name.split('/');
    let (Some("licenses"), Some(dependency), Some(file), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !["", ".", ".."].contains(&dependency)
        && dependency
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
        && LICENSE_NAMES.contains(&file)
}

struct Directory {
    path: PathBuf,
    file: File,
}

fn walk(
    root: &Path,
    directory: Directory,
    expected: &BTreeSet<String>,
    files: &mut BTreeMap<String, (File, u64)>,
    leases: &mut Vec<File>,
    entries: &mut usize,
    total_bytes: &mut u64,
) -> Result<()> {
    #[cfg(windows)]
    let names = windows::entries(&directory.file, MAX_PACKAGE_ENTRIES - *entries)?;
    #[cfg(not(windows))]
    let names = {
        let mut names = Vec::new();
        for entry in fs::read_dir(&directory.path)? {
            if names.len() == MAX_PACKAGE_ENTRIES - *entries {
                return Err("package contains too many entries".into());
            }
            let entry = entry?;
            if entry.file_type()?.is_symlink() {
                return Err("package contains a symbolic link".into());
            }
            names.push(
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "non-Unicode package path")?,
            );
        }
        names
    };
    for entry in names {
        *entries += 1;
        if *entries > MAX_PACKAGE_ENTRIES {
            return Err("package contains too many entries".into());
        }
        let path = directory.path.join(entry);
        let name = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("non-Unicode package path")?
            .replace('\\', "/");
        if expected.contains(&name) {
            let file = open_entry(&path, false)?;
            let length = observed_length(&file, byte_limit(&name))?;
            *total_bytes = total_bytes
                .checked_add(length)
                .filter(|&total| total <= MAX_PACKAGE_BYTES)
                .ok_or("package exceeds total byte limit")?;
            files.insert(name, (file, length));
        } else if name.split('/').count() <= 2
            && expected.iter().any(|file| {
                file.strip_prefix(&name)
                    .is_some_and(|tail| tail.starts_with('/'))
            })
        {
            let child = Directory {
                file: open_entry(&path, true)?,
                path,
            };
            walk(root, child, expected, files, leases, entries, total_bytes)?;
        } else {
            return Err("package contains an unapproved entry".into());
        }
    }
    leases.push(directory.file);
    Ok(())
}

/// Reject build-machine paths in both narrow and wide binary strings.
///
/// # Errors
/// Returns an error when a developer home or workspace path is embedded.
pub fn reject_developer_paths(bytes: &[u8]) -> Result<()> {
    fn contains_path(bytes: &[u8], marker: &[u8], wide: bool) -> bool {
        bytes
            .windows(marker.len())
            .enumerate()
            .any(|(index, window)| {
                if !window.eq_ignore_ascii_case(marker) {
                    return false;
                }
                let tail = &bytes[index + marker.len()..];
                let step = if wide { 2 } else { 1 };
                for (letters, index) in (0..tail.len().min(256)).step_by(step).enumerate() {
                    let byte = tail[index];
                    if wide && tail.get(index + 1) != Some(&0) {
                        return false;
                    }
                    if byte == b'/' || byte == b'\\' {
                        return letters > 0;
                    }
                    if !(byte.is_ascii_alphanumeric() || b" -_.".contains(&byte)) {
                        return false;
                    }
                }
                false
            })
    }
    for marker in [
        ":\\users\\",
        "\\projects\\keyrill\\",
        "/home/",
        "/users/",
        "\\.cargo\\",
        "\\.rustup\\",
    ] {
        let wide: Vec<_> = marker.encode_utf16().flat_map(u16::to_le_bytes).collect();
        if contains_path(bytes, marker.as_bytes(), false) || contains_path(bytes, &wide, true) {
            return Err(
                "release artifact contains a developer path; rebuild with path remapping".into(),
            );
        }
    }
    Ok(())
}

impl Package {
    /// Validate the complete directory, manifest, architecture and public export boundary.
    ///
    /// # Errors
    /// Rejects incomplete, altered, extra, test, unsupported or malformed package contents.
    pub fn open(directory: &Path) -> Result<Self> {
        let (directory, mut directory_leases) = retain_root(directory)?;
        let mut manifest_file = open_entry(&directory.join("package.json"), false)?;
        let manifest_length = observed_length(&manifest_file, MAX_JSON_BYTES)?;
        let manifest_bytes = read_file(&mut manifest_file, manifest_length)?;
        let manifest: Value = serde_json::from_slice(&manifest_bytes)?;
        if manifest["schema"] != 1 || manifest["unsigned"] != true {
            return Err("unsupported package manifest".into());
        }
        for key in ["source", "model", "dependencies"] {
            if !is_digest(text(&manifest, key)?) {
                return Err("invalid package provenance digest".into());
            }
        }
        let commit = text(&manifest, "commit")?;
        if commit.len() != 40 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid source commit".into());
        }
        let version = text(&manifest, "version")?;
        if version != env!("CARGO_PKG_VERSION") {
            return Err("unsupported package version".into());
        }
        let compiler = text(&manifest, "compiler")?;
        if compiler.len() > 1024 || !compiler.starts_with("rustc ") {
            return Err("invalid compiler provenance".into());
        }
        let records = manifest["files"]
            .as_object()
            .ok_or("package files missing")?;
        if records.len() >= MAX_PACKAGE_FILES {
            return Err("package contains too many files".into());
        }
        let mut files = BTreeMap::new();
        for (name, hash) in records {
            if !allowed_file(name) {
                return Err("unapproved release file".into());
            }
            let hash = hash
                .as_str()
                .filter(|v| is_digest(v))
                .ok_or("invalid release file hash")?;
            files.insert(name.clone(), hash.to_owned());
        }
        for name in RESOURCES {
            if !files.contains_key(name) {
                return Err("required release resource missing".into());
            }
        }
        for target in [Target::X64, Target::X86] {
            let arch = &manifest["architectures"][target.name()];
            let production=Digest::of_bytes(format!("keyrill-windows-v2\nsource={}\ntarget={}\nprofile=release\ninstrumentation=none\nfeatures=\n",text(&manifest,"source")?,target.triple()).as_bytes()).to_string();
            if arch["target"] != target.triple() || text(arch, "production")? != production {
                return Err("release architecture provenance missing".into());
            }
            for binary in BINARIES {
                if !files.contains_key(&format!("{}/{binary}", target.name())) {
                    return Err("both production client architectures are required".into());
                }
            }
        }
        let mut actual = BTreeMap::new();
        let mut expected: BTreeSet<_> = files.keys().cloned().collect();
        expected.insert("package.json".into());
        let root = Directory {
            path: directory.clone(),
            file: directory_leases.pop().ok_or("missing package root lease")?,
        };
        walk(
            &directory,
            root,
            &expected,
            &mut actual,
            &mut directory_leases,
            &mut 0,
            &mut 0,
        )?;
        if !actual.keys().eq(expected.iter()) {
            return Err("package contains missing or stale files".into());
        }
        let mut provenance = Value::Null;
        let mut dependencies = Value::Null;
        for (name, expected) in &files {
            let (file, length) = actual.get_mut(name).ok_or("missing package file")?;
            let bytes = read_file(file, *length)?;
            if Digest::of_bytes(&bytes).to_string() != *expected {
                return Err("package file hash mismatch".into());
            }
            if name.ends_with(".dll") || name.ends_with(".exe") {
                let target: Target = name
                    .split('/')
                    .next()
                    .ok_or("missing architecture")?
                    .parse()?;
                pe::validate(&bytes, target)?;
                reject_developer_paths(&bytes)?;
                if name.ends_with(".dll") {
                    pe::production(&bytes, target)?;
                }
            }
            if name.ends_with(".ico") && bytes != icon(name.contains("russian")) {
                return Err("profile icon resource differs from product resource".into());
            }
            match name.as_str() {
                "model-provenance.json" => provenance = serde_json::from_slice(&bytes)?,
                "licenses/dependencies.json" => dependencies = serde_json::from_slice(&bytes)?,
                _ => {}
            }
        }
        if provenance["output"]["sha256"] != manifest["model"] {
            return Err("model provenance missing".into());
        }
        let dependencies = dependencies
            .as_array()
            .filter(|v| !v.is_empty())
            .ok_or("dependency licenses missing")?;
        for dependency in dependencies {
            let license_files = dependency["files"]
                .as_array()
                .filter(|v| !v.is_empty())
                .ok_or("dependency license text missing")?;
            for file in license_files {
                if !files.contains_key(file.as_str().ok_or("invalid license path")?) {
                    return Err("dependency license text absent".into());
                }
            }
        }
        Ok(Self {
            directory,
            version: text(&manifest, "version")?.to_owned(),
            production: [
                text(&manifest["architectures"]["x64"], "production")?.to_owned(),
                text(&manifest["architectures"]["x86"], "production")?.to_owned(),
            ],
            digest: Digest::of_bytes(&manifest_bytes).to_string(),
            files,
        })
    }

    /// Return the immutable manifest identity used for the installed version directory.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
    /// Return the verified source directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    /// Return the verified product version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Return the verified production source identity for one architecture.
    #[must_use]
    pub fn production(&self, target: Target) -> &str {
        &self.production[match target {
            Target::X64 => 0,
            Target::X86 => 1,
        }]
    }

    /// Return a declared verified file digest.
    #[must_use]
    pub fn file_digest(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }

    /// Verify an already retained file against this package's content receipt.
    ///
    /// # Errors
    /// Rejects undeclared files, oversized or changed lengths and mismatched content.
    pub fn verify_file(&self, name: &str, file: &mut File) -> Result<()> {
        let expected = self.files.get(name).ok_or("undeclared package file")?;
        if hash_file(file, byte_limit(name))?.to_string() != *expected {
            return Err("package file hash mismatch".into());
        }
        Ok(())
    }
    /// Return the exact file receipt.
    #[must_use]
    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }
}

/// Deterministic 32-pixel opaque profile icon with a key glyph and language color.
#[must_use]
pub fn icon(russian: bool) -> Vec<u8> {
    let size = 40 + 32 * 32 * 4 + 32 * 4;
    let mut bytes = vec![0, 0, 1, 0, 1, 0, 32, 32, 0, 0, 1, 0, 32, 0];
    bytes.extend((size as u32).to_le_bytes());
    bytes.extend(22u32.to_le_bytes());
    bytes.extend(40u32.to_le_bytes());
    bytes.extend(32i32.to_le_bytes());
    bytes.extend(64i32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(32u16.to_le_bytes());
    bytes.extend([0u8; 24]);
    for y in (0..32i32).rev() {
        for x in 0..32i32 {
            let radius = (x - 11).pow(2) + (y - 12).pow(2);
            let key = (12..=42).contains(&radius)
                || ((15..=26).contains(&x) && (11..=14).contains(&y))
                || ((22..=25).contains(&x) && (13..=19).contains(&y));
            bytes.extend(if key {
                [255, 255, 255, 255]
            } else if russian {
                [130, 60, 160, 255]
            } else {
                [150, 105, 25, 255]
            });
        }
    }
    bytes.extend([0u8; 128]);
    bytes
}

#[cfg(test)]
pub(crate) mod tests;
