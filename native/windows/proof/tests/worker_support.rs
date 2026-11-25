#![allow(dead_code)]
use std::{
    ffi::{OsStr, OsString, c_void},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::*,
    System::{LibraryLoader::*, Threading::*},
    UI::WindowsAndMessaging::*,
};
use windows_core::{BOOL, HRESULT, PCSTR, PCWSTR};

pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if condition() {
            return;
        }
        assert!(Instant::now() < deadline, "bounded native progress");
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let _ = MsgWaitForMultipleObjectsEx(None, 10, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
}

pub struct Environment {
    name: OsString,
    previous: Option<OsString>,
}
impl Environment {
    pub fn absent(name: &str) -> Self {
        let previous = std::env::var_os(name);
        // Serial controlled tests own these process-global configuration variables.
        unsafe {
            std::env::remove_var(name);
        }
        Self {
            name: name.into(),
            previous,
        }
    }
    pub fn set(name: &str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(name);
        // Serial controlled tests own these process-global configuration variables.
        unsafe {
            std::env::set_var(name, value);
        }
        Self {
            name: name.into(),
            previous,
        }
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(&self.name, value),
                None => std::env::remove_var(&self.name),
            }
        }
    }
}

pub struct Files(pub PathBuf);
impl Files {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "keyrill-public-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("fresh public fixture directory");
        Self(path)
    }
    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    pub fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        std::fs::write(self.path(name), bytes).expect("write public fixture bytes");
    }
    pub fn source(&self, name: &str, default_source: bool) -> Source {
        Source {
            default_source,
            path: Some(wide(self.path(name))),
        }
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let resolved = self.0.canonicalize().expect("resolve fixture cleanup root");
        assert!(resolved.is_absolute());
        assert!(
            resolved
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("keyrill-public-")
        );
        use std::os::windows::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(&self.0).unwrap();
        assert!(metadata.is_dir() && metadata.file_attributes() & 0x400 == 0);
        std::fs::remove_dir_all(&self.0).expect("remove owned fixture tree");
    }
}

pub struct Module(pub HMODULE);
impl Module {
    pub fn load(path: &Path) -> Self {
        assert!(path.is_absolute());
        Self(unsafe {
            LoadLibraryW(PCWSTR(wide(path).as_ptr())).expect("load explicit worker DLL")
        })
    }
    pub fn test() -> Self {
        let module = Self::load(&PathBuf::from(
            std::env::var_os("KEYRILL_TEST_DLL").unwrap(),
        ));
        let digest: unsafe extern "system" fn(u32, *mut u32) -> HRESULT =
            unsafe { module.export(b"KeyrillTestBuildDigest\0") };
        let mut actual = [0; 8];
        assert_eq!(unsafe { digest(8, actual.as_mut_ptr()) }, S_OK);
        let expected = std::env::var("KEYRILL_TEST_BUILD_DIGEST")
            .unwrap()
            .split(',')
            .map(|word| u32::from_str_radix(word.trim_start_matches("0x"), 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual.as_slice(), expected);
        module
    }
    pub unsafe fn export<T: Copy>(&self, name: &[u8]) -> T {
        let pointer = unsafe { GetProcAddress(self.0, PCSTR(name.as_ptr())) }
            .expect("required worker export");
        assert_eq!(size_of::<T>(), size_of_val(&pointer));
        unsafe { std::mem::transmute_copy(&pointer) }
    }
}
impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0).expect("release explicit worker DLL reference");
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NativeCalls {
    pub open: *mut c_void,
    pub read: *mut c_void,
    pub completion: *mut c_void,
    pub cancel: *mut c_void,
    pub event: *mut c_void,
    pub pin: *mut c_void,
    pub submit: *mut c_void,
    pub long_running: *mut c_void,
    pub volume_paths: *mut c_void,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Hooks {
    pub call: *mut c_void,
    pub context: *mut c_void,
    pub native: NativeCalls,
    pub retain: *mut c_void,
    pub release: *mut c_void,
}
pub const BEFORE_WORK: u32 = 0;
pub const PARENT_OPENED: u32 = 1;
pub const BEFORE_READ: u32 = 2;
pub const AFTER_READ: u32 = 3;
pub const BEFORE_PARSE: u32 = 4;
pub const BEFORE_PUBLISH: u32 = 5;
pub const PUBLISHED: u32 = 6;
pub const CLEANUP: u32 = 7;
pub const AFTER_COUNT: u32 = 8;
#[repr(C)]
pub struct RawSource {
    pub default_source: u32,
    pub path: *const u16,
    pub length: u32,
}
pub struct Source {
    pub default_source: bool,
    pub path: Option<Vec<u16>>,
}
impl Source {
    pub fn absent() -> Self {
        Self {
            default_source: true,
            path: None,
        }
    }
    fn raw(&self) -> RawSource {
        RawSource {
            default_source: self.default_source.into(),
            path: self.path.as_ref().map_or(null(), |p| p.as_ptr()),
            length: self.path.as_ref().map_or(0, |p| (p.len() - 1) as u32),
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceReady {
    pub status: u32,
    pub error: u32,
    pub dictionary: *const c_void,
    pub owner: *const c_void,
}
#[repr(C)]
#[derive(Debug, Default)]
pub struct RawReady {
    pub activation: u32,
    pub model: u32,
    pub revision: u64,
    pub error: u32,
    pub reserved: u32,
    pub sources: [SourceReady; 2],
}
pub struct Packet<'a> {
    pub raw: RawReady,
    api: &'a WorkerApi,
}
impl std::ops::Deref for Packet<'_> {
    type Target = RawReady;
    fn deref(&self) -> &RawReady {
        &self.raw
    }
}
impl Drop for Packet<'_> {
    fn drop(&mut self) {
        for source in self.raw.sources {
            unsafe { (self.api.release)(source.owner) };
        }
    }
}
pub struct WorkerApi {
    pub module: Module,
    pub create: unsafe extern "system" fn() -> *mut c_void,
    pub start:
        unsafe extern "system" fn(*mut c_void, *const RawSource, u32, u32, *const Hooks) -> BOOL,
    pub stop: unsafe extern "system" fn(*mut c_void),
    pub poll: unsafe extern "system" fn(*mut c_void, *mut RawReady) -> BOOL,
    pub free: unsafe extern "system" fn(*mut c_void),
    pub release: unsafe extern "system" fn(*const c_void),
    pub set: unsafe extern "system" fn(*const Hooks) -> BOOL,
}
impl WorkerApi {
    pub fn load() -> Self {
        let module = Module::test();
        unsafe {
            Self {
                create: module.export(b"KeyrillTestWorkerCreate\0"),
                start: module.export(b"KeyrillTestWorkerStart\0"),
                stop: module.export(b"KeyrillTestWorkerStop\0"),
                poll: module.export(b"KeyrillTestWorkerPoll\0"),
                free: module.export(b"KeyrillTestWorkerFree\0"),
                release: module.export(b"KeyrillTestWorkerDictionaryRelease\0"),
                set: module.export(b"KeyrillTestWorkerHooks\0"),
                module,
            }
        }
    }
    pub fn worker(&self) -> Worker<'_> {
        let pointer = unsafe { (self.create)() };
        assert!(!pointer.is_null());
        Worker { api: self, pointer }
    }
}
pub struct Worker<'a> {
    pub api: &'a WorkerApi,
    pub pointer: *mut c_void,
}
impl<'a> Worker<'a> {
    pub fn start(&self, sources: &[Source; 2], activation: u32, dll: bool, hooks: Hooks) -> bool {
        let sources = sources.each_ref().map(Source::raw);
        unsafe {
            (self.api.start)(
                self.pointer,
                sources.as_ptr(),
                activation,
                dll.into(),
                &hooks,
            )
            .as_bool()
        }
    }
    pub fn stop(&self) {
        unsafe { (self.api.stop)(self.pointer) }
    }
    pub fn poll(&self) -> Option<Packet<'a>> {
        let mut raw = RawReady::default();
        unsafe { (self.api.poll)(self.pointer, &mut raw).as_bool() }
            .then_some(Packet { raw, api: self.api })
    }
}
impl Drop for Worker<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.free)(self.pointer) }
    }
}

pub struct Event(pub HANDLE);
unsafe impl Send for Event {}
unsafe impl Sync for Event {}
impl Event {
    pub fn new() -> Self {
        Self(unsafe { CreateEventW(None, true, false, None).unwrap() })
    }
    pub fn signal(&self) {
        unsafe { SetEvent(self.0).unwrap() }
    }
    pub fn reset(&self) {
        unsafe { ResetEvent(self.0).unwrap() }
    }
    pub fn wait(&self) {
        assert_eq!(unsafe { WaitForSingleObject(self.0, 15000) }, WAIT_OBJECT_0)
    }
    pub fn signaled(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub type Action = Arc<dyn Fn(u32, HANDLE, usize) + Send + Sync>;
pub struct Events {
    pub published: Event,
    pub resume: Event,
    pub finished: Event,
    pub retained: AtomicUsize,
    pub released: AtomicUsize,
    pub failed: AtomicBool,
    pub after_count: AtomicBool,
    pub pause: AtomicU32,
    pub action: Mutex<Option<Action>>,
}
impl Events {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            published: Event::new(),
            resume: Event::new(),
            finished: Event::new(),
            retained: AtomicUsize::new(0),
            released: AtomicUsize::new(0),
            failed: AtomicBool::new(false),
            after_count: AtomicBool::new(false),
            pause: AtomicU32::new(PUBLISHED),
            action: Mutex::new(None),
        })
    }
    pub fn hooks(self: &Arc<Self>) -> Hooks {
        Hooks {
            call: hook as *mut c_void,
            context: Arc::as_ptr(self).cast_mut().cast(),
            retain: retain as *mut c_void,
            release: release as *mut c_void,
            ..Hooks::default()
        }
    }
    pub fn next<'a>(&self, worker: &Worker<'a>) -> Packet<'a> {
        self.published.wait();
        self.published.reset();
        assert!(
            !self.failed.load(Ordering::Acquire),
            "worker callback failed"
        );
        let packet = worker.poll().expect("published whole packet");
        self.resume.signal();
        packet
    }
    pub fn stop(&self, worker: &Worker<'_>) {
        worker.stop();
        self.resume.signal();
        self.finished.wait();
        assert!(worker.poll().is_none());
        self.check();
    }
    pub fn check(&self) {
        assert!(!self.failed.load(Ordering::Acquire));
        assert_eq!(
            self.retained.load(Ordering::Acquire),
            self.released.load(Ordering::Acquire)
        );
    }
}
unsafe extern "system" fn retain(context: *mut c_void) {
    let events = unsafe { &*context.cast::<Events>() };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        unsafe { Arc::increment_strong_count(context.cast::<Events>()) };
        events.retained.fetch_add(1, Ordering::AcqRel);
    }));
    if result.is_err() {
        events.failed.store(true, Ordering::Release);
    }
}
unsafe extern "system" fn release(context: *mut c_void) {
    let events = unsafe { Arc::from_raw(context.cast::<Events>()) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        events.released.fetch_add(1, Ordering::AcqRel);
        if events.after_count.load(Ordering::Acquire) {
            events.finished.signal();
        }
    }));
    if result.is_err() {
        events.failed.store(true, Ordering::Release);
    }
}
unsafe extern "system" fn hook(context: *mut c_void, boundary: u32, handle: HANDLE, index: usize) {
    let events = unsafe { &*context.cast::<Events>() };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let action = events.action.lock().unwrap().clone();
        if let Some(action) = action {
            action(boundary, handle, index)
        }
        if boundary == AFTER_COUNT {
            events.after_count.store(true, Ordering::Release);
        }
        if boundary == events.pause.load(Ordering::Acquire) {
            events.published.signal();
            events.resume.wait();
            events.resume.reset();
        }
    }));
    if result.is_err() {
        events.failed.store(true, Ordering::Release);
        events.published.signal();
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub activation: u32,
    pub requested: u32,
    pub effective: u32,
    pub model_ready: u32,
    pub revision: u64,
    pub error: u32,
    pub configuration_error: u32,
    pub sources: [u32; 2],
    pub source_errors: [u32; 2],
}
#[windows_core::interface("cda9ed25-1dbc-4fb6-b2a5-bc132945e082")]
pub unsafe trait RuntimeReader: windows_core::IUnknown {
    fn copy(&self, size: u32, version: u32, output: *mut RuntimeSnapshot) -> HRESULT;
}
pub fn runtime(
    service: &windows::Win32::UI::TextServices::ITfTextInputProcessorEx,
) -> RuntimeSnapshot {
    use windows_core::Interface;
    let reader = service.cast::<RuntimeReader>().unwrap();
    let mut value = RuntimeSnapshot::default();
    assert_eq!(unsafe { reader.copy(48, 1, &mut value) }, S_OK);
    value
}
const _: () = {
    assert!(size_of::<Hooks>() == 13 * size_of::<usize>());
    assert!(size_of::<RawReady>() == 40 + 4 * size_of::<usize>());
    assert!(size_of::<RuntimeSnapshot>() == 48);
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Io {
    Real,
    Immediate,
    Pending,
    Drain,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    None,
    Event,
    Pin,
    Submit,
    LongRunning,
    Root,
    Component,
    Verification,
}
pub struct Native {
    pub events: Arc<Events>,
    pub io: Io,
    pub failure: Failure,
    pub fail_event_at: usize,
    pub remap_after_failure: bool,
    pub entered: Event,
    pub cancelled: Event,
    pub draining: Event,
    pub resume: Event,
    pub opens: AtomicUsize,
    pub reads: AtomicUsize,
    pub completions: AtomicUsize,
    pub event_calls: AtomicUsize,
    pub verification: AtomicBool,
    pub remap: AtomicBool,
    pub completion_returned: AtomicBool,
    pub file: AtomicUsize,
    pub operation: AtomicUsize,
    pub buffer: AtomicUsize,
}
static CURRENT: std::sync::atomic::AtomicPtr<Native> =
    std::sync::atomic::AtomicPtr::new(null_mut());
impl Native {
    pub fn new(
        io: Io,
        failure: Failure,
        fail_event_at: usize,
        remap_after_failure: bool,
    ) -> Box<Self> {
        let value = Box::new(Self {
            events: Events::new(),
            io,
            failure,
            fail_event_at,
            remap_after_failure,
            entered: Event::new(),
            cancelled: Event::new(),
            draining: Event::new(),
            resume: Event::new(),
            opens: AtomicUsize::new(0),
            reads: AtomicUsize::new(0),
            completions: AtomicUsize::new(0),
            event_calls: AtomicUsize::new(0),
            verification: AtomicBool::new(false),
            remap: AtomicBool::new(false),
            completion_returned: AtomicBool::new(false),
            file: AtomicUsize::new(0),
            operation: AtomicUsize::new(0),
            buffer: AtomicUsize::new(0),
        });
        assert!(
            CURRENT
                .swap((&*value as *const Self).cast_mut(), Ordering::AcqRel)
                .is_null()
        );
        value
    }
    pub fn hooks(&self) -> Hooks {
        let mut hooks = self.events.hooks();
        hooks.native = NativeCalls {
            open: native_open as *mut c_void,
            volume_paths: native_volume as *mut c_void,
            event: native_event as *mut c_void,
            pin: native_pin as *mut c_void,
            submit: native_submit as *mut c_void,
            long_running: native_long_running as *mut c_void,
            ..Default::default()
        };
        if self.io != Io::Real {
            hooks.native.read = native_read as *mut c_void;
            hooks.native.completion = native_completion as *mut c_void;
            hooks.native.cancel = native_cancel as *mut c_void;
        }
        let address = self as *const Self as usize;
        *self.events.action.lock().unwrap() = Some(Arc::new(move |at, _, index| {
            let state = unsafe { &*(address as *const Native) };
            if at == PARENT_OPENED && index == 0 {
                state.verification.store(false, Ordering::Release);
            }
            if at == AFTER_READ {
                state.verification.store(true, Ordering::Release);
            }
        }));
        hooks
    }
    pub fn stop(&self, worker: &Worker<'_>) {
        self.events.stop(worker)
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        assert_eq!(
            CURRENT.swap(null_mut(), Ordering::AcqRel),
            self as *mut Self
        );
    }
}
fn native() -> &'static Native {
    let pointer = CURRENT.load(Ordering::Acquire);
    assert!(!pointer.is_null());
    unsafe { &*pointer }
}
fn native_fence<T: Copy>(failure: T, call: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)) {
        Ok(value) => value,
        Err(_) => {
            native().events.failed.store(true, Ordering::Release);
            failure
        }
    }
}
use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Win32::System::IO::{IO_STATUS_BLOCK, OVERLAPPED};
windows_core::link!("ntdll.dll" "system" fn NtCreateFile(output:*mut HANDLE,access:u32,object:*const OBJECT_ATTRIBUTES,status:*mut IO_STATUS_BLOCK,size:*const i64,attributes:u32,sharing:u32,disposition:u32,options:u32,ea:*const c_void,ea_length:u32)->NTSTATUS);
windows_core::link!("kernel32.dll" "system" fn GetVolumePathNamesForVolumeNameW(guid:PCWSTR,paths:*mut u16,capacity:u32,length:*mut u32)->BOOL);
mod raw {
    use super::*;
    windows_core::link!("kernel32.dll" "system" fn CreateEventW(security:*const c_void,manual:BOOL,initial:BOOL,name:PCWSTR)->HANDLE);
    windows_core::link!("kernel32.dll" "system" fn GetModuleHandleExW(flags:u32,address:PCWSTR,module:*mut HMODULE)->BOOL);
    windows_core::link!("kernel32.dll" "system" fn TrySubmitThreadpoolCallback(callback:PTP_SIMPLE_CALLBACK,context:*mut c_void,environment:*const c_void)->BOOL);
    windows_core::link!("kernel32.dll" "system" fn CallbackMayRunLong(instance:PTP_CALLBACK_INSTANCE)->BOOL);
}
unsafe extern "system" fn native_open(
    output: *mut HANDLE,
    access: u32,
    object: *const OBJECT_ATTRIBUTES,
    status: *mut IO_STATUS_BLOCK,
    size: *const i64,
    attributes: u32,
    sharing: u32,
    disposition: u32,
    options: u32,
    ea: *const c_void,
    ea_length: u32,
) -> NTSTATUS {
    native_fence(NTSTATUS(0xc0000001u32 as i32), || {
        let s = native();
        s.opens.fetch_add(1, Ordering::AcqRel);
        let root = unsafe { (*object).RootDirectory.is_invalid() };
        let verification = s.verification.load(Ordering::Acquire);
        if (s.failure == Failure::Root && root)
            || (s.failure == Failure::Component && !root && !verification)
            || (s.failure == Failure::Verification && verification)
        {
            if s.remap_after_failure {
                s.remap.store(true, Ordering::Release);
            }
            unsafe { output.write(HANDLE::default()) };
            return NTSTATUS(0xc0000034u32 as i32);
        }
        unsafe {
            NtCreateFile(
                output,
                access,
                object,
                status,
                size,
                attributes,
                sharing,
                disposition,
                options,
                ea,
                ea_length,
            )
        }
    })
}
unsafe extern "system" fn native_volume(
    guid: PCWSTR,
    paths: *mut u16,
    capacity: u32,
    length: *mut u32,
) -> BOOL {
    native_fence(BOOL(0), || {
        if native().remap.load(Ordering::Acquire) {
            unsafe {
                length.write(1);
                if capacity > 0 {
                    paths.write(0);
                }
            }
            BOOL(1)
        } else {
            unsafe { GetVolumePathNamesForVolumeNameW(guid, paths, capacity, length) }
        }
    })
}
unsafe extern "system" fn native_event(
    security: *const c_void,
    manual: BOOL,
    initial: BOOL,
    name: PCWSTR,
) -> HANDLE {
    native_fence(HANDLE::default(), || {
        let s = native();
        let count = s.event_calls.fetch_add(1, Ordering::AcqRel) + 1;
        if s.failure == Failure::Event && count == s.fail_event_at {
            HANDLE::default()
        } else {
            unsafe { raw::CreateEventW(security, manual, initial, name) }
        }
    })
}
unsafe extern "system" fn native_pin(flags: u32, address: PCWSTR, module: *mut HMODULE) -> BOOL {
    native_fence(BOOL(0), || {
        if native().failure == Failure::Pin {
            unsafe { module.write(HMODULE::default()) };
            BOOL(0)
        } else {
            unsafe { raw::GetModuleHandleExW(flags, address, module) }
        }
    })
}
unsafe extern "system" fn native_submit(
    callback: PTP_SIMPLE_CALLBACK,
    context: *mut c_void,
    environment: *const c_void,
) -> BOOL {
    native_fence(BOOL(0), || {
        if native().failure == Failure::Submit {
            BOOL(0)
        } else {
            unsafe { raw::TrySubmitThreadpoolCallback(callback, context, environment) }
        }
    })
}
unsafe extern "system" fn native_long_running(instance: PTP_CALLBACK_INSTANCE) -> BOOL {
    native_fence(BOOL(0), || {
        if native().failure == Failure::LongRunning {
            BOOL(0)
        } else {
            unsafe { raw::CallbackMayRunLong(instance) }
        }
    })
}
unsafe extern "system" fn native_read(
    file: HANDLE,
    buffer: *mut u8,
    bytes: u32,
    count: *mut u32,
    operation: *mut OVERLAPPED,
) -> BOOL {
    native_fence(BOOL(0), || {
        let s = native();
        s.reads.fetch_add(1, Ordering::AcqRel);
        assert!(count.is_null() && !operation.is_null() && bytes > 0);
        s.file.store(file.0 as usize, Ordering::Release);
        s.operation.store(operation as usize, Ordering::Release);
        s.buffer.store(buffer as usize, Ordering::Release);
        let offset = unsafe { (*operation).Anonymous.Anonymous.Offset };
        if offset == 0 && s.io != Io::Drain {
            assert!(bytes >= 6);
            unsafe { std::ptr::copy_nonoverlapping(b"hello\n".as_ptr(), buffer, 6) }
        }
        s.entered.signal();
        if s.io == Io::Immediate {
            return BOOL(1);
        }
        if s.io == Io::Pending {
            unsafe { SetEvent((*operation).hEvent).unwrap() }
        }
        unsafe { SetLastError(ERROR_IO_PENDING) };
        BOOL(0)
    })
}
unsafe extern "system" fn native_cancel(file: HANDLE, operation: *mut OVERLAPPED) -> BOOL {
    native_fence(BOOL(0), || {
        let s = native();
        assert_eq!(file.0 as usize, s.file.load(Ordering::Acquire));
        assert_eq!(operation as usize, s.operation.load(Ordering::Acquire));
        s.cancelled.signal();
        BOOL(1)
    })
}
unsafe extern "system" fn native_completion(
    file: HANDLE,
    operation: *mut OVERLAPPED,
    count: *mut u32,
    wait: BOOL,
) -> BOOL {
    native_fence(BOOL(0), || {
        let s = native();
        s.completions.fetch_add(1, Ordering::AcqRel);
        assert_eq!(file.0 as usize, s.file.load(Ordering::Acquire));
        assert_eq!(operation as usize, s.operation.load(Ordering::Acquire));
        if s.io == Io::Drain {
            assert!(wait.as_bool() && s.cancelled.signaled());
            s.draining.signal();
            s.resume.wait();
            let mut flags = 0;
            unsafe {
                GetHandleInformation(file, &mut flags).unwrap();
                GetHandleInformation((*operation).hEvent, &mut flags).unwrap();
                (s.buffer.load(Ordering::Acquire) as *mut u8).write(0);
                count.write(0);
                SetLastError(ERROR_OPERATION_ABORTED)
            };
            s.completion_returned.store(true, Ordering::Release);
            return BOOL(0);
        }
        assert!(!wait.as_bool());
        unsafe {
            count.write(if (*operation).Anonymous.Anonymous.Offset == 0 {
                6
            } else {
                0
            })
        };
        BOOL(1)
    })
}

impl RuntimeReader {
    pub unsafe fn read(&self, size: u32, version: u32, output: *mut RuntimeSnapshot) -> HRESULT {
        unsafe { self.copy(size, version, output) }
    }
}

fn loaded(path: &Path) -> bool {
    let mut module = HMODULE::default();
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(wide(path).as_ptr()),
            &mut module,
        )
        .is_ok()
            && !module.is_invalid()
    }
}
pub fn worker_child_main() {
    use super::support::{
        document::Document,
        providers::{Kind, Manager, Provider},
        schedule::HostCapsule,
    };
    use std::rc::Rc;
    use windows::Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize, IClassFactory},
        UI::TextServices::*,
    };
    use windows_core::{GUID, Interface};
    let case = std::env::var("KEYRILL_WORKER_CASE").expect("explicit worker child case");
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap() };
    let files = Files::new();
    files.write(
        "en.add",
        if case == "drain" {
            b"hello\n".as_slice()
        } else {
            b"".as_slice()
        },
    );
    files.write("ru.add", b"");
    let _policy = Environment::set("KEYRILL_WINDOWS_POLICY", "model");
    let _en = Environment::set("KEYRILL_WINDOWS_EN_DICTIONARY", files.path("en.add"));
    let _ru = Environment::set("KEYRILL_WINDOWS_RU_DICTIONARY", files.path("en.add"));
    let path = PathBuf::from(
        std::env::var_os(if case == "production" {
            "KEYRILL_PRODUCTION_DLL"
        } else {
            "KEYRILL_TEST_DLL"
        })
        .unwrap(),
    );
    assert!(!loaded(&path), "child begins with unloaded DLL");
    let module = if case == "production" {
        Module::load(&path)
    } else {
        Module::test()
    };
    type GetClass =
        unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
    let get: GetClass = unsafe { module.export(b"DllGetClassObject\0") };
    let unload: unsafe extern "system" fn() -> HRESULT =
        unsafe { module.export(b"DllCanUnloadNow\0") };
    let events = Events::new();
    let boundary = case
        .strip_prefix("boundary-")
        .map(|s| s.parse::<u32>().unwrap());
    if let Some(boundary) = boundary {
        assert!([BEFORE_WORK, BEFORE_READ, CLEANUP, AFTER_COUNT].contains(&boundary));
        events.pause.store(boundary, Ordering::Release);
    }
    let failure = match case.as_str() {
        "startup-event" => Some(Failure::Event),
        "startup-pin" => Some(Failure::Pin),
        "startup-submit" => Some(Failure::Submit),
        "startup-long" => Some(Failure::LongRunning),
        "drain" => Some(Failure::None),
        _ => None,
    };
    assert!(
        boundary.is_some() || failure.is_some() || case == "production",
        "known worker child case"
    );
    let native = failure.map(|failure| {
        Native::new(
            if case == "drain" { Io::Drain } else { Io::Real },
            failure,
            1,
            false,
        )
    });
    let set = if case == "production" {
        None
    } else {
        Some(unsafe {
            module.export::<unsafe extern "system" fn(*const Hooks) -> BOOL>(
                b"KeyrillTestWorkerHooks\0",
            )
        })
    };
    if let Some(set) = set {
        assert!(
            unsafe {
                set(&native
                    .as_ref()
                    .map_or_else(|| events.hooks(), |s| s.hooks()))
            }
            .as_bool()
        );
    }
    let host = Rc::new(HostCapsule::default());
    let document = Document::new(Rc::clone(&host), 101);
    let context = unsafe {
        ITfContext::from_raw(Provider::create(
            Rc::clone(&host),
            Kind::Document(Rc::clone(&document)),
            ITfContext::IID,
        ))
    };
    document.context.set(context.as_raw());
    let manager = Manager::new(Rc::clone(&host), context.as_raw());
    let thread = unsafe {
        ITfThreadMgr::from_raw(Provider::create(
            Rc::clone(&host),
            Kind::Manager(manager),
            ITfThreadMgr::IID,
        ))
    };
    let mut factory = null_mut();
    assert_eq!(
        unsafe {
            get(
                &GUID::from_u128(0x1a746212_84db_4acf_88b3_caaa26352774),
                &IClassFactory::IID,
                &mut factory,
            )
        },
        S_OK
    );
    let factory = unsafe { IClassFactory::from_raw(factory) };
    let mut service = null_mut();
    assert_eq!(
        unsafe {
            (factory.vtable().CreateInstance)(
                factory.as_raw(),
                null_mut(),
                &ITfTextInputProcessorEx::IID,
                &mut service,
            )
        },
        S_OK
    );
    let service = unsafe { ITfTextInputProcessorEx::from_raw(service) };
    assert_eq!(
        unsafe { (service.vtable().ActivateEx)(service.as_raw(), thread.as_raw(), 42, 0) },
        S_OK
    );
    if let Some(set) = set {
        assert!(unsafe { set(null()) }.as_bool());
    }
    let reader = service.cast::<RuntimeReader>().unwrap();
    if matches!(boundary, Some(BEFORE_WORK | BEFORE_READ)) {
        events.published.wait();
    } else if case == "drain" {
        native.as_ref().unwrap().entered.wait();
    } else {
        wait(|| {
            let mut value = RuntimeSnapshot::default();
            assert_eq!(unsafe { reader.read(48, 1, &mut value) }, S_OK);
            if failure.is_some() {
                value.error == 8 && value.effective == 0 && value.model_ready == 0
            } else {
                value.effective == 2
            }
        });
    }
    assert_eq!(unsafe { unload() }, S_FALSE);
    assert_eq!(
        unsafe { (service.vtable().base__.Deactivate)(service.as_raw()) },
        S_OK
    );
    if let Some(schedule) = native.as_ref() {
        if case == "drain" {
            schedule.draining.wait();
            assert!(schedule.cancelled.signaled());
            assert!(!schedule.completion_returned.load(Ordering::Acquire));
            let operation = schedule.operation.load(Ordering::Acquire) as *const OVERLAPPED;
            assert_eq!(
                unsafe { WaitForSingleObject((*operation).hEvent, 0) },
                WAIT_TIMEOUT
            );
            let mut flags = 0;
            unsafe {
                GetHandleInformation(
                    HANDLE(schedule.file.load(Ordering::Acquire) as *mut c_void),
                    &mut flags,
                )
                .unwrap();
                GetHandleInformation((*operation).hEvent, &mut flags).unwrap();
            }
        } else {
            assert_eq!(schedule.event_calls.load(Ordering::Acquire), 1);
            assert_eq!(schedule.opens.load(Ordering::Acquire), 0);
            assert_eq!(schedule.reads.load(Ordering::Acquire), 0);
            schedule.events.resume.signal();
        }
    }
    drop(reader);
    drop(service);
    drop(factory);
    host.check();
    if matches!(boundary, Some(CLEANUP | AFTER_COUNT)) {
        events.published.wait();
    }
    if let Some(boundary) = boundary {
        assert_eq!(
            unsafe { unload() },
            if boundary == AFTER_COUNT {
                S_OK
            } else {
                S_FALSE
            }
        );
    }
    if case == "drain" {
        assert_eq!(unsafe { unload() }, S_FALSE);
    }
    if failure.is_some() && case != "drain" {
        wait(|| unsafe { unload() == S_OK });
    }
    drop(module);
    if boundary.is_some() || case == "drain" {
        assert!(
            loaded(&path),
            "worker pins actual module after external reference retires"
        );
    }
    if boundary.is_some() {
        events.resume.signal();
    }
    if case == "drain" {
        native.as_ref().unwrap().resume.signal();
    }
    wait(|| !loaded(&path));
    if let Some(schedule) = native.as_ref() {
        if case == "drain" {
            assert!(schedule.completion_returned.load(Ordering::Acquire));
            assert_eq!(schedule.completions.load(Ordering::Acquire), 1);
            assert_eq!(schedule.reads.load(Ordering::Acquire), 1);
        }
        schedule.events.check();
    }
    if boundary.is_some() {
        events.check();
    }
    assert_eq!(
        *document.text.borrow(),
        "prefix suffix".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(document.writes.get(), 0);
    drop(thread);
    drop(context);
    host.check();
    unsafe { CoUninitialize() };
    println!("scenario runtime.dll.{case}");
}
