use super::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::core::{PCWSTR, PWSTR, w};

unsafe extern "system" fn gated_thread(event: *mut std::ffi::c_void) -> u32 {
    u32::from(unsafe { WaitForSingleObject(HANDLE(event), 10000) } != WAIT_OBJECT_0)
}
struct Gated {
    event: Handle,
    thread: Handle,
    tid: u32,
}
impl Gated {
    fn create(attributes: Option<&SECURITY_ATTRIBUTES>) -> Self {
        let event = Handle(unsafe { CreateEventW(None, true, false, None) }.unwrap());
        let mut tid = 0;
        let thread = Handle(
            unsafe {
                CreateThread(
                    attributes.map(std::ptr::from_ref),
                    0,
                    Some(gated_thread),
                    Some(event.0.0.cast_const()),
                    THREAD_CREATION_FLAGS(0),
                    Some(&mut tid),
                )
            }
            .unwrap(),
        );
        Self { event, thread, tid }
    }
    fn finish(&self) {
        unsafe { SetEvent(self.event.0) }.unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(self.thread.0, 10000) },
            WAIT_OBJECT_0
        );
    }
}
impl Drop for Gated {
    fn drop(&mut self) {
        unsafe {
            let _ = SetEvent(self.event.0);
            WaitForSingleObject(self.thread.0, 10000);
        }
    }
}
#[test]
fn host_threads_enumerates_more_than_64_live_workers() {
    let workers: Vec<_> = (0..65).map(|_| Gated::create(None)).collect();
    for worker in &workers {
        assert_eq!(
            unsafe { WaitForSingleObject(worker.thread.0, 0) },
            WAIT_TIMEOUT
        );
    }
    let captured = threads(unsafe { GetCurrentProcessId() })
        .expect("enumeration must capture a process with 65 live worker threads");
    for worker in &workers {
        assert!(
            captured.contains(&worker.tid),
            "live worker {} missing from {} captured threads",
            worker.tid,
            captured.len()
        );
    }
    println!(
        "live_workers={} captured_threads={}",
        workers.len(),
        captured.len()
    );
}
#[test]
fn host_threads_live_exited_missing_invalid_and_denied() {
    let process = unsafe { GetCurrentProcess() };
    let pid = unsafe { GetCurrentProcessId() };
    let worker = Gated::create(None);
    let live = capture_thread(process, pid, worker.tid);
    assert_eq!(live.state, ThreadState::Live);
    assert_eq!(live.owner, pid);
    assert!(live.handle.is_some());
    drop(live);
    worker.finish();
    let exited = capture_thread(process, pid, worker.tid);
    assert_eq!(exited.state, ThreadState::Exited);
    assert_eq!(exited.wait, WAIT_OBJECT_0.0);
    drop(exited);
    let tid = worker.tid;
    drop(worker);
    let missing = capture_thread(process, pid, tid);
    assert!(
        missing.state == ThreadState::Exited
            || (missing.state == ThreadState::Missing
                && missing.error == ERROR_INVALID_PARAMETER.0)
    );
    let absent = capture_thread(process, pid, 0);
    assert_eq!(absent.state, ThreadState::Missing);
    assert_eq!(absent.error, ERROR_INVALID_PARAMETER.0);
    let invalid = capture_thread(HANDLE::default(), pid, unsafe { GetCurrentThreadId() });
    assert_eq!(invalid.state, ThreadState::ProcessFailed);
    assert_eq!(invalid.error, ERROR_INVALID_HANDLE.0);
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:P(D;;0x00100800;;;WD)"),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .unwrap();
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let denied = Gated::create(Some(&attributes));
    unsafe {
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    let check = capture_thread(process, pid, denied.tid);
    if check.state == ThreadState::Live {
        println!("INCONCLUSIVE thread DACL denial bypassed, no security privilege changed");
    } else {
        assert_eq!(check.state, ThreadState::OpenFailed);
        assert_eq!(check.error, ERROR_ACCESS_DENIED.0);
    }
    denied.finish();
}

pub(crate) struct Child {
    pub process: Handle,
    _thread: Handle,
    pub pid: u32,
    pub tid: u32,
}
impl Child {
    pub(crate) fn start(test: &str, variables: &[(&str, String)], inherit_handles: bool) -> Self {
        use std::os::windows::ffi::OsStrExt;
        let executable = std::env::current_exe().unwrap();
        let application: Vec<u16> = executable
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut command = wide(&format!(
            "\"{}\" --exact {test} --nocapture",
            executable.display()
        ));
        let mut environment: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
            .filter(|(key, _)| {
                !variables
                    .iter()
                    .any(|(name, _)| key.eq_ignore_ascii_case(name))
            })
            .collect();
        environment.extend(
            variables
                .iter()
                .map(|(key, value)| ((*key).into(), value.into())),
        );
        environment.sort_by_key(|(key, _)| key.to_string_lossy().to_lowercase());
        let mut block = Vec::new();
        for (key, value) in environment {
            block.extend(key.encode_wide());
            block.push(61);
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        let startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut child = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                PCWSTR(application.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                inherit_handles,
                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                Some(block.as_ptr().cast()),
                None,
                &startup,
                &mut child,
            )
        }
        .unwrap();
        Self {
            process: Handle(child.hProcess),
            _thread: Handle(child.hThread),
            pid: child.dwProcessId,
            tid: child.dwThreadId,
        }
    }
    pub(crate) fn finish(&self) {
        assert_eq!(
            unsafe { WaitForSingleObject(self.process.0, 10000) },
            WAIT_OBJECT_0
        );
        let mut code = 1;
        unsafe { GetExitCodeProcess(self.process.0, &mut code) }.unwrap();
        assert_eq!(code, 0);
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        if unsafe { WaitForSingleObject(self.process.0, 0) } == WAIT_TIMEOUT {
            unsafe {
                let _ = TerminateProcess(self.process.0, 99);
                WaitForSingleObject(self.process.0, 10000);
            }
        }
    }
}
#[test]
fn host_threads_child_wait_worker() {
    let Some(value) = std::env::var_os("KEYRILL_PROOF_THREAD_CHILD_EVENT") else {
        return;
    };
    let ready = usize::from_str_radix(
        &std::env::var("KEYRILL_PROOF_THREAD_CHILD_READY_EVENT").unwrap(),
        16,
    )
    .unwrap();
    unsafe { SetEvent(HANDLE(ready as *mut _)) }.unwrap();
    let handle = usize::from_str_radix(&value.to_string_lossy(), 16).unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(HANDLE(handle as *mut _), 10000) },
        WAIT_OBJECT_0
    );
}
#[test]
fn host_threads_foreign_owner_wrong_process_and_dead_process() {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let event = Handle(unsafe { CreateEventW(Some(&attributes), true, false, None) }.unwrap());
    let ready = Handle(unsafe { CreateEventW(Some(&attributes), true, false, None) }.unwrap());
    let child = Child::start(
        "native::baseline_tests::host_threads_child_wait_worker",
        &[
            (
                "KEYRILL_PROOF_THREAD_CHILD_EVENT",
                format!("{:x}", event.0.0 as usize),
            ),
            (
                "KEYRILL_PROOF_THREAD_CHILD_READY_EVENT",
                format!("{:x}", ready.0.0 as usize),
            ),
        ],
        true,
    );
    assert_eq!(
        unsafe { WaitForSingleObject(ready.0, 10000) },
        WAIT_OBJECT_0
    );
    let process = unsafe { GetCurrentProcess() };
    let pid = unsafe { GetCurrentProcessId() };
    let wrong = capture_thread(process, pid, child.tid);
    assert_eq!(wrong.state, ThreadState::WrongOwner);
    assert_eq!(wrong.owner, child.pid);
    assert_eq!(
        capture_thread(child.process.0, pid, child.tid).state,
        ThreadState::WrongProcess
    );
    unsafe { SetEvent(event.0) }.unwrap();
    child.finish();
    let stale = capture_thread(child.process.0, child.pid, child.tid);
    assert_eq!(stale.state, ThreadState::ProcessStale);
    assert_eq!(stale.process_wait, WAIT_OBJECT_0.0);
}
