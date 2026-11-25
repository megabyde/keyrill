use super::*;
use crate::vocabulary_contract::{Error, Status};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, INVALID_HANDLE_VALUE};
use windows::Win32::System::Memory::{CreateFileMappingW, PAGE_READWRITE};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
};

fn identity() -> HostIdentity {
    static NEXT_THREAD: AtomicU32 = AtomicU32::new(0x8000_0000);
    // SAFETY: these functions return the current process pseudo-handle and scalar IDs.
    let mut identity = unsafe {
        capture_identity(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            GetCurrentThreadId(),
        )
    }
    .expect("current supported process identity");
    identity.thread = NEXT_THREAD.fetch_add(1, Ordering::SeqCst);
    identity
}

#[test]
fn process_architecture_accepts_x64_and_wow64_only_on_intel_amd_hosts() {
    for (machine, native, accepted) in [
        (0, 0x8664, true),
        (0x014c, 0x8664, true),
        (0, 0x014c, false),
        (0, 0xaa64, false),
        (0x014c, 0xaa64, false),
        (0x8664, 0xaa64, false),
        (0xaa64, 0x8664, false),
        (0xffff, 0x8664, false),
    ] {
        assert_eq!(
            supported_process_architecture(IMAGE_FILE_MACHINE(machine), IMAGE_FILE_MACHINE(native)),
            accepted
        );
    }
}

struct Mapping {
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl Mapping {
    fn create(identity: &HostIdentity) -> Self {
        let name = name(identity);
        // SAFETY: name is nul-terminated, and this creates a new page-file-backed diagnostic mapping.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                PAGE_BYTES as u32,
                PCWSTR(name.as_ptr()),
            )
        }
        .expect("create diagnostic mapping");
        // SAFETY: GetLastError is captured immediately after successful CreateFileMappingW.
        assert_ne!(unsafe { GetLastError() }, ERROR_ALREADY_EXISTS);
        // SAFETY: handle owns a writable mapping of PAGE_BYTES bytes.
        let view =
            unsafe { MapViewOfFile(handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, PAGE_BYTES) };
        assert!(!view.Value.is_null());
        let values = [
            MAGIC,
            PROTOCOL,
            PAGE_BYTES as u32,
            PAYLOAD_WORDS as u32,
            identity.process,
            identity.started as u32,
            (identity.started >> 32) as u32,
            identity.thread,
        ];
        let page = Page {
            header: values.map(AtomicU32::new),
            allocated: AtomicU32::new(0),
            claim: AtomicU32::new(0),
            sequence: AtomicU32::new(0),
            words: std::array::from_fn(|_| AtomicU32::new(0)),
        };
        // SAFETY: the fresh view is Page-aligned writable storage and no publisher has been attached.
        unsafe {
            view.Value.cast::<Page>().write(page);
        }
        Self { handle, view }
    }

    fn page(&self) -> &Page {
        // SAFETY: create initialized the complete Page and the mapping stays live through self.
        unsafe { &*self.view.Value.cast::<Page>() }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: these are the unique owned view and mapping handle returned by create.
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.handle);
        }
    }
}

fn sample(identity: &HostIdentity, instance: u32, journal: &ObservationJournal) -> Payload {
    let mut lifecycle = LifecycleJournal::default();
    let serial = lifecycle.acquire();
    lifecycle.record(
        LifecycleKind::EndRequested,
        serial,
        LifecycleCause::None,
        Unresolved::None,
    );
    let mut base = Snapshot {
        activation: 7,
        active: 1,
        mode: 2,
        request_present: 1,
        session_present: 1,
        ..Snapshot::default()
    };
    count(&mut base, Counter::Verified);
    refuse(
        &mut base,
        Stage::Request,
        Cause::Native,
        windows_core::HRESULT(0x8000_4005u32 as i32),
    );
    let service = SnapshotV2 {
        base,
        lifecycle: lifecycle.copy(true, serial),
    };
    let runtime = RuntimeSnapshot {
        activation: 7,
        requested: 2,
        effective: 2,
        model_ready: 1,
        revision: 0x1234_5678_9abc_def0,
        error: Error::Unavailable,
        configuration_error: Error::Configuration,
        sources: [Status::Ready, Status::Retained],
        source_errors: [Error::None, Error::Read],
    };
    let metadata = Metadata {
        reason: Reason::Session,
        focus: Focus::Exact,
        scope: Scope::Text,
        request: 0,
        session: 0,
        entered: 1,
        completed: 1,
        selection: 1,
        acp: 1,
        ..Metadata::default()
    };
    encode(
        identity,
        &metadata,
        &service,
        &runtime,
        instance,
        &journal.copy(),
    )
}

#[test]
fn current_process_identity_preserves_process_creation_time() {
    // SAFETY: these functions return the current process pseudo-handle and scalar IDs.
    let (process, pid, thread) = unsafe {
        (
            GetCurrentProcess(),
            GetCurrentProcessId(),
            GetCurrentThreadId(),
        )
    };
    let captured = capture_identity(process, pid, thread).expect("capture current process");
    assert_eq!(captured.process, pid);
    assert_eq!(captured.thread, thread);
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: process is the live current process and outputs are distinct initialized buffers.
    unsafe {
        GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user).unwrap();
    }
    assert_eq!(
        captured.started,
        (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)
    );
    assert!(capture_identity(HANDLE::default(), pid, thread).is_none());
}

#[test]
fn payload_offsets_and_reserved_padding_match_protocol_four() {
    let mut identity = identity();
    identity.kind = Kind::PackagedNotepad as u32;
    identity.version = [11, 2510, 6, 0];
    let words = sample(&identity, 3, &ObservationJournal::default());
    assert_eq!(&words[..6], &[3, 2, 11, 2510, 6, 0]);
    assert_eq!(&words[6..14], &crate::build_identity::BUILD_DIGEST);
    assert_eq!(&words[24..28], &[7, 1, 0, 2]);
    assert_eq!(
        &words[165..173],
        &[7, 2, 2, 1, 0x9abc_def0, 0x1234_5678, 2, 1]
    );
    assert_eq!(&words[173..177], &[2, 0, 3, 5]);
    assert!(words[177..500].iter().all(|word| *word == 0));
    assert_eq!(
        words[500],
        if cfg!(target_arch = "x86") {
            0x014c
        } else {
            0x8664
        }
    );
    assert!(words[501..].iter().all(|word| *word == 0));
}

#[test]
fn actual_named_mapping_preserves_claim_sequence_and_allocation() {
    let identity = identity();
    let mapping = Mapping::create(&identity);
    let page = mapping.page();
    let mut first = Publisher::default();
    let mut second = Publisher::default();
    first.attach(&identity);
    assert_eq!(first.instance(), 1);
    second.attach(&identity);
    assert_eq!(second.instance(), 0);
    assert_eq!(page.allocated.load(Ordering::SeqCst), 2);
    let words = sample(&identity, first.instance(), &ObservationJournal::default());
    first.publish(&words);
    assert_eq!(page.sequence.load(Ordering::SeqCst), 2);
    for (field, word) in page.words.iter().zip(words) {
        assert_eq!(field.load(Ordering::SeqCst), word);
    }
    first.close();
    assert_eq!(page.claim.load(Ordering::SeqCst), 0);
    second.attach(&identity);
    assert_eq!(second.instance(), 3);
    first.close();
    assert_eq!(page.claim.load(Ordering::SeqCst), 3);
    second.close();
    assert_eq!(page.claim.load(Ordering::SeqCst), 0);
}

#[test]
fn unavailable_mapping_invalid_header_and_exhaustion_refuse_publication() {
    let identity = identity();
    let mut publisher = Publisher::default();
    publisher.attach(&identity);
    assert_eq!(publisher.instance(), 0);
    let mapping = Mapping::create(&identity);
    let page = mapping.page();
    page.header[1].store(PROTOCOL + 1, Ordering::SeqCst);
    publisher.attach(&identity);
    assert_eq!(publisher.instance(), 0);
    assert_eq!(page.allocated.load(Ordering::SeqCst), 0);
    page.header[1].store(PROTOCOL, Ordering::SeqCst);
    page.allocated.store(i32::MAX as u32, Ordering::SeqCst);
    publisher.attach(&identity);
    assert_eq!(publisher.instance(), 0);
    page.allocated.store(0, Ordering::SeqCst);
    publisher.attach(&identity);
    let words = sample(
        &identity,
        publisher.instance(),
        &ObservationJournal::default(),
    );
    for sequence in [1, u32::MAX - 2, u32::MAX - 1, u32::MAX] {
        page.sequence.store(sequence, Ordering::SeqCst);
        publisher.publish(&words);
        assert_eq!(page.sequence.load(Ordering::SeqCst), sequence);
        assert!(
            page.words
                .iter()
                .all(|word| word.load(Ordering::SeqCst) == 0)
        );
    }
    page.sequence.store(0, Ordering::SeqCst);
    page.claim.store(2, Ordering::SeqCst);
    publisher.publish(&words);
    publisher.close();
    assert_eq!(page.claim.load(Ordering::SeqCst), 2);
    assert_eq!(page.sequence.load(Ordering::SeqCst), 0);
}

#[test]
fn observation_rows_preserve_all_discriminants() {
    let stamp = ObservationStamp::default();
    let facts = ModifierFacts {
        left_control: true,
        left_alt: true,
        ..ModifierFacts::default()
    };
    let mut journal = ObservationJournal::default();
    let ticket = journal.begin(
        RecognizedCommand::Ordinary {
            intent: ManualIntent::Reverse,
            modifiers: facts,
        },
        stamp,
    );
    journal.record(ticket, CommandStage::ExecutorDispatch(facts), stamp);
    journal.complete(
        CompletionObservation {
            command: ticket.identity.unwrap(),
            witnessed: true,
            completion: CommandCompletion::ManualMark,
        },
        stamp,
    );
    journal.release(
        ReleaseObservation {
            delivery: ReleaseDelivery::Actual,
            modifier: ReleaseModifier::LeftAlt,
            context: ReleaseContext::Matched,
            consumed: true,
        },
        stamp,
    );
    journal.observer(
        ObserverObservation {
            stage: ObserverStage::Admitted,
        },
        stamp,
    );
    let rows: Vec<_> = journal.copy().events[..5]
        .iter()
        .copied()
        .map(observation_row)
        .collect();
    assert_eq!(rows[0], [1, 0, 0, 0, 1, 1, 2, 1, 0x103, 0]);
    assert_eq!(rows[1], [2, 0, 0, 0, 5, 1, 2, 1, 0x103, 0]);
    assert_eq!(rows[2], [3, 0, 0, 0, 9, 1, 2, 1, 1, 4]);
    assert_eq!(rows[3], [4, 0, 0, 0, 7, 1, 1, 0, 3, 2]);
    assert_eq!(rows[4], [5, 0, 0, 0, 8, 0, 0, 0, 2, 0]);
}

#[test]
#[ignore = "Requires KEYRILL_DIAGNOSTIC_DECODER pointing to the independent SDK decoder helper."]
fn sdk_decoder_roundtrips_rust_payloads_through_logon_secured_mapping() {
    let decoder = std::env::var_os("KEYRILL_DIAGNOSTIC_DECODER").expect("SDK decoder path");
    let identity = identity();
    let mut child = Command::new(decoder)
        .args([
            identity.process.to_string(),
            identity.started.to_string(),
            identity.thread.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("launch SDK decoder");
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut ready = [0];
    output
        .read_exact(&mut ready)
        .expect("SDK logon-secured mapping readiness");
    assert_eq!(ready, [b'R']);
    let mut publisher = Publisher::default();
    publisher.attach(&identity);
    assert_eq!(publisher.instance(), 1);
    let facts = ModifierFacts {
        left_control: true,
        left_alt: true,
        ..ModifierFacts::default()
    };
    let unknown = ModifierKnowledge::Unknown;
    let preserved = RecognizedCommand::Preserved {
        intent: ManualIntent::Convert,
    };
    let ordinary = RecognizedCommand::Ordinary {
        intent: ManualIntent::Reverse,
        modifiers: facts,
    };
    let cases = [
        (preserved, CommandStage::AdmissionRefused(unknown)),
        (
            preserved,
            CommandStage::ExactModifiersRefused(ModifierFacts::default()),
        ),
        (preserved, CommandStage::DispatchRefused(facts)),
        (preserved, CommandStage::ExecutorDispatch(facts)),
        (
            preserved,
            CommandStage::Exception(ModifierKnowledge::Known(facts)),
        ),
        (ordinary, CommandStage::DispatchRefused(facts)),
        (ordinary, CommandStage::ExecutorDispatch(facts)),
        (
            ordinary,
            CommandStage::Exception(ModifierKnowledge::Known(facts)),
        ),
    ];
    for (recognized, stage) in cases {
        let stamp = ObservationStamp {
            after: LifecycleCursor { value: 2 },
            owned: ObservedOwnership {
                present: true,
                composition_serial: 1,
            },
        };
        let mut journal = ObservationJournal::default();
        let ticket = journal.begin(recognized, stamp);
        journal.record(ticket, stage, stamp);
        if matches!(stage, CommandStage::ExecutorDispatch(_)) {
            journal.complete(
                CompletionObservation {
                    command: ticket.identity.unwrap(),
                    witnessed: true,
                    completion: CommandCompletion::ManualMark,
                },
                stamp,
            );
            for completion in [
                CommandCompletion::Refused,
                CommandCompletion::Unknown,
                CommandCompletion::Reserved,
            ] {
                let ticket = journal.begin(recognized, stamp);
                journal.record(ticket, stage, stamp);
                journal.complete(
                    CompletionObservation {
                        command: ticket.identity.unwrap(),
                        witnessed: false,
                        completion,
                    },
                    stamp,
                );
            }
        }
        for delivery in [ReleaseDelivery::Test, ReleaseDelivery::Actual] {
            for modifier in [
                ReleaseModifier::LeftControl,
                ReleaseModifier::RightControl,
                ReleaseModifier::LeftAlt,
                ReleaseModifier::RightAlt,
                ReleaseModifier::ControlUnresolved,
                ReleaseModifier::AltUnresolved,
            ] {
                journal.release(
                    ReleaseObservation {
                        delivery,
                        modifier,
                        context: ReleaseContext::Unchecked,
                        consumed: false,
                    },
                    stamp,
                );
            }
        }
        journal.release(
            ReleaseObservation {
                delivery: ReleaseDelivery::Actual,
                modifier: ReleaseModifier::LeftAlt,
                context: ReleaseContext::Matched,
                consumed: true,
            },
            stamp,
        );
        journal.release(
            ReleaseObservation {
                delivery: ReleaseDelivery::Actual,
                modifier: ReleaseModifier::LeftAlt,
                context: ReleaseContext::Refused,
                consumed: false,
            },
            stamp,
        );
        for stage in [
            ObserverStage::Arrival,
            ObserverStage::Admitted,
            ObserverStage::Refused,
        ] {
            journal.observer(ObserverObservation { stage }, stamp);
        }
        let payload = sample(&identity, publisher.instance(), &journal);
        assert!(payload[501..].iter().all(|word| *word == 0));
        publisher.publish(&payload);
        input.write_all(b"R").unwrap();
        input.flush().unwrap();
        let mut roundtrip = [0; PAYLOAD_WORDS * 4];
        output
            .read_exact(&mut roundtrip)
            .expect("SDK decoder accepted Rust payload");
        let expected: Vec<_> = payload.into_iter().flat_map(u32::to_le_bytes).collect();
        assert_eq!(roundtrip.as_slice(), expected);
    }
    publisher.close();
    drop(input);
    assert!(child.wait().unwrap().success());
}
