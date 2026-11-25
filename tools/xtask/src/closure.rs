use std::fs;
use std::io;
use std::path::Path;

fn inspect(directory: &Path) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::other("Windows sources must be self-contained"));
        }
        if kind.is_dir() {
            if entry.file_name() != "target" {
                inspect(&path)?;
            }
        } else if path
            .file_name()
            .is_some_and(|name| name == "CMakeLists.txt")
            || matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("c" | "cc" | "cpp" | "cxx" | "h" | "hpp" | "ps1" | "py")
            )
        {
            return Err(io::Error::other(
                "superseded Windows implementation source remains",
            ));
        }
    }
    Ok(())
}

pub(crate) fn check(root: &Path) -> io::Result<()> {
    inspect(&root.join("native/windows"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join("keyrill-windows-closure-tests");
            fs::create_dir_all(&base).unwrap();
            loop {
                let directory = base.join(format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&directory) {
                    Ok(()) => {
                        fs::create_dir_all(directory.join("native/windows/proof/src")).unwrap();
                        return Self(directory);
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("fixture directory failed: {error}"),
                }
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let base = std::env::temp_dir().join("keyrill-windows-closure-tests");
            assert_eq!(self.0.parent(), Some(base.as_path()));
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn rust_and_declarative_inputs_pass() {
        let fixture = Fixture::new();
        for file in [
            "proof/src/main.rs",
            "Cargo.toml",
            "proof_activation.manifest",
            "proof/scenarios.json",
            "resources.rc",
        ] {
            fs::write(fixture.0.join("native/windows").join(file), []).unwrap();
        }
        check(&fixture.0).unwrap();
    }

    #[test]
    fn every_legacy_executable_source_form_is_rejected() {
        let fixture = Fixture::new();
        for file in [
            "old.c",
            "old.cc",
            "old.cpp",
            "old.cxx",
            "old.h",
            "old.hpp",
            "build.ps1",
            "fingerprint.py",
            "CMakeLists.txt",
        ] {
            let path = fixture.0.join("native/windows/proof/src").join(file);
            fs::write(&path, []).unwrap();
            assert_eq!(
                check(&fixture.0).unwrap_err().to_string(),
                "superseded Windows implementation source remains"
            );
            fs::remove_file(path).unwrap();
        }
        check(&fixture.0).unwrap();
    }

    #[test]
    fn generated_target_outputs_are_outside_source_coverage() {
        let fixture = Fixture::new();
        let generated = fixture.0.join("native/windows/target");
        fs::create_dir_all(&generated).unwrap();
        fs::write(generated.join("external.cpp"), []).unwrap();
        check(&fixture.0).unwrap();
    }
}
