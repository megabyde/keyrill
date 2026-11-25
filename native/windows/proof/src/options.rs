//! Strict argument parsing for the independent host inspector and editor.
use crate::field::PublicCase;
use crate::public_fixture::Fixture;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// The four established proof-editor startup modes.
pub enum FixtureMode {
    /// Ordinary four-field proof editor.
    Normal,
    /// Verify SES_CTFNOLOCK on field A and publish the denial setup row.
    DenyTsfLocks,
    /// Retain owned-mark evidence through native shutdown.
    ProveOwnedShutdown,
    /// Run the bounded native EN-to-RU owned-mark proof.
    ProveOwnedEnToRu,
}
impl FixtureMode {
    /// Parse an exact canonical command, case name, or status representation.
    pub fn parse(arguments: &[String]) -> Option<Self> {
        match arguments
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [] => Some(Self::Normal),
            ["--deny-tsf-locks"] => Some(Self::DenyTsfLocks),
            ["--prove-owned-shutdown"] => Some(Self::ProveOwnedShutdown),
            ["--prove-owned-en-to-ru"] => Some(Self::ProveOwnedEnToRu),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// An explicit host-inspector operation.
pub enum Command {
    /// Inspect the target build identity.
    Build,
    /// Inspect host publication evidence.
    Host,
    /// Inspect service publication evidence.
    Service,
    /// Inspect focused field evidence.
    Field,
    /// Check the document and caret against the named public case.
    PublicCase(PublicCase),
    /// Inspect an explicitly identified owned public fixture.
    PublicFixture(Fixture, PublicCase),
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// Validated host-inspector command arguments.
pub struct Options {
    /// Validated explicit inspector command.
    pub command: Command,
    /// Explicit target process identifier.
    pub pid: u32,
    /// Bounded arming duration, with zero reading existing mappings.
    pub seconds: u32,
    /// Optional reviewed server build digest.
    pub expected_build: Option<[u32; 8]>,
}

/// Parse the original nonzero bounded decimal PID or duration syntax.
pub fn number(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|&word| word != 0 && word != u32::MAX)
}

/// Parse exactly 64 hexadecimal digits into eight display-order DWORDs.
pub fn digest(value: &str) -> Option<[u32; 8]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut result = [0; 8];
    for (index, word) in result.iter_mut().enumerate() {
        *word = u32::from_str_radix(&value[index * 8..index * 8 + 8], 16).ok()?;
    }
    Some(result)
}

/// Parse an exact canonical command, case name, or status representation.
pub fn parse(arguments: &[String]) -> Option<Options> {
    let mut result = Options {
        command: Command::Build,
        pid: 0,
        seconds: 0,
        expected_build: None,
    };
    if arguments == ["build"] {
        return Some(result);
    }
    if !(2..=6).contains(&arguments.len()) {
        return None;
    }
    result.pid = number(&arguments[1])?;
    result.command = match arguments[0].as_str() {
        "host" => Command::Host,
        "service" => Command::Service,
        "field" => Command::Field,
        "public-fixture" if arguments.len() == 4 => Command::PublicFixture(
            Fixture::parse(&arguments[2])?,
            PublicCase::parse(&arguments[3])?,
        ),
        "public-case" if arguments.len() == 3 => {
            Command::PublicCase(PublicCase::parse(&arguments[2])?)
        }
        _ => return None,
    };
    if matches!(
        result.command,
        Command::PublicCase(_) | Command::PublicFixture(_, _)
    ) {
        return Some(result);
    }
    if result.command != Command::Service {
        return (arguments.len() == 2).then_some(result);
    }
    if !arguments.len().is_multiple_of(2) {
        return None;
    }
    for pair in arguments[2..].as_chunks::<2>().0.iter() {
        match pair[0].as_str() {
            "--arm-seconds" if result.seconds == 0 => {
                result.seconds = number(&pair[1])?;
                if result.seconds > 3600 {
                    return None;
                }
            }
            "--expect-server-build" if result.expected_build.is_none() => {
                result.expected_build = Some(digest(&pair[1])?)
            }
            _ => return None,
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }
    #[test]
    fn host_inspector_arguments() {
        assert_eq!(parse(&args(&["build"])).unwrap().command, Command::Build);
        for command in ["host", "service", "field"] {
            let value = parse(&args(&[command, "123"])).unwrap();
            assert_eq!(value.pid, 123);
            assert_eq!(value.seconds, 0);
            assert_eq!(value.expected_build, None);
        }
        for case in PublicCase::ALL {
            assert_eq!(
                parse(&args(&["public-case", "123", case.name()]))
                    .unwrap()
                    .command,
                Command::PublicCase(case)
            );
        }
        for name in ["", "anything", "ghbdtn ", "EMPTY", "empty "] {
            assert!(parse(&args(&["public-case", "123", name])).is_none());
        }
        for value in [
            "",
            "0",
            "-1",
            "+1",
            " 1",
            "4294967295",
            "4294967296",
            "999999999999999999999",
        ] {
            assert!(parse(&args(&["service", value])).is_none());
        }
        for value in ["0", "3601", "-1", "1x"] {
            assert!(parse(&args(&["service", "123", "--arm-seconds", value])).is_none());
        }
        let hash = "0123456789abcdefABCDEF019876543200000000ffffffff8000000000000001";
        let arm = parse(&args(&["service", "123", "--arm-seconds", "3600"])).unwrap();
        assert_eq!((arm.seconds, arm.expected_build), (3600, None));
        let build = parse(&args(&["service", "123", "--expect-server-build", hash])).unwrap();
        assert_eq!(build.seconds, 0);
        assert_eq!(build.expected_build, digest(hash));
        let options = parse(&args(&[
            "service",
            "123",
            "--arm-seconds",
            "3600",
            "--expect-server-build",
            hash,
        ]))
        .unwrap();
        assert_eq!(
            options.expected_build,
            Some([
                0x01234567,
                0x89abcdef,
                0xabcdef01,
                0x98765432,
                0,
                u32::MAX,
                0x80000000,
                1
            ])
        );
        assert!(
            parse(&args(&[
                "service",
                "123",
                "--expect-server-build",
                hash,
                "--arm-seconds",
                "1"
            ]))
            .is_some()
        );
        for invalid in [
            "",
            "0",
            &hash[..63],
            "0123456789abcdefABCDEF019876543200000000ffffffff80000000000000011",
            "g123456789abcdefABCDEF019876543200000000ffffffff8000000000000001",
            " 123456789abcdefABCDEF019876543200000000ffffffff8000000000000001",
        ] {
            assert!(digest(invalid).is_none());
        }
        for values in [
            vec!["public-case", "123"],
            vec!["public-empty", "123"],
            vec!["public-case", "123", "EMPTY"],
            vec!["public-case", "123", "empty", "--arm-seconds", "1"],
            vec!["host", "123", "--expect-server-build", hash],
            vec!["service", "123", "--expect-server-build"],
            vec![
                "service",
                "123",
                "--expect-server-build",
                hash,
                "--expect-server-build",
                hash,
            ],
            vec!["service", "123", "--arm-seconds", "1", "--arm-seconds", "2"],
            vec!["service", "123", "--ignore-build", hash],
        ] {
            assert!(parse(&args(&values)).is_none(), "{values:?}");
        }
        for command in ["host", "field", "public-case", "build"] {
            assert!(parse(&args(&[command, "123", "--expect-server-build", hash])).is_none());
        }
    }
    #[test]
    fn owned_public_fixture_arguments_are_explicit_and_exact() {
        for control in [
            "input", "textarea", "plain", "rich", "field-b", "vscode", "word",
        ] {
            assert!(matches!(
                parse(&args(&["public-fixture", "123", control, "empty"]))
                    .unwrap()
                    .command,
                Command::PublicFixture(_, PublicCase::Empty)
            ));
        }
        for values in [
            vec!["public-fixture", "123", "password", "empty"],
            vec!["public-fixture", "123", "readonly", "empty"],
            vec!["public-fixture", "123", "sensitive", "empty"],
            vec!["public-fixture", "123", "input", "Empty"],
            vec!["public-fixture", "0", "input", "empty"],
            vec!["public-fixture", "123", "input", "empty", "extra"],
        ] {
            assert!(parse(&args(&values)).is_none());
        }
    }
    #[test]
    fn editor_mode_arguments() {
        assert_eq!(FixtureMode::parse(&[]), Some(FixtureMode::Normal));
        for (flag, mode) in [
            ("--deny-tsf-locks", FixtureMode::DenyTsfLocks),
            ("--prove-owned-shutdown", FixtureMode::ProveOwnedShutdown),
            ("--prove-owned-en-to-ru", FixtureMode::ProveOwnedEnToRu),
        ] {
            assert_eq!(FixtureMode::parse(&args(&[flag])), Some(mode));
        }
        assert!(FixtureMode::parse(&args(&["--deny-tsf-locks", "extra"])).is_none());
    }
}
