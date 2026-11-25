use crate::cargo::Result;
use std::collections::BTreeSet;

pub(crate) struct TestRun {
    prefix: String,
    selected: BTreeSet<String>,
    exact: Option<String>,
    filtered: usize,
}

impl TestRun {
    pub(crate) fn enumerate(prefix: &str, listing: &[u8], exact: Option<&str>) -> Result<Self> {
        let mut selected = BTreeSet::new();
        for line in std::str::from_utf8(listing)?.lines() {
            let name = line
                .strip_suffix(": test")
                .ok_or("test inventory contains an invalid record")?;
            if name.is_empty() || !selected.insert(name.to_owned()) {
                return Err("test inventory contains an empty or duplicate name".into());
            }
        }
        if selected.is_empty() {
            return Err("test inventory is empty".into());
        }
        let exact = exact
            .map(|name| -> Result<String> {
                let qualified_prefix = format!("{prefix}::");
                let name = name.strip_prefix(&qualified_prefix).unwrap_or(name);
                if !selected.contains(name) {
                    return Err("requested exact test is absent from the compiled inventory".into());
                }
                Ok(name.to_owned())
            })
            .transpose()?;
        let filtered = if let Some(name) = &exact {
            let filtered = selected.len() - 1;
            selected = BTreeSet::from([name.clone()]);
            filtered
        } else {
            0
        };
        Ok(Self {
            prefix: prefix.to_owned(),
            selected,
            exact,
            filtered,
        })
    }

    pub(crate) fn count(&self) -> usize {
        self.selected.len()
    }

    pub(crate) fn exact(&self) -> Option<&str> {
        self.exact.as_deref()
    }

    pub(crate) fn confirmed(self, output: &[u8]) -> Result<BTreeSet<String>> {
        let output = std::str::from_utf8(output)?;
        let mut passed = BTreeSet::new();
        let mut pending = None;
        for line in output.lines() {
            let result = if let Some((name, result)) = line
                .strip_prefix("test ")
                .and_then(|line| line.split_once(" ... "))
            {
                if pending.replace(name).is_some() || !self.selected.contains(name) {
                    return Err("test output contains an unexpected execution".into());
                }
                result
            } else {
                line
            };
            if result == "ok" {
                let name = pending.take().ok_or("test result has no active test")?;
                if !passed.insert(name.to_owned()) {
                    return Err("test output repeats a completed test".into());
                }
            }
        }
        let mut summaries = output
            .lines()
            .filter_map(|line| line.strip_prefix("test result: ok. "));
        let summary = summaries
            .next()
            .ok_or("successful test summary is missing")?;
        let expected = format!(
            "{} passed; 0 failed; 0 ignored; 0 measured; {} filtered out; finished in ",
            passed.len(),
            self.filtered
        );
        let duration = summary
            .strip_prefix(&expected)
            .and_then(|tail| tail.strip_suffix('s'))
            .ok_or("test summary is malformed")?
            .parse::<f64>()?;
        if !duration.is_finite()
            || duration < 0.0
            || summaries.next().is_some()
            || pending.is_some()
            || passed != self.selected
        {
            return Err("selected tests and confirmed successful executions differ".into());
        }
        Ok(passed
            .into_iter()
            .map(|name| format!("{}::{name}", self.prefix))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &[u8] = b"first: test\nnested::second: test\n";

    fn output(name: &str) -> Vec<u8> {
        format!(
            "running 1 test\ntest {name} ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n"
        )
        .into_bytes()
    }

    #[test]
    fn exact_selection_accepts_only_a_compiled_complete_name() {
        for name in ["nested::second", "controlled::nested::second"] {
            let selected = TestRun::enumerate("controlled", LISTING, Some(name)).unwrap();
            assert_eq!(selected.count(), 1);
            assert_eq!(selected.exact(), Some("nested::second"));
            assert_eq!(
                selected.confirmed(&output("nested::second")).unwrap(),
                BTreeSet::from(["controlled::nested::second".to_owned()])
            );
        }
        for name in ["", "second", "nested", "absent", "--exact"] {
            assert!(TestRun::enumerate("controlled", LISTING, Some(name)).is_err());
        }
    }

    #[test]
    fn empty_duplicate_and_invalid_inventories_are_rejected() {
        for listing in [
            b"".as_slice(),
            b"first: benchmark\n",
            b": test\n",
            b"first: test\nfirst: test\n",
            b"first: test\nsecond: tes\n",
            b"\xff",
        ] {
            assert!(TestRun::enumerate("controlled", listing, None).is_err());
        }
    }

    #[test]
    fn focused_success_cannot_claim_the_full_inventory() {
        let selected = TestRun::enumerate("controlled", LISTING, None).unwrap();
        assert_eq!(selected.count(), 2);
        assert!(selected.confirmed(&output("first")).is_err());
    }

    #[test]
    fn wrong_missing_ignored_or_duplicate_execution_is_rejected() {
        let incomplete = b"test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s\n";
        for output in [
            output("nested::second"),
            incomplete.to_vec(),
            b"test first ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 1 filtered out; finished in 0.00s\n".to_vec(),
            [b"test first ... ok\n".as_slice(), &output("first")].concat(),
            b"test first ... ok\n".to_vec(),
            b"test first ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;garbage\n".to_vec(),
            output("first").repeat(2),
            b"\xff".to_vec(),
        ] {
            let selected = TestRun::enumerate("controlled", LISTING, Some("first")).unwrap();
            assert!(selected.confirmed(&output).is_err());
        }
    }

    #[test]
    fn native_stdout_can_precede_a_separate_success_result() {
        let selected = TestRun::enumerate("controlled", LISTING, Some("first")).unwrap();
        let output = output("first");
        let output = String::from_utf8(output).unwrap().replace(
            " ... ok",
            " ... Native HRESULT: 0x00000000.\nNative readback succeeded.\nok",
        );
        assert_eq!(
            selected.confirmed(output.as_bytes()).unwrap(),
            BTreeSet::from(["controlled::first".to_owned()])
        );
    }

    #[test]
    fn complete_execution_qualifies_every_observed_success() {
        let selected = TestRun::enumerate("controlled", LISTING, None).unwrap();
        let output = b"test first ... ok\r\ntest nested::second ... ok\r\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\r\n";
        assert_eq!(
            selected.confirmed(output).unwrap(),
            BTreeSet::from([
                "controlled::first".to_owned(),
                "controlled::nested::second".to_owned()
            ])
        );
    }
}
