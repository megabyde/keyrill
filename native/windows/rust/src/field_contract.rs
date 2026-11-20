//! Fixed native-field facts passed by the SDK test substrate.

use windows::Win32::Foundation::HWND;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct HostIdentity {
    pub(crate) process: u32,
    pub(crate) thread: u32,
    pub(crate) started: u64,
    pub(crate) kind: u32,
    pub(crate) version: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Window {
    pub(crate) handle: HWND,
    pub(crate) process: u32,
    pub(crate) thread: u32,
    pub(crate) control: i32,
    pub(crate) style: isize,
    pub(crate) class_name: [u16; 64],
    pub(crate) enabled: u32,
    pub(crate) complete: u32,
}

impl Default for Window {
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
pub(crate) struct Facts {
    pub(crate) host: HostIdentity,
    pub(crate) process: u32,
    pub(crate) thread: u32,
    pub(crate) view: HWND,
    pub(crate) focus: HWND,
    pub(crate) foreground: HWND,
    pub(crate) root: HWND,
    pub(crate) field: Window,
    pub(crate) parent: Window,
    pub(crate) grandparent: Window,
    pub(crate) proof_root: Window,
    pub(crate) proof_process: u32,
}

const _: () = assert!(size_of::<HostIdentity>() == 40);
const _: () = assert!(
    size_of::<Window>()
        == if cfg!(target_pointer_width = "64") {
            168
        } else {
            156
        }
);
const _: () = assert!(
    size_of::<Facts>()
        == if cfg!(target_pointer_width = "64") {
            760
        } else {
            696
        }
);
