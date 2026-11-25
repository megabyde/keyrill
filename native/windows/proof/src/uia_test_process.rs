use std::ffi::OsStr;
use std::io::{self, Read};
use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, Stdio};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::WaitForSingleObject;

const CHILD_CASE: &str = "KEYRILL_PROOF_UIA_CHILD_CASE";
const OUTPUT_LIMIT: usize = 64 * 1024;

pub(crate) struct MtaApartment;

impl MtaApartment {
    pub(crate) fn initialize() -> Self {
        // SAFETY: This window-free test thread keeps the guard until its COM objects retire.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        Self
    }
}

impl Drop for MtaApartment {
    fn drop(&mut self) {
        // SAFETY: This guard balances successful initialization on the same test thread.
        unsafe { CoUninitialize() };
    }
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn capture(mut pipe: impl Read) -> io::Result<CapturedOutput> {
    let mut output = CapturedOutput {
        bytes: Vec::new(),
        truncated: false,
    };
    let mut buffer = [0; 4096];
    loop {
        let count = pipe.read(&mut buffer)?;
        if count == 0 {
            return Ok(output);
        }
        let retained = count.min(OUTPUT_LIMIT - output.bytes.len());
        output.bytes.extend_from_slice(&buffer[..retained]);
        output.truncated |= retained != count;
    }
}

fn completed_case(output: &str, case: &str) -> bool {
    let passed = format!("test {case} ... ok");
    let summaries: Vec<_> = output
        .lines()
        .filter(|line| line.starts_with("test result:"))
        .collect();
    output
        .lines()
        .filter(|line| *line == "running 1 test")
        .count()
        == 1
        && output.lines().filter(|line| *line == passed).count() == 1
        && summaries.len() == 1
        && summaries[0].starts_with("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;")
}

pub(crate) fn run(case: &str, body: impl FnOnce()) {
    if let Some(selected) = std::env::var_os(CHILD_CASE) {
        assert_eq!(selected, OsStr::new(case), "unexpected UIA child case");
        body();
        return;
    }
    std::thread::scope(|scope| {
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    case,
                    "--nocapture",
                    "--format=pretty",
                    "--color=never",
                ])
                .env(CHILD_CASE, case)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = child.0.stdout.take().unwrap();
        let stderr = child.0.stderr.take().unwrap();
        let stdout = scope.spawn(move || capture(stdout));
        let stderr = scope.spawn(move || capture(stderr));
        // SAFETY: Child owns the valid process handle throughout this synchronous wait.
        let waited = unsafe { WaitForSingleObject(HANDLE(child.0.as_raw_handle()), 30_000) };
        if waited != WAIT_OBJECT_0 {
            child.0.kill().unwrap();
        }
        let status = child.0.wait().unwrap();
        let stdout = stdout.join().unwrap().unwrap();
        let stderr = stderr.join().unwrap().unwrap();
        let output = String::from_utf8_lossy(&stdout.bytes);
        let errors = String::from_utf8_lossy(&stderr.bytes);
        assert!(
            waited == WAIT_OBJECT_0
                && status.success()
                && !stdout.truncated
                && !stderr.truncated
                && completed_case(&output, case),
            "UIA child {case} failed, wait={waited:?}, status={status}, stdout truncated={}, stderr truncated={}\n{output}\n{errors}",
            stdout.truncated,
            stderr.truncated,
        );
    });
}

#[test]
fn child_accounting_requires_the_exact_executed_case() {
    let success = "running 1 test\ntest selected ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 50 filtered out; finished in 0.01s\n";
    assert!(completed_case(success, "selected"));
    assert!(!completed_case(success, "wrong-filter"));
    assert!(!completed_case(
        "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.00s\n",
        "selected",
    ));
    assert!(!completed_case(
        &success.replace("0 ignored", "1 ignored"),
        "selected",
    ));
}

#[test]
fn child_output_drains_beyond_the_retained_limit() {
    let mut input = io::Cursor::new(vec![b'x'; OUTPUT_LIMIT * 3]);
    let output = capture(&mut input).unwrap();
    assert_eq!(output.bytes, vec![b'x'; OUTPUT_LIMIT]);
    assert!(output.truncated);
    assert_eq!(input.position(), (OUTPUT_LIMIT * 3) as u64);
}
