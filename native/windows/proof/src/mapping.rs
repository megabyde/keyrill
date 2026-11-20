//! Logon-scoped diagnostic mappings with a read-only seqlock reader.
use crate::native::{Handle, wide};
use crate::protocol::{
    Identity, PAGE_BYTES, PAYLOAD_WORDS, PROTOCOL, Payload, decode_payload, mapping_name,
};
use std::sync::atomic::{AtomicU32, Ordering, fence};
use windows::Win32::Foundation::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Threading::*;
use windows::core::{PCWSTR, PWSTR, Result};

const MAGIC: u32 = 0x4b525031;
#[repr(C)]
struct Page {
    header: [AtomicU32; 8],
    allocated: AtomicU32,
    claim: AtomicU32,
    sequence: AtomicU32,
    words: [AtomicU32; PAYLOAD_WORDS],
}
const _: () = assert!(std::mem::offset_of!(Page, words) == 44);
const _: () = assert!(std::mem::align_of::<Page>() == 4);
const _: () = assert!(std::mem::size_of::<Page>() <= PAGE_BYTES);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Availability of a coherent version-four mapping publication.
pub enum Availability {
    /// A complete valid publication is readable.
    Available,
    /// The requested native object or publication is absent.
    Missing,
    /// The native operation or publication remains in flight.
    Busy,
    /// The wire identity or schema failed validation.
    Incompatible,
    /// Captured lifetime, native focus, or provider facts changed.
    Stale,
}

fn expected_header(id: &Identity) -> [u32; 8] {
    [
        MAGIC,
        PROTOCOL,
        PAGE_BYTES as u32,
        PAYLOAD_WORDS as u32,
        id.process,
        id.started as u32,
        (id.started >> 32) as u32,
        id.thread,
    ]
}
fn matches(page: &Page, id: &Identity) -> bool {
    page.header
        .iter()
        .zip(expected_header(id))
        .all(|(actual, expected)| actual.load(Ordering::Acquire) == expected)
}
fn read(page: &Page, id: &Identity) -> (Availability, Payload) {
    for _ in 0..3 {
        if !matches(page, id) {
            return (Availability::Incompatible, [0; PAYLOAD_WORDS]);
        }
        let claim = page.claim.load(Ordering::Acquire);
        let before = page.sequence.load(Ordering::Acquire);
        fence(Ordering::SeqCst);
        if before >= u32::MAX - 2 {
            return (Availability::Incompatible, [0; PAYLOAD_WORDS]);
        }
        if claim == 0 || before == 0 {
            return (Availability::Missing, [0; PAYLOAD_WORDS]);
        }
        if before & 1 != 0 {
            continue;
        }
        let candidate = std::array::from_fn(|index| page.words[index].load(Ordering::Acquire));
        fence(Ordering::SeqCst);
        if before == page.sequence.load(Ordering::Acquire)
            && claim == page.claim.load(Ordering::Acquire)
            && matches(page, id)
            && candidate[0] == claim
        {
            return if decode_payload(&candidate).is_ok_and(|snapshot| {
                snapshot.kind == id.kind
                    && snapshot.version == id.version
                    && snapshot.architecture == id.architecture
            }) {
                (Availability::Available, candidate)
            } else {
                (Availability::Incompatible, [0; PAYLOAD_WORDS])
            };
        }
    }
    (Availability::Busy, [0; PAYLOAD_WORDS])
}
struct View(MEMORY_MAPPED_VIEW_ADDRESS);
impl View {
    fn map(handle: HANDLE, access: FILE_MAP) -> Result<Self> {
        let view = unsafe { MapViewOfFile(handle, access, 0, 0, PAGE_BYTES) };
        if view.Value.is_null() {
            return Err(windows::core::Error::from_hresult(E_FAIL));
        }
        Ok(Self(view))
    }
    fn page(&self) -> &Page {
        unsafe { &*self.0.Value.cast::<Page>() }
    }
}
impl Drop for View {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.0);
        }
    }
}
struct LocalAllocation(HLOCAL);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(self.0));
        }
    }
}

/// A newly created logon-secured mapping retained through a read-only view.
pub struct ArmedPage {
    view: View,
    _mapping: Handle,
    identity: Identity,
}
impl ArmedPage {
    /// Create a fresh logon-secured mapping and retain a read-only view.
    ///
    /// # Errors
    /// Returns the first failed native operation or evidence predicate.
    pub fn create(identity: Identity) -> Result<Self> {
        let mut token = HANDLE::default();
        unsafe {
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        }
        let token = Handle(token);
        let mut needed = 0;
        let _ = unsafe { GetTokenInformation(token.0, TokenLogonSid, None, 0, &mut needed) };
        if needed == 0 || needed > 65536 {
            return Err(windows::core::Error::from_hresult(E_FAIL));
        }
        let mut storage = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            GetTokenInformation(
                token.0,
                TokenLogonSid,
                Some(storage.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )?;
        }
        let groups = unsafe { &*storage.as_ptr().cast::<TOKEN_GROUPS>() };
        if groups.GroupCount != 1 {
            return Err(windows::core::Error::from_hresult(E_FAIL));
        }
        let mut sid = PWSTR::null();
        unsafe {
            ConvertSidToStringSidW(groups.Groups[0].Sid, &mut sid)?;
        }
        let sid_owner = LocalAllocation(HLOCAL(sid.0.cast()));
        let descriptor = wide(&format!("D:P(A;;GRGW;;;{})", unsafe { sid.to_string()? }));
        drop(sid_owner);
        let mut security = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(descriptor.as_ptr()),
                SDDL_REVISION_1,
                &mut security,
                None,
            )?;
        }
        let security_owner = LocalAllocation(HLOCAL(security.0));
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security.0,
            bInheritHandle: false.into(),
        };
        let name = wide(&mapping_name(&identity));
        let mapping = Handle(unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                Some(&attributes),
                PAGE_READWRITE,
                0,
                PAGE_BYTES as u32,
                PCWSTR(name.as_ptr()),
            )?
        });
        let error = unsafe { GetLastError() };
        drop(security_owner);
        if error == ERROR_ALREADY_EXISTS {
            return Err(windows::core::Error::from_hresult(
                HRESULT_FROM_WIN32_ALREADY_EXISTS,
            ));
        }
        let writer = View::map(mapping.0, FILE_MAP_WRITE)?;
        let page = writer.page();
        for (word, value) in page.header.iter().zip(expected_header(&identity)) {
            word.store(value, Ordering::SeqCst);
        }
        page.allocated.store(0, Ordering::SeqCst);
        page.claim.store(0, Ordering::SeqCst);
        page.sequence.store(0, Ordering::SeqCst);
        for word in &page.words {
            word.store(0, Ordering::SeqCst);
        }
        drop(writer);
        let read_only =
            Handle(unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr()))? });
        let view = View::map(read_only.0, FILE_MAP_READ)?;
        Ok(Self {
            view,
            _mapping: read_only,
            identity,
        })
    }
    /// Read a coherent publication while checking its captured mapping identity.
    pub fn read(&self) -> (Availability, Payload) {
        read(self.view.page(), &self.identity)
    }
}
const HRESULT_FROM_WIN32_ALREADY_EXISTS: windows::core::HRESULT =
    windows::core::HRESULT(0x800700b7u32 as i32);
/// Read an already existing mapping without modifying its contents.
pub fn existing(identity: &Identity) -> (Availability, Payload) {
    let name = wide(&mapping_name(identity));
    let Ok(handle) = (unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr())) })
    else {
        return (Availability::Missing, [0; PAYLOAD_WORDS]);
    };
    let mapping = Handle(handle);
    let Ok(view) = View::map(mapping.0, FILE_MAP_READ) else {
        return (Availability::Incompatible, [0; PAYLOAD_WORDS]);
    };
    read(view.page(), identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Publisher {
        view: Option<View>,
        mapping: Option<Handle>,
        instance: u32,
    }
    impl Publisher {
        fn attach(identity: &Identity) -> Self {
            let mut result = Self {
                view: None,
                mapping: None,
                instance: 0,
            };
            let name = wide(&mapping_name(identity));
            let Ok(mapping) = (unsafe {
                OpenFileMappingW(
                    (FILE_MAP_READ | FILE_MAP_WRITE).0,
                    false,
                    PCWSTR(name.as_ptr()),
                )
            }) else {
                return result;
            };
            let mapping = Handle(mapping);
            let Ok(view) = View::map(mapping.0, FILE_MAP_READ | FILE_MAP_WRITE) else {
                return result;
            };
            let page = view.page();
            if !matches(page, identity) {
                return result;
            }
            let allocated = page.allocated.load(Ordering::SeqCst);
            if allocated >= i32::MAX as u32
                || page
                    .allocated
                    .compare_exchange(allocated, allocated + 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
            {
                return result;
            }
            let instance = allocated + 1;
            if page
                .claim
                .compare_exchange(0, instance, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return result;
            }
            result.instance = instance;
            result.mapping = Some(mapping);
            result.view = Some(view);
            result
        }
        fn publish(&self, payload: &Payload) {
            if let Some(view) = &self.view {
                publish(view.page(), self.instance, payload);
            }
        }
    }
    fn publish(page: &Page, instance: u32, payload: &Payload) {
        if page.claim.load(Ordering::SeqCst) != instance {
            return;
        }
        let sequence = page.sequence.load(Ordering::SeqCst);
        if sequence & 1 != 0 || sequence >= u32::MAX - 2 {
            return;
        }
        page.sequence.store(sequence + 1, Ordering::SeqCst);
        for (word, value) in page.words.iter().zip(payload) {
            word.store(*value, Ordering::SeqCst);
        }
        page.sequence.store(sequence + 2, Ordering::SeqCst);
    }
    impl Drop for Publisher {
        fn drop(&mut self) {
            if let Some(view) = &self.view {
                let _ = view.page().claim.compare_exchange(
                    self.instance,
                    0,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
            }
        }
    }
    fn payload(identity: &Identity, instance: u32, serial: u32) -> Payload {
        let mut words = [0; 512];
        words[0] = instance;
        words[500] = identity.architecture;
        words[1] = identity.kind;
        words[2..6].copy_from_slice(&identity.version);
        words[15] = serial;
        words[24] = serial;
        words[25] = 1;
        words[165] = serial;
        words
    }
    fn identity() -> Identity {
        crate::native::capture_identity(
            unsafe { GetCurrentProcess() },
            unsafe { GetCurrentProcessId() },
            unsafe { GetCurrentThreadId() },
        )
        .unwrap()
    }
    #[test]
    fn host_probe_read_only_child() {
        let Ok(value) = std::env::var("KEYRILL_PROOF_MAPPING_CHILD") else {
            return;
        };
        let parts: Vec<_> = value.split(',').collect();
        assert_eq!(parts.len(), 4);
        let pid = parts[0].parse().unwrap();
        let expected_started = u64::from_str_radix(parts[1], 16).unwrap();
        let tid = parts[2].parse().unwrap();
        let process = Handle(
            unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
            }
            .unwrap(),
        );
        let id = crate::native::capture_identity(process.0, pid, tid).unwrap();
        assert_eq!(id.started, expected_started);
        assert_eq!(unsafe { WaitForSingleObject(process.0, 0) }, WAIT_TIMEOUT);
        let name = wide(&mapping_name(&id));
        let mapping = Handle(
            unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr())) }.unwrap(),
        );
        let view = View::map(mapping.0, FILE_MAP_READ).unwrap();
        let mut information = MEMORY_BASIC_INFORMATION::default();
        assert_eq!(
            unsafe {
                VirtualQuery(
                    Some(view.0.Value),
                    &mut information,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            },
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>()
        );
        assert_eq!(information.Protect, PAGE_READONLY);
        let (state, words) = read(view.page(), &id);
        assert_eq!(state, Availability::Available);
        let decoded = decode_payload(&words).unwrap();
        assert_eq!(
            (decoded.metadata[1], decoded.service[0], decoded.runtime[0]),
            (37, 37, 37)
        );
        let ready = usize::from_str_radix(parts[3], 16).unwrap();
        unsafe { SetEvent(HANDLE(ready as *mut _)) }.unwrap();
    }
    #[test]
    fn host_probe_main_mapping_claims_sequences_hostile_headers_and_independent_process() {
        let id = identity();
        assert_eq!(Publisher::attach(&id).instance, 0);
        let armed = ArmedPage::create(id).unwrap();
        assert!(ArmedPage::create(id).is_err());
        assert_eq!(armed.read().0, Availability::Missing);
        let first = Publisher::attach(&id);
        assert_ne!(first.instance, 0);
        assert_eq!(armed.read().0, Availability::Missing);
        let collision = Publisher::attach(&id);
        assert_eq!(collision.instance, 0);
        first.publish(&payload(&id, first.instance, 37));
        collision.publish(&payload(&id, 100, 99));
        assert_eq!(armed.read().0, Availability::Available);
        assert_eq!(decode_payload(&armed.read().1).unwrap().metadata[1], 37);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: true.into(),
        };
        let ready = Handle(unsafe { CreateEventW(Some(&attributes), true, false, None) }.unwrap());
        let child = crate::native::baseline_tests::Child::start(
            "mapping::tests::host_probe_read_only_child",
            &[(
                "KEYRILL_PROOF_MAPPING_CHILD",
                format!(
                    "{},{:x},{},{:x}",
                    id.process, id.started, id.thread, ready.0.0 as usize
                ),
            )],
            true,
        );
        assert_eq!(
            unsafe { WaitForSingleObject(ready.0, 10000) },
            WAIT_OBJECT_0
        );
        child.finish();
        let previous_instance = first.instance;
        drop(first);
        assert_eq!(armed.read().0, Availability::Missing);
        let second = Publisher::attach(&id);
        assert!(second.instance > previous_instance);
        second.publish(&payload(&id, second.instance, 38));
        assert_eq!(armed.read().0, Availability::Available);
        let page = second.view.as_ref().unwrap().page();
        page.sequence.fetch_add(1, Ordering::SeqCst);
        assert_eq!(armed.read().0, Availability::Busy);
        page.sequence.fetch_add(1, Ordering::SeqCst);
        for field in 0..8 {
            let old = page.header[field].fetch_add(1, Ordering::SeqCst);
            assert_eq!(armed.read().0, Availability::Incompatible);
            page.header[field].store(old, Ordering::SeqCst);
        }
        let mut malformed = payload(&id, second.instance, 38);
        malformed[511] = 1;
        second.publish(&malformed);
        assert_eq!(armed.read().0, Availability::Incompatible);
        malformed = payload(&id, second.instance, 38);
        malformed[14] = u32::MAX;
        second.publish(&malformed);
        assert_eq!(armed.read().0, Availability::Incompatible);
        let done = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for value in 1..2001 {
                    publish(page, second.instance, &payload(&id, second.instance, value));
                }
                done.store(true, Ordering::SeqCst);
            });
            while !done.load(Ordering::SeqCst) {
                let (state, words) = armed.read();
                if state == Availability::Available {
                    let decoded = decode_payload(&words).unwrap();
                    assert_eq!(decoded.metadata[1], decoded.service[0]);
                    assert_eq!(decoded.metadata[1], decoded.runtime[0]);
                }
            }
        });
        assert_eq!(armed.read().0, Availability::Available);
        page.sequence.store(u32::MAX - 1, Ordering::SeqCst);
        assert_eq!(armed.read().0, Availability::Incompatible);
        second.publish(&payload(&id, second.instance, 40));
        assert_eq!(armed.read().0, Availability::Incompatible);
        page.allocated.store(i32::MAX as u32, Ordering::SeqCst);
        drop(second);
        assert_eq!(Publisher::attach(&id).instance, 0);
        let truncated_id = Identity {
            thread: id.thread.wrapping_add(1),
            ..id
        };
        let name = wide(&mapping_name(&truncated_id));
        let _truncated = Handle(
            unsafe {
                CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    None,
                    PAGE_READWRITE,
                    0,
                    16,
                    PCWSTR(name.as_ptr()),
                )
            }
            .unwrap(),
        );
        assert_eq!(Publisher::attach(&truncated_id).instance, 0);
    }
    #[test]
    fn mapping_reader_checks_frozen_layout_and_unpublished_busy_stale_payloads() {
        let identity = Identity {
            kind: 3,
            architecture: 0x8664,
            process: 17,
            thread: 19,
            started: 23,
            ..Identity::default()
        };
        let page: Page = unsafe { std::mem::zeroed() };
        for (word, value) in page.header.iter().zip(expected_header(&identity)) {
            word.store(value, Ordering::SeqCst);
        }
        assert_eq!(read(&page, &identity).0, Availability::Missing);
        page.claim.store(1, Ordering::SeqCst);
        page.sequence.store(1, Ordering::SeqCst);
        assert_eq!(read(&page, &identity).0, Availability::Busy);
        page.sequence.store(2, Ordering::SeqCst);
        page.words[0].store(1, Ordering::SeqCst);
        page.words[1].store(identity.kind, Ordering::SeqCst);
        page.words[500].store(identity.architecture, Ordering::SeqCst);
        assert_eq!(read(&page, &identity).0, Availability::Available);
        for (index, value) in [(1, 2), (2, 1), (500, 0x014c)] {
            let old = page.words[index].swap(value, Ordering::SeqCst);
            assert_eq!(read(&page, &identity).0, Availability::Incompatible);
            page.words[index].store(old, Ordering::SeqCst);
        }
        for index in 0..8 {
            let old = page.header[index].fetch_add(1, Ordering::SeqCst);
            assert_eq!(read(&page, &identity).0, Availability::Incompatible);
            page.header[index].store(old, Ordering::SeqCst);
        }
        page.words[511].store(1, Ordering::SeqCst);
        assert_eq!(read(&page, &identity).0, Availability::Incompatible);
        page.words[511].store(0, Ordering::SeqCst);
        page.sequence.store(u32::MAX - 1, Ordering::SeqCst);
        assert_eq!(read(&page, &identity).0, Availability::Incompatible);
    }
    #[test]
    fn real_logon_secured_mapping_rejects_collision_and_has_read_only_view() {
        let identity = Identity {
            process: unsafe { GetCurrentProcessId() },
            thread: unsafe { GetCurrentThreadId() },
            started: crate::native::started(unsafe { GetCurrentProcess() }, false).unwrap(),
            ..Identity::default()
        };
        let armed = ArmedPage::create(identity).unwrap();
        assert_eq!(armed.read().0, Availability::Missing);
        assert!(ArmedPage::create(identity).is_err());
        assert_eq!(existing(&identity).0, Availability::Missing);
    }
}
