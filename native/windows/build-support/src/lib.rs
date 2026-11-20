//! Shared source and artifact identities for the Rust Windows build.
#![deny(unsafe_code)]

extern crate self as keyrill_windows_build;

use sha2::{Digest as _, Sha256};
use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Supported Windows client process architectures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Intel and AMD 64-bit processes.
    X64,
    /// Intel and AMD 32-bit processes.
    X86,
}

impl Target {
    /// Return the command-line architecture name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::X86 => "x86",
        }
    }

    /// Return the authoritative MSVC Cargo target triple.
    #[must_use]
    pub const fn triple(self) -> &'static str {
        match self {
            Self::X64 => "x86_64-pc-windows-msvc",
            Self::X86 => "i686-pc-windows-msvc",
        }
    }

    /// Return the COFF machine value.
    #[must_use]
    pub const fn pe_machine(self) -> u16 {
        match self {
            Self::X64 => 0x8664,
            Self::X86 => 0x014c,
        }
    }

    /// Return the PE optional header magic.
    #[must_use]
    pub const fn pe_magic(self) -> u16 {
        match self {
            Self::X64 => 0x020b,
            Self::X86 => 0x010b,
        }
    }

    /// Return the native pointer width in bits.
    #[must_use]
    pub const fn pointer_bits(self) -> u32 {
        match self {
            Self::X64 => 64,
            Self::X86 => 32,
        }
    }

    /// Return the activation manifest's architecture name.
    #[must_use]
    pub const fn manifest_architecture(self) -> &'static str {
        match self {
            Self::X64 => "amd64",
            Self::X86 => "x86",
        }
    }
}

impl std::str::FromStr for Target {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "x64" | "x86_64-pc-windows-msvc" => Ok(Self::X64),
            "x86" | "i686-pc-windows-msvc" => Ok(Self::X86),
            _ => Err("supported Windows architectures are x64 and x86"),
        }
    }
}

/// SHA-256 bytes with their content identity representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Digest([u8; 32]);

impl Digest {
    /// Return the SHA-256 bytes in their standard order.
    #[must_use]
    pub fn bytes(self) -> [u8; 32] {
        self.0
    }

    /// Hash the complete byte slice.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Return the eight big-endian words used by the frozen diagnostic protocol.
    #[must_use]
    pub fn words(self) -> [u32; 8] {
        std::array::from_fn(|index| {
            let start = index * 4;
            u32::from_be_bytes([
                self.0[start],
                self.0[start + 1],
                self.0[start + 2],
                self.0[start + 3],
            ])
        })
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(output, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Production and controlled-test DLL feature domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// The installed input method.
    Production,
    /// The separately built DLL with its private test exports.
    TestApi,
}

impl Variant {
    fn features(self) -> &'static str {
        match self {
            Self::Production => "",
            Self::TestApi => "test-api",
        }
    }
}

/// Instrumentation represented in an artifact identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instrumentation {
    /// Ordinary compilation.
    None,
    /// Rust AddressSanitizer compilation.
    Address,
}

impl Instrumentation {
    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Address => "address",
        }
    }
}

/// Cargo configuration that distinguishes independently built artifacts.
#[derive(Clone, Copy, Debug)]
pub struct BuildConfig<'a> {
    /// Cargo target triple.
    pub target: &'a str,
    /// Cargo profile name.
    pub profile: &'a str,
    /// Instrumentation selected by the build runner.
    pub instrumentation: Instrumentation,
}

/// A normalized source identity and both DLL feature identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Source records before configuration tagging.
    pub source: Digest,
    /// Ordinary public input-method feature set.
    pub production: Digest,
    /// Private controlled-test feature set.
    pub test_api: Digest,
}

impl Identity {
    /// Select the requested feature identity.
    #[must_use]
    pub fn variant(&self, variant: Variant) -> Digest {
        match variant {
            Variant::Production => self.production,
            Variant::TestApi => self.test_api,
        }
    }
}

fn source_file(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|name| name == "CMakeLists.txt")
    {
        return true;
    }
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(
            "rs" | "toml"
                | "h"
                | "c"
                | "cpp"
                | "json"
                | "jsonl"
                | "def"
                | "ps1"
                | "py"
                | "manifest"
                | "rc"
        )
    )
}

const EXCLUDED_DIRECTORIES: &[&str] = &["target", ".git", ".audit"];

fn collect_directory(directory: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    if !directory.try_exists()? {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "build identity requires self-contained source inputs",
            ));
        }
        if kind.is_dir() {
            if !entry
                .file_name()
                .to_str()
                .is_some_and(|name| EXCLUDED_DIRECTORIES.contains(&name))
            {
                collect_directory(&path, paths)?;
            }
        } else if kind.is_file() && source_file(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

const ROOT_INPUTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "LICENSE",
    "rust-toolchain.toml",
    ".cargo/config.toml",
    "tools/verify",
    "tools/pack-model.py",
    "native/macos/tests/browser-fixture.html",
    "assets/model.bin",
    "assets/model-provenance.json",
    "assets/model-notice.txt",
    "assets/wordfreq-code-license.txt",
    "model/data/model.inc.gz",
    "model/data/model-manifest.json",
    "model/data/model-notice.txt",
    "model/data/wordfreq-code-license.txt",
];
const SOURCE_DIRECTORIES: &[&str] = &["include", "crates", "native/windows", "tools/xtask"];

/// Identify a repository-relative source input, including a deleted or untracked path.
#[must_use]
pub fn is_source_input(path: &Path) -> bool {
    if ROOT_INPUTS.iter().any(|input| path == Path::new(input)) {
        return true;
    }
    SOURCE_DIRECTORIES.iter().any(|directory| {
        path.strip_prefix(directory).is_ok_and(|relative| {
            !relative.components().any(|component| {
                component
                    .as_os_str()
                    .to_str()
                    .is_some_and(|name| EXCLUDED_DIRECTORIES.contains(&name))
            }) && source_file(relative)
        })
    })
}

/// Enumerate the source inputs in sorted repository-relative order.
///
/// # Errors
/// Returns filesystem errors or an error for a symbolic link in an input tree.
pub fn source_inputs(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for file in ROOT_INPUTS {
        let path = root.join(file);
        if path.is_file() {
            paths.push(path);
        }
    }
    for directory in SOURCE_DIRECTORIES {
        collect_directory(&root.join(directory), &mut paths)?;
    }
    paths.sort_by_cached_key(|path| relative_path(root, path));
    paths.dedup();
    Ok(paths)
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("enumerated source is inside the workspace")
        .to_string_lossy()
        .replace('\\', "/")
}

fn normalized_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let bytes = fs::read(path)?;
    if matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("bin" | "gz")
    ) {
        return Ok(bytes);
    }
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            index += 1;
        }
        normalized.push(bytes[index]);
        index += 1;
    }
    Ok(normalized)
}

/// Calculate source and configured artifact identities with the v2 domain.
///
/// # Errors
/// Returns source enumeration or file-read errors.
pub fn identity(root: &Path, config: BuildConfig<'_>) -> io::Result<Identity> {
    let mut records = String::new();
    for path in source_inputs(root)? {
        let content = Digest::of_bytes(&normalized_bytes(&path)?);
        writeln!(records, "{}:{content}", relative_path(root, &path))
            .expect("formatting into a String cannot fail");
    }
    let source = Digest::of_bytes(records.as_bytes());
    let artifact = |variant: Variant| {
        Digest::of_bytes(
            format!(
                "keyrill-windows-v2\nsource={source}\ntarget={}\nprofile={}\ninstrumentation={}\nfeatures={}\n",
                config.target,
                config.profile,
                config.instrumentation.label(),
                variant.features(),
            )
            .as_bytes(),
        )
    };
    Ok(Identity {
        source,
        production: artifact(Variant::Production),
        test_api: artifact(Variant::TestApi),
    })
}

/// Write the runtime's selected Rust identity constant.
///
/// # Errors
/// Returns an output filesystem error.
pub fn write_rust_identity(path: &Path, digest: Digest) -> io::Result<()> {
    let words = digest
        .words()
        .map(|word| format!("0x{word:08x}"))
        .join(", ");
    fs::write(
        path,
        format!("pub(crate) const BUILD_DIGEST: [u32; 8] = [{words}];\n"),
    )
}

#[cfg(test)]
mod tests;

/// Portable PE validation shared by building and packaging.
pub mod pe;

/// Complete unsigned production package validation.
pub mod artifact;
