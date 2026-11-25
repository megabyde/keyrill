//! Explicit public-input child for independent Terminal byte-delivery checks.

use std::io::{self, Read};
use std::process::ExitCode;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::ReadFile;
use windows::Win32::System::Console::*;

const LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    ConvertedEn,
    ConvertedRu,
    OriginalEn,
    OriginalRu,
    Continuation,
}

impl Scenario {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "converted-en" => Some(Self::ConvertedEn),
            "converted-ru" => Some(Self::ConvertedRu),
            "original-en" => Some(Self::OriginalEn),
            "original-ru" => Some(Self::OriginalRu),
            "continuation" => Some(Self::Continuation),
            _ => None,
        }
    }

    fn expected(self) -> &'static [u8] {
        match self {
            Self::ConvertedEn => "привет \r\n".as_bytes(),
            Self::ConvertedRu => b"world \r\n",
            Self::OriginalEn => b"ghbdtn \r\n",
            Self::OriginalRu => "цщкдв \r\n".as_bytes(),
            Self::Continuation => "привет x\r\n".as_bytes(),
        }
    }
}

struct Console {
    input: HANDLE,
    mode: CONSOLE_MODE,
    code_page: u32,
    restored: bool,
}

impl Console {
    fn prepare(echo: bool) -> windows_core::Result<Self> {
        // SAFETY: the standard handle is queried without transferring ownership.
        let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) }?;
        let mut mode = CONSOLE_MODE::default();
        // SAFETY: input is a borrowed handle and mode is distinct writable storage.
        unsafe { GetConsoleMode(input, &mut mode) }?;
        // SAFETY: this read-only query has no pointer arguments.
        let code_page = unsafe { GetConsoleCP() };
        if code_page == 0 {
            return Err(windows_core::Error::from_thread());
        }
        let mut console = Self {
            input,
            mode,
            code_page,
            restored: false,
        };
        let mut desired = mode | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT;
        if echo {
            desired |= ENABLE_ECHO_INPUT;
        } else {
            desired &= !ENABLE_ECHO_INPUT;
        }
        // SAFETY: the live borrowed console handle and scalar code page are valid.
        unsafe {
            SetConsoleMode(input, desired)?;
            SetConsoleCP(65001)?;
        }
        let mut actual = CONSOLE_MODE::default();
        // SAFETY: actual is writable and the handle remains live.
        unsafe { GetConsoleMode(input, &mut actual) }?;
        // SAFETY: this read-only query has no pointer arguments.
        if actual != desired || unsafe { GetConsoleCP() } != 65001 {
            console.restore()?;
            return Err(windows_core::Error::from_hresult(
                windows::Win32::Foundation::E_UNEXPECTED,
            ));
        }
        println!(
            "Keyrill public Terminal probe v1; native_console=1; echo={}; line=1; code_page=65001; mode={}",
            u32::from(echo),
            actual.0
        );
        Ok(console)
    }

    fn restore(&mut self) -> windows_core::Result<()> {
        if self.restored {
            return Ok(());
        }
        // SAFETY: these original scalar settings belong to the retained borrowed console.
        let mode_result = unsafe { SetConsoleMode(self.input, self.mode) };
        // SAFETY: restore is attempted even when the independent mode restore failed.
        let page_result = unsafe { SetConsoleCP(self.code_page) };
        mode_result?;
        page_result?;
        let mut actual = CONSOLE_MODE::default();
        // SAFETY: actual is writable and input remains live until this child exits.
        unsafe { GetConsoleMode(self.input, &mut actual) }?;
        // SAFETY: this read-only query has no pointer arguments.
        if actual != self.mode || unsafe { GetConsoleCP() } != self.code_page {
            return Err(windows_core::Error::from_hresult(
                windows::Win32::Foundation::E_UNEXPECTED,
            ));
        }
        self.restored = true;
        Ok(())
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

struct ConsoleInput(HANDLE);

impl Read for ConsoleInput {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut count = 0;
        // SAFETY: the borrowed console handle and complete mutable buffer remain live.
        unsafe { ReadFile(self.0, Some(buffer), Some(&mut count), None) }
            .map_err(io::Error::other)?;
        Ok(count as usize)
    }
}

fn read_public_line(input: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 64];
        let count = input.read(&mut buffer)?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len() + count > LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "public fixture input exceeds bound",
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.contains(&b'\n') {
            return Ok(bytes);
        }
    }
}

fn run(arguments: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    let [mode, name] = arguments else {
        return Err("usage: KeyrillTerminalProbe console|no-echo|verify-pipe converted-en|converted-ru|original-en|original-ru|continuation".into());
    };
    let scenario = Scenario::parse(name).ok_or("unknown public Terminal scenario")?;
    if mode == "no-echo" && !matches!(scenario, Scenario::OriginalEn | Scenario::OriginalRu) {
        return Err("no-echo checks require an unchanged public original scenario".into());
    }
    let mut console = match mode.as_str() {
        "console" => Some(Console::prepare(true)?),
        "no-echo" => Some(Console::prepare(false)?),
        "verify-pipe" => None,
        _ => return Err("unknown public Terminal probe mode".into()),
    };
    println!("Use only the public fixture input, then press Enter.");
    let observed = match &console {
        Some(console) => read_public_line(&mut ConsoleInput(console.input)),
        None => read_public_line(&mut io::stdin().lock()),
    };
    if let Some(console) = &mut console {
        console.restore()?;
    }
    let observed = observed?;
    let matched = observed == scenario.expected();
    println!(
        "scenario={name}; matched={}; bytes={}; console_restored={}",
        u32::from(matched),
        observed.len(),
        u32::from(console.is_some())
    );
    Ok(matched)
}

/// Runs a maintainer-selected public console fixture or the explicit pipe-only oracle check.
/// No observed input is printed or saved. Each comparison includes exact UTF-8 and CRLF bytes.
pub fn terminal_probe(arguments: &[String]) -> ExitCode {
    match run(arguments) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn byte_oracle_requires_exact_utf8_boundary_and_line_ending() {
        for scenario in [
            Scenario::ConvertedEn,
            Scenario::ConvertedRu,
            Scenario::OriginalEn,
            Scenario::OriginalRu,
            Scenario::Continuation,
        ] {
            let expected = scenario.expected();
            assert_eq!(
                read_public_line(&mut Cursor::new(expected)).unwrap(),
                expected
            );
            for invalid in [
                &expected[..expected.len() - 1],
                b"ghbdtn \n".as_slice(),
                b"ghbdtn x\r\n",
            ] {
                assert_ne!(invalid, expected);
            }
        }
    }

    #[test]
    fn input_is_bounded_and_multiple_delivered_lines_are_visible() {
        assert!(read_public_line(&mut Cursor::new([b'x'; LIMIT + 1])).is_err());
        let extra = b"ghbdtn \r\nx";
        assert_eq!(read_public_line(&mut Cursor::new(extra)).unwrap(), extra);
        assert_ne!(extra.as_slice(), Scenario::OriginalEn.expected());
        assert!(Scenario::parse("custom-private-input").is_none());
    }

    #[test]
    fn excluded_mode_rejects_correction_expectations_before_native_setup() {
        for name in ["converted-en", "converted-ru", "continuation"] {
            let arguments = ["no-echo".to_owned(), name.to_owned()];
            assert!(run(&arguments).is_err());
        }
    }
}
