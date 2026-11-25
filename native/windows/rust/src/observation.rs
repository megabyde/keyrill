//! Read-only host metadata and protocol-four diagnostic publication.

use crate::diagnostics::*;
use crate::field_contract::HostIdentity;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, FILETIME, HANDLE};
use windows::Win32::Storage::Packaging::Appx::{GetPackageFamilyName, GetPackageId, PACKAGE_ID};
use windows::Win32::System::Memory::{
    FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW,
    UnmapViewOfFile,
};
use windows::Win32::System::SystemInformation::{
    IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_I386,
    IMAGE_FILE_MACHINE_UNKNOWN, PROCESSOR_ARCHITECTURE_AMD64,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, IsWow64Process2, PROCESS_NAME_WIN32, QueryFullProcessImageNameW,
};
use windows_core::{PCWSTR, PWSTR};

pub(crate) const PROTOCOL: u32 = 4;
pub(crate) const PAGE_BYTES: usize = 4096;
pub(crate) const PAYLOAD_WORDS: usize = 512;
const MAGIC: u32 = 0x4b525031;
pub(crate) type Payload = [u32; PAYLOAD_WORDS];

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Kind {
    #[default]
    Unknown,
    ProofEditor,
    PackagedNotepad,
    GenericHost,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Reason {
    #[default]
    Unobserved,
    Unsupported,
    Stale,
    Native,
    Context,
    ReadOnly,
    Disabled,
    Selection,
    MissingScope,
    Scope,
    #[expect(
        dead_code,
        reason = "Protocol 4 reserves this retired wire discriminant."
    )]
    MissingAcp,
    #[expect(
        dead_code,
        reason = "Protocol 4 reserves this retired wire discriminant."
    )]
    ProofOnly,
    Request,
    Session,
    Absent,
    Secure,
    Exception,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Focus {
    #[default]
    Unobserved,
    Exact,
    Different,
    Windowless,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Scope {
    #[default]
    Unobserved,
    Missing,
    Default,
    Text,
    Excluded,
    Ambiguous,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub(crate) reason: Reason,
    pub(crate) result: u32,
    pub(crate) focus: Focus,
    pub(crate) scope: Scope,
    pub(crate) request: u32,
    pub(crate) session: u32,
    pub(crate) entered: u32,
    pub(crate) completed: u32,
    pub(crate) selection: u32,
    pub(crate) acp: u32,
}

fn supported_process_architecture(machine: IMAGE_FILE_MACHINE, native: IMAGE_FILE_MACHINE) -> bool {
    native == IMAGE_FILE_MACHINE_AMD64
        && matches!(
            machine,
            IMAGE_FILE_MACHINE_UNKNOWN | IMAGE_FILE_MACHINE_I386
        )
}

pub(crate) fn capture_identity(process: HANDLE, pid: u32, thread: u32) -> Option<HostIdentity> {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let mut machine = IMAGE_FILE_MACHINE::default();
    let mut native = IMAGE_FILE_MACHINE::default();
    // SAFETY: the borrowed process handle and all output buffers stay live for these Win32 calls.
    unsafe {
        GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user).ok()?;
        IsWow64Process2(process, &mut machine, Some(&mut native)).ok()?;
    }
    if !supported_process_architecture(machine, native) {
        return None;
    }
    let mut identity = HostIdentity {
        kind: Kind::GenericHost as u32,
        process: pid,
        thread,
        started: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
        ..HostIdentity::default()
    };
    let mut path = [0u16; 32768];
    let mut length = path.len() as u32;
    // SAFETY: path has the advertised writable UTF-16 capacity and process is borrowed.
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
        .ok()?;
    }
    let path = path.get(..length as usize)?;
    let base = path.iter().rposition(|unit| *unit == b'\\' as u16)?;
    let basename = &path[base + 1..];
    if ascii_case_equal(basename, "KeyrillProofEditor.exe") {
        identity.kind = Kind::ProofEditor as u32;
        return Some(identity);
    }
    if machine != IMAGE_FILE_MACHINE_UNKNOWN || !ascii_case_equal(basename, "Notepad.exe") {
        return Some(identity);
    }
    let mut family = [0u16; 66];
    let mut family_size = family.len() as u32;
    // SAFETY: family is a writable UTF-16 buffer of family_size units.
    let family_result = unsafe {
        GetPackageFamilyName(process, &mut family_size, Some(PWSTR(family.as_mut_ptr())))
    };
    if family_result != ERROR_SUCCESS
        || !terminated(&family).is_some_and(|units| {
            units
                .iter()
                .copied()
                .eq("Microsoft.WindowsNotepad_8wekyb3d8bbwe".encode_utf16())
        })
    {
        return Some(identity);
    }
    let mut storage = [0u64; 512];
    let mut size = size_of_val(&storage) as u32;
    // SAFETY: storage is PACKAGE_ID-aligned and writable for the advertised byte size.
    if unsafe { GetPackageId(process, &mut size, Some(storage.as_mut_ptr().cast())) }
        != ERROR_SUCCESS
    {
        return Some(identity);
    }
    if (size as usize) < size_of::<PACKAGE_ID>() || size as usize > size_of_val(&storage) {
        return Some(identity);
    }
    // SAFETY: successful GetPackageId initialized a complete PACKAGE_ID in aligned storage.
    let package = unsafe { &*storage.as_ptr().cast::<PACKAGE_ID>() };
    if package.processorArchitecture != u32::from(PROCESSOR_ARCHITECTURE_AMD64.0) {
        return Some(identity);
    }
    let start = storage.as_ptr() as usize;
    let end = start + size as usize;
    let name = package.name.0 as usize;
    if name < start || name >= end || !name.is_multiple_of(align_of::<u16>()) {
        return Some(identity);
    }
    // SAFETY: package.name was checked inside the initialized package buffer, with UTF-16 alignment.
    let name = unsafe { std::slice::from_raw_parts(package.name.0, (end - name) / 2) };
    if !terminated(name).is_some_and(|units| {
        units
            .iter()
            .copied()
            .eq("Microsoft.WindowsNotepad".encode_utf16())
    }) {
        return Some(identity);
    }
    // SAFETY: PACKAGE_VERSION contains the SDK's four u16 version components.
    let version = unsafe { package.version.Anonymous.Anonymous };
    identity.version = [
        u32::from(version.Major),
        u32::from(version.Minor),
        u32::from(version.Build),
        u32::from(version.Revision),
    ];
    identity.kind = Kind::PackagedNotepad as u32;
    Some(identity)
}

fn terminated(units: &[u16]) -> Option<&[u16]> {
    Some(&units[..units.iter().position(|unit| *unit == 0)?])
}

fn ascii_case_equal(units: &[u16], text: &str) -> bool {
    units.len() == text.len()
        && units.iter().zip(text.bytes()).all(|(left, right)| {
            u8::try_from(*left).is_ok_and(|left| left.eq_ignore_ascii_case(&right))
        })
}

fn knowledge_word(knowledge: ModifierKnowledge) -> u32 {
    match knowledge {
        ModifierKnowledge::Unknown => 0,
        ModifierKnowledge::Known(facts) => {
            0x100
                | u32::from(facts.left_control)
                | (u32::from(facts.left_alt) << 1)
                | (u32::from(facts.right_control) << 2)
                | (u32::from(facts.right_alt) << 3)
                | (u32::from(facts.any_shift) << 4)
                | (u32::from(facts.any_windows) << 5)
        }
    }
}

fn observation_row(slot: Option<ObservationEvent>) -> [u32; 10] {
    let Some(event) = slot else {
        return [0; 10];
    };
    let mut row = [
        event.sequence.value,
        event.stamp.after.value,
        u32::from(event.stamp.owned.present),
        event.stamp.owned.composition_serial,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    let command = match event.body {
        ObservationBody::Command(command) => {
            row[4] = match command.stage {
                CommandStage::Arrival(_) => 1,
                CommandStage::AdmissionRefused(_) => 2,
                CommandStage::ExactModifiersRefused(_) => 3,
                CommandStage::DispatchRefused(_) => 4,
                CommandStage::ExecutorDispatch(_) => 5,
                CommandStage::Exception(_) => 6,
            };
            row[8] = knowledge_word(command.stage.knowledge());
            Some(command.command)
        }
        ObservationBody::Release(release) => {
            row[4] = 7;
            row[5] = u32::from(release.consumed);
            row[6] = release.context as u32;
            row[8] = release.modifier as u32 + 1;
            row[9] = release.delivery as u32 + 1;
            None
        }
        ObservationBody::Observer(observer) => {
            row[4] = 8;
            row[8] = observer.stage as u32 + 1;
            None
        }
        ObservationBody::Completion(completion) => {
            row[4] = 9;
            row[8] = u32::from(completion.witnessed);
            row[9] = completion.completion as u32 + 1;
            Some(completion.command)
        }
    };
    if let Some(command) = command {
        row[5] = command.route as u32 + 1;
        row[6] = command.intent as u32 + 1;
        row[7] = command.id.arrival.value;
    }
    row
}

pub(crate) fn encode(
    identity: &HostIdentity,
    metadata: &Metadata,
    service: &SnapshotV2,
    runtime: &RuntimeSnapshot,
    instance: u32,
    observations: &ObservationWindow,
) -> Payload {
    let mut words = [0; PAYLOAD_WORDS];
    let mut slots = words.iter_mut();
    let mut add = |word| {
        *slots.next().expect("protocol payload capacity") = word;
    };
    add(instance);
    add(identity.kind);
    for part in identity.version {
        add(part);
    }
    for part in crate::build_identity::BUILD_DIGEST {
        add(part);
    }
    for word in [
        metadata.reason as u32,
        metadata.result,
        metadata.focus as u32,
        metadata.scope as u32,
        metadata.request,
        metadata.session,
        metadata.entered,
        metadata.completed,
        metadata.selection,
        metadata.acp,
    ] {
        add(word);
    }
    let base = &service.base;
    for word in [
        base.activation,
        base.active,
        base.stopped,
        base.mode,
        base.saturated,
        base.request_present,
        base.request_result,
        base.session_present,
        base.session_result,
    ] {
        add(word);
    }
    for word in base.counts {
        add(word);
    }
    for word in base.refusals {
        add(word);
    }
    for refusal in [base.first, base.latest] {
        for word in [
            refusal.present,
            refusal.stage as u32,
            refusal.cause as u32,
            refusal.result,
        ] {
            add(word);
        }
    }
    let lifecycle = &service.lifecycle;
    for word in [
        lifecycle.sequence,
        lifecycle.allocated,
        lifecycle.saturated,
        lifecycle.count,
        lifecycle.owned_present,
        lifecycle.owned_serial,
    ] {
        add(word);
    }
    for event in lifecycle.events {
        for word in [
            event.sequence,
            event.kind as u32,
            event.subject,
            event.cause as u32,
            event.unresolved as u32,
        ] {
            add(word);
        }
    }
    for word in [
        runtime.activation,
        runtime.requested,
        runtime.effective,
        runtime.model_ready,
        runtime.revision as u32,
        (runtime.revision >> 32) as u32,
        runtime.error as u32,
        runtime.configuration_error as u32,
    ] {
        add(word);
    }
    for (status, error) in runtime.sources.into_iter().zip(runtime.source_errors) {
        add(status as u32);
        add(error as u32);
    }
    for word in [
        observations.sequence.value,
        u32::from(observations.saturated),
        observations.count,
    ] {
        add(word);
    }
    for event in observations.events {
        for word in observation_row(event) {
            add(word);
        }
    }
    words[500] = if cfg!(target_arch = "x86") {
        0x014c
    } else {
        0x8664
    };
    words
}

#[repr(C)]
struct Page {
    header: [AtomicU32; 8],
    allocated: AtomicU32,
    claim: AtomicU32,
    sequence: AtomicU32,
    words: [AtomicU32; PAYLOAD_WORDS],
}

impl Page {
    fn matches(&self, identity: &HostIdentity) -> bool {
        self.header
            .iter()
            .zip([
                MAGIC,
                PROTOCOL,
                PAGE_BYTES as u32,
                PAYLOAD_WORDS as u32,
                identity.process,
                identity.started as u32,
                (identity.started >> 32) as u32,
                identity.thread,
            ])
            .all(|(field, word)| field.load(Ordering::SeqCst) == word)
    }
}

fn name(identity: &HostIdentity) -> Vec<u16> {
    format!(
        "Local\\KeyrillProbe-v{PROTOCOL}-{}-{:016x}-{}",
        identity.process, identity.started, identity.thread
    )
    .encode_utf16()
    .chain(Some(0))
    .collect()
}

#[derive(Default)]
pub(crate) struct Publisher {
    mapping: HANDLE,
    page: Option<NonNull<Page>>,
    instance: u32,
}

impl Publisher {
    pub(crate) fn attach(&mut self, identity: &HostIdentity) {
        self.close();
        let name = name(identity);
        // SAFETY: name is nul-terminated. The opened mapping is owned until close.
        let Ok(mapping) = (unsafe {
            OpenFileMappingW(
                (FILE_MAP_READ | FILE_MAP_WRITE).0,
                false,
                PCWSTR(name.as_ptr()),
            )
        }) else {
            return;
        };
        self.mapping = mapping;
        // SAFETY: mapping is open for read/write and page_bytes bounds the mapped view.
        let view =
            unsafe { MapViewOfFile(mapping, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, PAGE_BYTES) };
        self.page = NonNull::new(view.Value.cast());
        let Some(pointer) = self.page else {
            self.close();
            return;
        };
        // SAFETY: the mapped view owns a complete, DWORD-aligned Page until close.
        let page = unsafe { pointer.as_ref() };
        if !page.matches(identity) {
            self.close();
            return;
        }
        let allocated = page.allocated.load(Ordering::SeqCst);
        if allocated >= i32::MAX as u32
            || page
                .allocated
                .compare_exchange(allocated, allocated + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            self.close();
            return;
        }
        let instance = allocated + 1;
        if page
            .claim
            .compare_exchange(0, instance, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            self.close();
            return;
        }
        self.instance = instance;
    }

    pub(crate) fn close(&mut self) {
        if let Some(pointer) = self.page.take() {
            // SAFETY: this view remains mapped until UnmapViewOfFile below.
            let page = unsafe { pointer.as_ref() };
            if self.instance != 0 {
                let _ = page.claim.compare_exchange(
                    self.instance,
                    0,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
            }
            // SAFETY: pointer is the base returned by MapViewOfFile and is unmapped once.
            let _ = unsafe {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: pointer.as_ptr().cast(),
                })
            };
        }
        let mapping = std::mem::take(&mut self.mapping);
        self.instance = 0;
        if !mapping.is_invalid() {
            // SAFETY: mapping is the owned OpenFileMappingW handle, closed once.
            let _ = unsafe { CloseHandle(mapping) };
        }
    }

    pub(crate) fn instance(&self) -> u32 {
        self.instance
    }

    pub(crate) fn publish(&self, words: &Payload) {
        let Some(pointer) = self.page else {
            return;
        };
        // SAFETY: the publisher keeps its view mapped for the entire shared borrow.
        let page = unsafe { pointer.as_ref() };
        if page.claim.load(Ordering::SeqCst) != self.instance {
            return;
        }
        let sequence = page.sequence.load(Ordering::SeqCst);
        if sequence & 1 != 0 || sequence >= u32::MAX - 2 {
            return;
        }
        page.sequence.store(sequence + 1, Ordering::SeqCst);
        for (field, word) in page.words.iter().zip(words) {
            field.store(*word, Ordering::SeqCst);
        }
        page.sequence.store(sequence + 2, Ordering::SeqCst);
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.close();
    }
}

const _: () = {
    assert!(align_of::<Page>() == 4);
    assert!(std::mem::offset_of!(Page, words) == 44);
    assert!(size_of::<Page>() == 2092);
    assert!(size_of::<Metadata>() == 40);
    assert!(14 + 10 + 9 + 12 + 26 + 8 + 6 + 5 * LIFECYCLE_CAPACITY + 12 == 177);
    assert!(177 + 3 + 10 * OBSERVATION_CAPACITY == 500);
    assert!(500 + 12 == PAYLOAD_WORDS);
};

#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
