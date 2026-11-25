use std::{ffi::c_void, path::PathBuf};
use windows::Win32::{Foundation::*, System::LibraryLoader::*};
use windows_core::{HRESULT, PCSTR, PCWSTR};
pub const VERSION: u32 = 3;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct HostIdentity {
    pub process: u32,
    pub thread: u32,
    pub started: u64,
    pub kind: u32,
    pub version: [u32; 4],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FieldWindow {
    pub handle: HWND,
    pub process: u32,
    pub thread: u32,
    pub control: i32,
    pub style: isize,
    pub class_name: [u16; 64],
    pub enabled: u32,
    pub complete: u32,
}
impl Default for FieldWindow {
    fn default() -> Self {
        Self {
            handle: HWND::default(),
            process: 0,
            thread: 0,
            control: 0,
            style: 0,
            class_name: [0; 64],
            enabled: 0,
            complete: 0,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FieldFacts {
    pub host: HostIdentity,
    pub process: u32,
    pub thread: u32,
    pub view: HWND,
    pub focus: HWND,
    pub foreground: HWND,
    pub root: HWND,
    pub field: FieldWindow,
    pub parent: FieldWindow,
    pub grandparent: FieldWindow,
    pub proof_root: FieldWindow,
    pub proof_process: u32,
}
#[repr(C)]
pub struct Dependencies {
    pub size: u32,
    pub version: u32,
    pub context: *mut c_void,
    pub retain: Option<unsafe extern "system" fn(*mut c_void)>,
    pub release: Option<unsafe extern "system" fn(*mut c_void)>,
    pub profiles: *mut c_void,
    pub layouts: Option<unsafe extern "system" fn(i32, *mut isize) -> i32>,
    pub capture: Option<
        unsafe extern "system" fn(
            *mut c_void,
            HWND,
            *const HostIdentity,
            *mut FieldFacts,
        ) -> HRESULT,
    >,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Event {
    pub role: u32,
    pub mode: u32,
    pub context: u32,
    pub length: u32,
    pub text: [u16; 8],
}
impl Event {
    pub fn text(unit: u16) -> Self {
        Self {
            role: 1,
            mode: 1,
            context: 1,
            length: 1,
            text: [unit, 0, 0, 0, 0, 0, 0, 0],
        }
    }
    pub fn command(role: u32) -> Self {
        Self {
            role,
            mode: 1,
            context: 1,
            ..Self::default()
        }
    }
}
#[repr(C)]
#[derive(Default, Debug)]
pub struct Probe {
    pub size: u32,
    pub version: u32,
    pub outer: HRESULT,
    pub callback: HRESULT,
    pub reserved: u32,
    pub entered: i32,
    pub completed: i32,
    pub effect: u32,
    pub identity: usize,
}
const _: () = {
    assert!(size_of::<HostIdentity>() == 40);
    assert!(align_of::<HostIdentity>() == 8);
    assert!(align_of::<FieldWindow>() == align_of::<usize>());
    assert!(align_of::<FieldFacts>() == 8);
    assert!(align_of::<Dependencies>() == align_of::<usize>());
    assert!(align_of::<Probe>() == align_of::<usize>());
    assert!(size_of::<Dependencies>() == 8 + 6 * size_of::<usize>());
    assert!(size_of::<Event>() == 32);
    assert!(size_of::<Probe>() == 32 + size_of::<usize>());
    assert!(std::mem::offset_of!(Dependencies, capture) == 8 + 5 * size_of::<usize>());
    assert!(std::mem::offset_of!(Probe, identity) == 32);
};
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<FieldWindow>() == 168);
    assert!(size_of::<FieldFacts>() == 760);
    assert!(std::mem::offset_of!(FieldFacts, proof_process) == 752);
};
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<FieldWindow>() == 156);
    assert!(size_of::<FieldFacts>() == 696);
    assert!(std::mem::offset_of!(FieldFacts, proof_process) == 688);
};

pub struct Api {
    module: HMODULE,
    pub create: unsafe extern "system" fn(
        u32,
        *const Dependencies,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> HRESULT,
    pub release: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    pub event:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const Event, *mut i32) -> HRESULT,
    pub probe: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, *mut Probe) -> HRESULT,
    pub stopped: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
    pub panic: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
}
impl Api {
    pub unsafe fn export<T: Copy>(&self, name: &[u8]) -> T {
        let value = unsafe { GetProcAddress(self.module, PCSTR(name.as_ptr())) }
            .expect("required DLL export");
        assert_eq!(size_of::<T>(), size_of_val(&value));
        unsafe { std::mem::transmute_copy(&value) }
    }
    pub fn load() -> Self {
        let path = PathBuf::from(
            std::env::var_os("KEYRILL_TEST_DLL").expect("runner must supply KEYRILL_TEST_DLL"),
        );
        assert!(path.is_absolute());
        let path: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // The controlled runner deliberately retains the module reference until process exit.
        let module =
            unsafe { LoadLibraryW(PCWSTR(path.as_ptr())).expect("load explicit test DLL") };
        unsafe fn symbol<T: Copy>(module: HMODULE, name: &[u8]) -> T {
            let value = unsafe { GetProcAddress(module, PCSTR(name.as_ptr())) }
                .expect("required test export");
            assert_eq!(size_of::<T>(), size_of_val(&value));
            unsafe { std::mem::transmute_copy(&value) }
        }
        unsafe {
            let digest: safe_digest::Call = symbol(module, b"KeyrillTestBuildDigest\0");
            let mut actual = [0; 8];
            assert_eq!(digest(8, actual.as_mut_ptr()), S_OK);
            let expected = std::env::var("KEYRILL_TEST_BUILD_DIGEST")
                .expect("runner must supply matching build digest");
            let words: Vec<_> = expected
                .split(',')
                .map(|s| {
                    u32::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex digest word")
                })
                .collect();
            assert_eq!(
                actual.as_slice(),
                words.as_slice(),
                "loaded DLL build identity"
            );
            Self {
                module,
                create: symbol(module, b"KeyrillTestCreate\0"),
                release: symbol(module, b"KeyrillTestRelease\0"),
                event: symbol(module, b"KeyrillTestEvent\0"),
                probe: symbol(module, b"KeyrillTestProbe\0"),
                stopped: symbol(module, b"KeyrillTestStopped\0"),
                panic: symbol(module, b"KeyrillTestPanic\0"),
            }
        }
    }
}
mod safe_digest {
    pub type Call = unsafe extern "system" fn(u32, *mut u32) -> windows_core::HRESULT;
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldIdentity {
    pub kind: u32,
    pub field: HWND,
    pub parent: HWND,
    pub root: HWND,
    pub process: u32,
    pub thread: u32,
    pub root_thread: u32,
}
const _: () = {
    assert!(align_of::<FieldIdentity>() == align_of::<usize>());
    assert!(std::mem::offset_of!(Event, text) == 16);
    assert!(std::mem::offset_of!(HostIdentity, started) == 8);
    assert!(std::mem::offset_of!(HostIdentity, version) == 20);
    assert!(std::mem::offset_of!(Dependencies, profiles) == 8 + 3 * size_of::<usize>());
    assert!(std::mem::offset_of!(Probe, reserved) == 16);
};
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<FieldIdentity>() == 48);
    assert!(std::mem::offset_of!(FieldIdentity, field) == 8);
    assert!(std::mem::offset_of!(FieldIdentity, root_thread) == 40);
    assert!(std::mem::offset_of!(FieldWindow, style) == 24);
    assert!(std::mem::offset_of!(FieldWindow, class_name) == 32);
    assert!(std::mem::offset_of!(FieldWindow, enabled) == 160);
    assert!(std::mem::offset_of!(FieldFacts, field) == 80);
};
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<FieldIdentity>() == 28);
    assert!(std::mem::offset_of!(FieldIdentity, field) == 4);
    assert!(std::mem::offset_of!(FieldIdentity, root_thread) == 24);
    assert!(std::mem::offset_of!(FieldWindow, style) == 16);
    assert!(std::mem::offset_of!(FieldWindow, class_name) == 20);
    assert!(std::mem::offset_of!(FieldWindow, enabled) == 148);
    assert!(std::mem::offset_of!(FieldFacts, field) == 64);
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub activation: u32,
    pub active: u32,
    pub stopped: u32,
    pub mode: u32,
    pub saturated: u32,
    pub request_present: u32,
    pub request_result: u32,
    pub session_present: u32,
    pub session_result: u32,
    pub counts: [u32; 12],
    pub refusals: [u32; 26],
    pub first: [u32; 4],
    pub latest: [u32; 4],
}
#[windows_core::interface("f75f909b-293a-4a7b-a172-f5a061880352")]
pub unsafe trait SnapshotReader: windows_core::IUnknown {
    fn copy(&self, size: u32, version: u32, output: *mut Snapshot) -> HRESULT;
}
const _: () = {
    assert!(size_of::<Snapshot>() == 220);
    assert!(std::mem::offset_of!(Snapshot, counts) == 36);
    assert!(std::mem::offset_of!(Snapshot, first) == 188);
};
pub fn snapshot(service: &windows::Win32::UI::TextServices::ITfTextInputProcessorEx) -> Snapshot {
    use windows_core::Interface;
    let reader = service.cast::<SnapshotReader>().unwrap();
    let mut value = Snapshot::default();
    unsafe {
        assert_eq!(
            reader.copy(size_of::<Snapshot>() as u32, 1, &mut value),
            S_OK
        );
    }
    value
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotV2 {
    pub base: Snapshot,
    pub lifecycle: [u32; 86],
}
impl Default for SnapshotV2 {
    fn default() -> Self {
        Self {
            base: Snapshot::default(),
            lifecycle: [0; 86],
        }
    }
}
impl SnapshotV2 {
    pub fn last(&self) -> [u32; 5] {
        let count = self.lifecycle[3] as usize;
        assert!((1..=16).contains(&count));
        self.lifecycle[6 + (count - 1) * 5..6 + count * 5]
            .try_into()
            .unwrap()
    }
    pub fn kinds(&self) -> Vec<u32> {
        self.lifecycle[6..6 + self.lifecycle[3] as usize * 5]
            .as_chunks::<5>()
            .0
            .iter()
            .map(|e| e[1])
            .collect()
    }
}
#[windows_core::interface("4f89429e-a955-4885-bb33-c78ad1982481")]
pub unsafe trait SnapshotReaderV2: windows_core::IUnknown {
    fn copy(&self, size: u32, version: u32, output: *mut SnapshotV2) -> HRESULT;
}
const _: () = {
    assert!(size_of::<SnapshotV2>() == 564);
    assert!(std::mem::offset_of!(SnapshotV2, lifecycle) == 220);
};
pub fn snapshot_v2(
    service: &windows::Win32::UI::TextServices::ITfTextInputProcessorEx,
) -> SnapshotV2 {
    use windows_core::Interface;
    let reader = service.cast::<SnapshotReaderV2>().unwrap();
    let mut value = SnapshotV2::default();
    unsafe {
        assert_eq!(reader.copy(564, 2, &mut value), S_OK);
    }
    value
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub reason: u32,
    pub result: u32,
    pub focus: u32,
    pub scope: u32,
    pub request: u32,
    pub session: u32,
    pub entered: u32,
    pub completed: u32,
    pub selection: u32,
    pub acp: u32,
}
const _: () = assert!(size_of::<Metadata>() == 40);
impl SnapshotReaderV2 {
    pub unsafe fn read(&self, size: u32, version: u32, out: *mut SnapshotV2) -> HRESULT {
        unsafe { self.copy(size, version, out) }
    }
}
impl SnapshotReader {
    pub unsafe fn read(&self, size: u32, version: u32, out: *mut Snapshot) -> HRESULT {
        unsafe { self.copy(size, version, out) }
    }
}
