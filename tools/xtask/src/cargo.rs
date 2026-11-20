use serde_json::Value;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
pub(crate) struct Artifact {
    pub(crate) name: String,
    pub(crate) executable: Option<PathBuf>,
    pub(crate) files: Vec<PathBuf>,
    pub(crate) test: bool,
}

fn decode(output: &[u8]) -> Result<Vec<Artifact>> {
    let mut artifacts = Vec::new();
    for line in std::str::from_utf8(output)?.lines() {
        let message: Value = serde_json::from_str(line)?;
        match message["reason"].as_str() {
            Some("compiler-artifact") => {
                let name = message["target"]["name"]
                    .as_str()
                    .ok_or("Cargo artifact target missing")?
                    .to_owned();
                let executable = message["executable"].as_str().map(PathBuf::from);
                let files = message["filenames"]
                    .as_array()
                    .ok_or("Cargo artifact filenames missing")?
                    .iter()
                    .map(|file| {
                        file.as_str()
                            .map(PathBuf::from)
                            .ok_or("Cargo artifact filename invalid")
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let test = message["profile"]["test"]
                    .as_bool()
                    .ok_or("Cargo artifact profile missing")?;
                artifacts.push(Artifact {
                    name,
                    executable,
                    files,
                    test,
                });
            }
            Some("compiler-message") => {
                if let Some(rendered) = message["message"]["rendered"].as_str() {
                    eprint!("{rendered}");
                }
            }
            Some("build-finished" | "build-script-executed") => {}
            _ => return Err("unknown Cargo JSON message".into()),
        }
    }
    Ok(artifacts)
}

pub(crate) fn artifacts(
    root: &Path,
    arguments: &[&str],
    environment: &[(&str, &str)],
) -> Result<Vec<Artifact>> {
    let output = Command::new("cargo")
        .current_dir(root)
        .args(arguments)
        .args(["--locked", "--offline", "--message-format=json"])
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .envs(environment.iter().copied())
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()?;
    let artifacts = decode(&output.stdout)?;
    if !output.status.success() {
        return Err("Cargo artifact build failed".into());
    }
    if artifacts.is_empty() {
        return Err("Cargo emitted no artifacts".into());
    }
    Ok(artifacts)
}

pub(crate) fn executable(artifacts: &[Artifact], name: &str, test: bool) -> Result<PathBuf> {
    let mut matches = artifacts
        .iter()
        .filter(|artifact| artifact.name == name && artifact.test == test)
        .filter_map(|artifact| artifact.executable.clone());
    let path = matches.next().ok_or("required Cargo executable missing")?;
    if matches.next().is_some() || !path.is_file() {
        return Err("required Cargo executable is ambiguous or absent".into());
    }
    Ok(path)
}

pub(crate) fn dll(artifacts: &[Artifact]) -> Result<PathBuf> {
    let mut matches = artifacts
        .iter()
        .filter(|artifact| artifact.name == "keyrill_windows" && !artifact.test)
        .flat_map(|artifact| &artifact.files)
        .filter(|path| path.extension().is_some_and(|extension| extension == "dll"));
    let path = matches.next().ok_or("required Cargo DLL missing")?.clone();
    if matches.next().is_some() || !path.is_file() {
        return Err("required Cargo DLL is ambiguous or absent".into());
    }
    Ok(path)
}

pub(crate) fn run(command: &mut Command) -> Result<()> {
    let status = command.stdin(Stdio::null()).status()?;
    if !status.success() {
        return Err(io::Error::other(format!("verification child exited with {status}")).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_json_distinguishes_test_executables_and_library_outputs() {
        let output = br#"{"reason":"compiler-artifact","target":{"name":"controlled"},"filenames":["control.exe"],"executable":"control.exe","profile":{"test":true}}
{"reason":"compiler-artifact","target":{"name":"keyrill_windows"},"filenames":["service.dll","service.rlib"],"executable":null,"profile":{"test":false}}
{"reason":"build-finished","success":true}"#;
        let artifacts = decode(output).unwrap();
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts[0].test);
        assert_eq!(artifacts[0].executable, Some(PathBuf::from("control.exe")));
        assert_eq!(
            artifacts[1].files,
            [PathBuf::from("service.dll"), PathBuf::from("service.rlib")]
        );
        assert!(!artifacts[1].test);
        assert!(artifacts[1].executable.is_none());
    }

    #[test]
    fn cargo_json_rejects_missing_metadata_and_unknown_lines() {
        for line in [
            b"plain text".as_slice(),
            br#"{"reason":"unknown"}"#,
            br#"{"reason":"compiler-artifact"}"#,
        ] {
            assert!(decode(line).is_err());
        }
    }

    #[test]
    #[ignore = "requires an explicit Cargo JSON artifact stream"]
    fn actual_cargo_stream_resolves_the_required_executable() {
        let path = std::env::var_os("KEYRILL_CARGO_JSON").expect("Cargo JSON stream required");
        let messages = std::fs::read(path).unwrap();
        let artifacts = decode(&messages).unwrap();
        let path = executable(&artifacts, "controlled", true).unwrap();
        assert!(path.is_absolute());
        assert_eq!(path.extension().unwrap(), "exe");
        assert!(executable(&artifacts, "absent", true).is_err());
        assert!(executable(&artifacts, "controlled", false).is_err());
        assert!(dll(&artifacts).is_err());
    }
}
