use crate::cargo::Result;
use keyrill_windows_build::{Target, Variant};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Architectures {
    One(Target),
    All,
}

impl Architectures {
    pub(crate) fn targets(self) -> &'static [Target] {
        match self {
            Self::One(Target::X64) => &[Target::X64],
            Self::One(Target::X86) => &[Target::X86],
            Self::All => &[Target::X64, Target::X86],
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    PackageBuild(Architectures),
    Package(String),
    PackageVerify(String),
    Build(Architectures),
    Controlled {
        architectures: Architectures,
        case: Option<String>,
    },
    Check(Architectures),
    Sanitize,
    ModelCheck,
    Inventory,
    AcceptanceInventory,
    AcceptanceResults(PathBuf),
    Fingerprint {
        target: Target,
        variant: Variant,
    },
}

pub(crate) fn parse(arguments: &[String]) -> Result<Command> {
    let [platform, command, options @ ..] = arguments else {
        return Err("usage: windows build|controlled|check [--arch x64|x86|all] | windows sanitize [--arch x64] | windows model-check|inventory | windows fingerprint production|test-api [--arch x64|x86] | windows acceptance inventory|results PATH".into());
    };
    if platform != "windows" {
        return Err("expected windows command".into());
    }
    if command == "package" || command == "package-verify" {
        let [flag, directory] = options else {
            return Err("package and package-verify require --candidate DIRECTORY".into());
        };
        if flag != "--candidate" || directory.is_empty() || directory.starts_with('-') {
            return Err("invalid candidate directory option".into());
        }
        return Ok(if command == "package" {
            Command::Package(directory.clone())
        } else {
            Command::PackageVerify(directory.clone())
        });
    }
    let mut architecture = None;
    let mut case = None;
    let mut positional = Vec::new();
    let mut options = options.iter();
    while let Some(option) = options.next() {
        match option.as_str() {
            "--arch" => {
                if architecture.is_some() {
                    return Err("duplicate --arch option".into());
                }
                let value = options.next().ok_or("--arch requires x64, x86 or all")?;
                architecture = Some(match value.as_str() {
                    "x64" => Architectures::One(Target::X64),
                    "x86" => Architectures::One(Target::X86),
                    "all" => Architectures::All,
                    _ => return Err("--arch supports x64, x86 or all; ARM64 is unsupported".into()),
                });
            }
            "--case" => {
                if case.is_some() {
                    return Err("duplicate --case option".into());
                }
                let value = options.next().ok_or("--case requires an exact test name")?;
                if value.is_empty() || value.starts_with("--") {
                    return Err("--case requires an exact test name".into());
                }
                case = Some(value.clone());
            }
            option if option.starts_with('-') => return Err("unknown command option".into()),
            _ => positional.push(option.as_str()),
        }
    }
    if case.is_some() && command != "controlled" {
        return Err("--case is supported by windows controlled".into());
    }
    if !matches!(command.as_str(), "fingerprint" | "acceptance") && !positional.is_empty() {
        return Err("unexpected command argument".into());
    }
    let architectures = architecture.unwrap_or(Architectures::One(Target::X64));
    match command.as_str() {
        "acceptance" => {
            if architecture.is_some() {
                return Err(
                    "acceptance validates the complete matrix and does not accept --arch".into(),
                );
            }
            match positional.as_slice() {
                ["inventory"] => Ok(Command::AcceptanceInventory),
                ["results", path] if !path.is_empty() => {
                    Ok(Command::AcceptanceResults(PathBuf::from(path)))
                }
                _ => Err("acceptance requires inventory or results PATH".into()),
            }
        }
        "package-build" => Ok(Command::PackageBuild(architectures)),
        "build" => Ok(Command::Build(architectures)),
        "controlled" => Ok(Command::Controlled {
            architectures,
            case,
        }),
        "check" => Ok(Command::Check(architectures)),
        "sanitize" => {
            if architectures != Architectures::One(Target::X64) {
                return Err("Windows AddressSanitizer supports x64 only; x86 sanitizer execution is unsupported".into());
            }
            Ok(Command::Sanitize)
        }
        "model-check" | "inventory" => {
            if architecture.is_some() {
                return Err("this command does not accept --arch".into());
            }
            Ok(if command == "model-check" {
                Command::ModelCheck
            } else {
                Command::Inventory
            })
        }
        "fingerprint" => {
            let Architectures::One(target) = architectures else {
                return Err("fingerprint requires one architecture, x64 or x86".into());
            };
            let variant = match positional.as_slice() {
                ["production"] => Variant::Production,
                ["test-api"] => Variant::TestApi,
                _ => return Err("fingerprint requires production or test-api".into()),
            };
            Ok(Command::Fingerprint { target, variant })
        }
        _ => Err("unknown windows command".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_words(value: &str) -> Result<Command> {
        parse(
            &value
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn defaults_and_architecture_selection_preserve_commands() {
        for (command, expected) in [
            (
                "windows build",
                Command::Build(Architectures::One(Target::X64)),
            ),
            (
                "windows build --arch x86",
                Command::Build(Architectures::One(Target::X86)),
            ),
            (
                "windows check --arch all",
                Command::Check(Architectures::All),
            ),
            ("windows sanitize --arch x64", Command::Sanitize),
            ("windows model-check", Command::ModelCheck),
            ("windows inventory", Command::Inventory),
        ] {
            assert_eq!(parse_words(command).unwrap(), expected);
        }
        for arguments in [
            "windows controlled --arch x86 --case boundary::exact",
            "windows controlled --case boundary::exact --arch x86",
        ] {
            assert_eq!(
                parse_words(arguments).unwrap(),
                Command::Controlled {
                    architectures: Architectures::One(Target::X86),
                    case: Some("boundary::exact".to_owned()),
                }
            );
        }
        for arguments in [
            "windows fingerprint production --arch x86",
            "windows fingerprint --arch x86 production",
        ] {
            assert_eq!(
                parse_words(arguments).unwrap(),
                Command::Fingerprint {
                    target: Target::X86,
                    variant: Variant::Production,
                }
            );
        }
    }

    #[test]
    fn rejects_unsupported_targets_duplicates_and_misplaced_options() {
        for arguments in [
            "windows build --arch arm64",
            "windows build --arch unknown",
            "windows build --arch x64 --arch x64",
            "windows controlled --case a --case a",
            "windows build --arch",
            "windows controlled --case",
            "windows controlled --case --arch x64",
            "windows build --case a",
            "windows model-check --arch x64",
            "windows inventory --arch all",
            "windows fingerprint production --arch all",
            "windows fingerprint production test-api",
            "windows build extra",
            "windows build --unknown",
            "windows sanitize --arch x86",
            "windows sanitize --arch all",
            "windows acceptance",
            "windows acceptance results",
            "windows acceptance unknown",
            "windows acceptance inventory extra",
            "windows acceptance inventory --arch x64",
            "windows acceptance inventory --case one",
        ] {
            assert!(parse_words(arguments).is_err(), "{arguments}");
        }
    }

    #[test]
    fn acceptance_commands_preserve_the_exact_local_result_path() {
        assert_eq!(
            parse_words("windows acceptance inventory").unwrap(),
            Command::AcceptanceInventory
        );
        let path = "C:\\Public Fixture\\results.json";
        let arguments = ["windows", "acceptance", "results", path].map(str::to_owned);
        assert_eq!(
            parse(&arguments).unwrap(),
            Command::AcceptanceResults(PathBuf::from(path))
        );
    }
}
