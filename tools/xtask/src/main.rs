//! Rust commands for building and verifying the Windows port.

use keyrill_windows_build::{BuildConfig, Instrumentation, Target, identity, source_inputs};
use std::path::Path;

mod acceptance;
mod cargo;
mod cli;
mod closure;
mod coverage;
mod model;
mod package;
mod pe;
mod sanitize;
mod test_run;
mod windows;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root unavailable")?;
    match cli::parse(&arguments)? {
        cli::Command::PackageBuild(architectures) => {
            for &target in architectures.targets() {
                package::build(root, target)?;
            }
        }
        cli::Command::Package(directory) => package::stage(root, Path::new(&directory))?,
        cli::Command::PackageVerify(directory) => package::verify(Path::new(&directory))?,
        cli::Command::Build(architectures) => {
            for &target in architectures.targets() {
                let build = windows::build(root, target, Instrumentation::None)?;
                println!("{}", build.directory.display());
            }
        }
        cli::Command::Controlled {
            architectures,
            case,
        } => {
            for &target in architectures.targets() {
                let build = windows::build(root, target, Instrumentation::None)?;
                match &case {
                    Some(case) => windows::controlled_case(&build, case)?,
                    None => windows::controlled(&build)?,
                };
            }
        }
        cli::Command::Check(architectures) => {
            closure::check(root)?;
            acceptance::inventory(root)?;
            model::check(root)?;
            for &target in architectures.targets() {
                windows::common_checks(root, target)?;
                let mut build = windows::build(root, target, Instrumentation::None)?;
                let mut executed = windows::controlled(&build)?;
                executed.extend(windows::native_tests(root, &mut build)?);
                executed.extend(windows::smoke_tests(&mut build)?);
                coverage::check(root, &executed)?;
            }
        }
        cli::Command::Sanitize => {
            closure::check(root)?;
            acceptance::inventory(root)?;
            let mut build = windows::build(root, Target::X64, Instrumentation::Address)?;
            let mut executed = windows::controlled(&build)?;
            executed.extend(windows::native_tests(root, &mut build)?);
            executed.extend(windows::smoke_tests(&mut build)?);
            coverage::check(root, &executed)?;
        }
        cli::Command::ModelCheck => model::check(root)?,
        cli::Command::AcceptanceInventory => {
            let inventory = acceptance::inventory(root)?;
            println!(
                "Validated {} acceptance obligations.",
                inventory.requirements.len()
            );
            for (key, requirement) in inventory.requirements {
                println!("{key}\t{}\t{}", requirement.mode, requirement.expected);
            }
        }
        cli::Command::AcceptanceResults(path) => {
            acceptance::validate_results(root, &path)?;
            println!("Windows result-ledger consistency validated.");
        }
        cli::Command::Inventory => {
            for path in source_inputs(root)? {
                println!(
                    "{}",
                    path.strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/")
                );
            }
        }
        cli::Command::Fingerprint { target, variant } => {
            let config = BuildConfig {
                target: target.triple(),
                profile: "release",
                instrumentation: Instrumentation::None,
            };
            println!("{}", identity(root, config)?.variant(variant));
        }
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
