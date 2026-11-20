//! CLI entry point for explicit-target native, UIA, and diagnostic inspection.
use crate::diagnostics::{CAUSES, COUNTERS, STAGES};
use crate::field::{self, Provider};
use crate::mapping::{self, ArmedPage, Availability};
use crate::native::{Target, ThreadState, capture_thread, threads};
use crate::options::{self, Command};
use crate::protocol::{Payload, ProbeSnapshot, complete_interval, decode_payload};
use crate::uia::{NativeProvider, describe};
use std::process::ExitCode;
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

fn digest_text(build: &[u32; 8]) -> String {
    build.iter().map(|word| format!("{word:08x}")).collect()
}
fn print_snapshot(thread: u32, state: Availability, words: &Payload, expected: &[u32; 8]) {
    print!("thread={thread} availability={state:?}");
    let Ok(value) = decode_payload(words) else {
        println!();
        return;
    };
    if state != Availability::Available {
        println!();
        return;
    }
    let m = &value.metadata;
    let s = &value.service;
    let l = &value.lifecycle;
    let r = &value.runtime;
    let reasons = [
        "Unobserved",
        "Unsupported",
        "Stale",
        "Native",
        "Context",
        "ReadOnly",
        "Disabled",
        "Selection",
        "MissingScope",
        "Scope",
        "MissingAcp",
        "ProofOnly",
        "Request",
        "Session",
        "Absent",
        "Secure",
        "Exception",
    ];
    println!(
        " instance={} kind={} build_match={} build={} reason={} result={:08x} focus={} scope={} request={:08x} session={:08x} entered={} completed={} selection={} acp={} activation={} active={} stopped={} mode={}",
        value.instance,
        value.kind,
        u32::from(value.build == *expected),
        digest_text(&value.build),
        reasons[m[0] as usize],
        m[1],
        m[2],
        m[3],
        m[4],
        m[5],
        m[6],
        m[7],
        m[8],
        m[9],
        s[0],
        s[1],
        s[2],
        s[3]
    );
    for (name, count) in COUNTERS.iter().zip(&s[9..21]) {
        print!(" {name}={count}");
    }
    println!(
        " owned={} serial={} lifecycle_sequence={} lifecycle_count={} requested_policy={} effective_policy={} model={} revision={} en_state={} en_error={} ru_state={} ru_error={}",
        l[4],
        l[5],
        l[0],
        l[3],
        r[1],
        r[2],
        r[3],
        u64::from(r[4]) | (u64::from(r[5]) << 32),
        r[8],
        r[9],
        r[10],
        r[11]
    );
    for event in l[6..6 + l[3] as usize * 5].as_chunks::<5>().0.iter() {
        println!(
            " lifecycle={} kind={} subject={} cause={} unresolved={}",
            event[0], event[1], event[2], event[3], event[4]
        );
    }
    let observations = &value.observations;
    println!(
        " observations_sequence={} count={} saturated={} lifecycle_saturated={} history_evicted={}",
        observations[0],
        observations[2],
        observations[1],
        l[2],
        u32::from(observations[0] > observations[2])
    );
    for event in observations[3..3 + observations[2] as usize * 10]
        .as_chunks::<10>()
        .0
        .iter()
    {
        print!(
            " observation={} lifecycle_after={} owned={} serial={}",
            event[0], event[1], event[2], event[3]
        );
        match event[4] {
            7 => println!(
                " release={} callback={} context={} eaten={}",
                [
                    "LeftControl",
                    "RightControl",
                    "LeftAlt",
                    "RightAlt",
                    "ControlUnresolved",
                    "AltUnresolved"
                ][(event[8] - 1) as usize],
                if event[9] == 1 { "TEST" } else { "ACTUAL" },
                ["Unchecked", "Matched", "Refused"][event[6] as usize],
                event[5]
            ),
            8 => println!(
                " alt_observer={}",
                ["Arrival", "Admitted", "Refused"][(event[8] - 1) as usize]
            ),
            9 => println!(
                " command={} witnessed={} completion={}",
                event[7],
                event[8],
                ["Refused", "Unknown", "Reserved", "ManualMark"][(event[9] - 1) as usize]
            ),
            _ => {
                print!(
                    " command={} route={} intent={} stage={} arrival_evicted={}",
                    event[7],
                    if event[5] == 1 {
                        "Ordinary"
                    } else {
                        "Preserved"
                    },
                    if event[6] == 1 { "Convert" } else { "Reverse" },
                    [
                        "Arrival",
                        "AdmissionRefused",
                        "ExactModifiersRefused",
                        "DispatchRefused",
                        "ExecutorDispatch",
                        "CallbackException"
                    ][(event[4] - 1) as usize],
                    u32::from(event[7] < observations[0] - observations[2] + 1)
                );
                if event[8] == 0 {
                    print!(" modifiers=Unknown");
                } else {
                    print!(
                        " modifiers=Known left_control={} left_alt={} right_control={} right_alt={} any_shift={} any_windows={}",
                        event[8] & 1,
                        (event[8] >> 1) & 1,
                        (event[8] >> 2) & 1,
                        (event[8] >> 3) & 1,
                        (event[8] >> 4) & 1,
                        (event[8] >> 5) & 1
                    );
                }
                if event[4] == 1 {
                    let continued = observations[3..3 + observations[2] as usize * 10]
                        .as_chunks::<10>()
                        .0
                        .iter()
                        .any(|next| next[0] > event[0] && next[4] <= 6 && next[7] == event[7]);
                    print!(" incomplete={}", u32::from(!continued));
                }
                println!();
            }
        }
    }
    for (stage, count) in STAGES.iter().zip(&s[21..47]) {
        if *count != 0 {
            println!(" refusal_count_stage={stage} count={count}");
        }
    }
    for (label, refusal) in ["first_refusal", "refusal"]
        .into_iter()
        .zip(s[47..].as_chunks::<4>().0.iter())
    {
        if refusal[0] != 0 {
            println!(
                " {label}_stage={} cause={} result={:08x}",
                STAGES[refusal[1] as usize], CAUSES[refusal[2] as usize], refusal[3]
            );
        }
    }
}
fn print_field(value: &field::Metadata) {
    let native = &value.native;
    println!(
        "field refusal={:?} pid={} foreground_tid={} process_started={:016x} foreground_thread_started={:016x} focused_thread_started={:016x} foreground={:x} parents={} parents_truncated={}",
        value.refusal,
        native.pid,
        native.foreground_tid,
        native.process_started,
        native.foreground_thread_started,
        native.focused_thread_started,
        native.foreground,
        native.parents.len(),
        u32::from(native.parents_truncated)
    );
    for (role, window) in [("focused", &native.focused), ("root", &native.root)]
        .into_iter()
        .chain(native.parents.iter().map(|window| ("parent", window)))
    {
        println!(
            "{role} hwnd={:x} pid={} tid={} control={} style={:016x} exstyle={:016x} enabled={} complete={} class_truncated={} class={}",
            window.handle,
            window.pid,
            window.tid,
            window.control,
            window.style,
            window.exstyle,
            u32::from(window.enabled),
            u32::from(window.complete),
            u32::from(window.class_truncated),
            String::from_utf16_lossy(&window.class_name)
        );
    }
    println!(
        "uia pid={} hwnd={:x} control_type={} exact_binding={} nonpassword={} enabled={} value_readonly={:?} text_readonly={:?} text_pattern={}",
        value.provider_pid,
        value.provider_window,
        value.control_type,
        u32::from(value.exact_binding),
        u32::from(value.nonpassword),
        u32::from(value.enabled),
        value.value_readonly,
        value.text_readonly,
        u32::from(value.text_pattern)
    );
}
/// Execute the original host-inspector commands with an explicit observer build digest.
pub fn inspect_host(arguments: &[String], observer_build: [u32; 8]) -> ExitCode {
    let Some(options) = options::parse(arguments) else {
        eprintln!(
            "Usage: KeyrillHostInspect build | host|field PID | public-case PID CASE | public-fixture PID CONTROL CASE | service PID [--arm-seconds 1..3600] [--expect-server-build SHA256]"
        );
        return ExitCode::from(2);
    };
    if options.command == Command::Build {
        println!("{}", digest_text(&observer_build));
        return ExitCode::SUCCESS;
    }
    let target = match Target::open(options.pid) {
        Ok(target) if target.identity.kind != 0 && target.current() => target,
        _ => {
            println!(
                "explicit target unavailable, incompatible, or outside diagnostic candidate set"
            );
            return ExitCode::FAILURE;
        }
    };
    let threads = match threads(options.pid) {
        Ok(threads) => threads,
        Err(error) => {
            eprintln!(
                "thread enumeration failed pid={} result={:08x}",
                options.pid,
                error.code().0 as u32
            );
            return ExitCode::FAILURE;
        }
    };
    let id = target.identity;
    if !matches!(
        options.command,
        Command::PublicCase(_) | Command::PublicFixture(_, _)
    ) {
        println!(
            "pid={} architecture={} started={:016x} kind={} version={}.{}.{}.{} threads={}",
            id.process,
            target.architecture.name(),
            id.started,
            id.kind,
            id.version[0],
            id.version[1],
            id.version[2],
            id.version[3],
            threads.len()
        );
    }
    match options.command {
        Command::Host => {
            for tid in &threads {
                let mut info = GUITHREADINFO {
                    cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                    ..Default::default()
                };
                if unsafe { GetGUIThreadInfo(*tid, &mut info) }.is_ok() {
                    let focused = describe(info.hwndFocus);
                    if focused.pid == id.process && focused.tid == *tid {
                        println!(
                            "thread={tid} focused_class={}",
                            String::from_utf16_lossy(&focused.class_name)
                        );
                    }
                }
            }
            return if threads.is_empty() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
        Command::Field | Command::PublicCase(_) | Command::PublicFixture(_, _) => {
            let Ok(_apartment) = crate::editor::Apartment::initialize() else {
                println!("field provider initialization unavailable before text read");
                return ExitCode::FAILURE;
            };
            let mut provider = match NativeProvider::open(&target) {
                Ok(provider) => provider,
                Err(refusal) => {
                    if let Command::PublicCase(case) = options.command {
                        println!(
                            "public-case case={} matched=0 refusal={refusal:?}",
                            case.name()
                        );
                    } else {
                        print_field(&field::Metadata {
                            refusal,
                            ..Default::default()
                        });
                    }
                    return ExitCode::FAILURE;
                }
            };
            let success = match options.command {
                Command::PublicFixture(fixture, case) => {
                    provider = provider.for_fixture(fixture);
                    let result = crate::public_fixture::check(&mut provider, fixture, case);
                    println!(
                        "public-fixture fixture={fixture:?} case={} matched={} refusal={:?}",
                        case.name(),
                        u32::from(result.is_ok()),
                        result.err().unwrap_or(field::Refusal::None)
                    );
                    result.is_ok()
                }
                Command::PublicCase(case) => {
                    let result = field::check_public_case(&mut provider, case);
                    println!(
                        "public-case case={} matched={} refusal={:?}",
                        case.name(),
                        u32::from(result.is_ok()),
                        result.err().unwrap_or(field::Refusal::None)
                    );
                    result.is_ok()
                }
                _ => {
                    let metadata = provider.metadata();
                    print_field(&metadata);
                    metadata.refusal == field::Refusal::None
                }
            };
            return if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
        _ => {}
    }
    let expected = options.expected_build.unwrap_or(observer_build);
    println!(
        "observer_native_build={} expected_service_build={} expected_explicit={}",
        digest_text(&observer_build),
        digest_text(&expected),
        u32::from(options.expected_build.is_some())
    );
    let mut live = Vec::new();
    for tid in threads {
        let check = capture_thread(target.process.0, id.process, tid);
        if check.state == ThreadState::Live {
            live.push(check);
        } else {
            println!(
                "pid={} started={:016x} thread={} capture={:?} owner={} error={} thread_wait={:08x} process_wait={:08x}",
                id.process,
                id.started,
                tid,
                check.state,
                check.owner,
                check.error,
                check.wait,
                check.process_wait
            );
            if !matches!(check.state, ThreadState::Exited | ThreadState::Missing) {
                return ExitCode::FAILURE;
            }
        }
    }
    if live.is_empty() {
        println!("no live target threads captured; no mapping armed");
        return ExitCode::FAILURE;
    }
    let mut armed = Vec::new();
    if options.seconds != 0 {
        for thread in &live {
            if !target.current() {
                return ExitCode::FAILURE;
            }
            match ArmedPage::create(crate::protocol::Identity {
                thread: thread.tid,
                ..id
            }) {
                Ok(page) => armed.push(page),
                Err(_) => {
                    println!("arming unavailable or collision; no existing mapping changed");
                    return ExitCode::FAILURE;
                }
            }
        }
        println!(
            "armed={}; activate source now; newly created threads require explicit rearming",
            armed.len()
        );
    }
    let mut previous: Vec<Option<ProbeSnapshot>> = vec![None; live.len()];
    let mut previous_words = vec![[0; 512]; live.len()];
    let mut previous_states = vec![Availability::Missing; live.len()];
    let mut available = false;
    for iteration in 0..=options.seconds {
        if !target.current() {
            println!("target stale; retained mappings are unavailable evidence");
            return ExitCode::FAILURE;
        }
        for (index, thread) in live.iter().enumerate() {
            let identity = crate::protocol::Identity {
                thread: thread.tid,
                ..id
            };
            let (state, words) =
                if thread.handle.as_ref().is_none_or(
                    |handle| unsafe { WaitForSingleObject(handle.0, 0) } != WAIT_TIMEOUT,
                ) {
                    (Availability::Stale, [0; 512])
                } else if options.seconds != 0 {
                    armed[index].read()
                } else {
                    mapping::existing(&identity)
                };
            let decoded = (state == Availability::Available)
                .then(|| decode_payload(&words).ok())
                .flatten();
            if iteration == 0 || words != previous_words[index] || state != previous_states[index] {
                print_snapshot(thread.tid, state, &words, &expected);
                if let (Some(before), Some(after)) = (&previous[index], &decoded) {
                    println!(
                        " thread={} complete_interval={}",
                        thread.tid,
                        u32::from(complete_interval(before, after))
                    );
                }
            }
            available |= decoded
                .as_ref()
                .is_some_and(|snapshot| snapshot.build == expected);
            previous[index] = decoded;
            previous_words[index] = words;
            previous_states[index] = state;
        }
        if iteration < options.seconds {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    if available {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
