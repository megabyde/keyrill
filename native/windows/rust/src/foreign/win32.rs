//! Mutable Win32 buffers retain mutable pointer provenance across the native call.

use windows::Win32::Foundation::{HANDLE, HMODULE, HWND};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows_core::{BOOL, Error, PCWSTR, Result};

#[cfg(target_pointer_width = "64")]
pub(crate) use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW as window_long, SetWindowLongPtrW as set_window_long,
};

// The 32-bit SDK aliases use i32 where pointer-sized callers use isize.
#[cfg(target_pointer_width = "32")]
pub(crate) unsafe fn window_long(
    window: HWND,
    index: windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX,
) -> isize {
    // SAFETY: the caller preserves the native HWND and index contract.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(window, index) as isize }
}

#[cfg(target_pointer_width = "32")]
pub(crate) unsafe fn set_window_long(
    window: HWND,
    index: windows::Win32::UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX,
    value: isize,
) -> isize {
    // SAFETY: the caller preserves the native contract and both signed word types are 32 bits.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongW(window, index, value as i32)
            as isize
    }
}

// The pinned slice projections derive output pointers through as_ptr(). These bounded wrappers
// keep the SDK ABI and pass as_mut_ptr() for every writable buffer.
pub(crate) fn module_path(module: Option<HMODULE>, output: &mut [u16]) -> u32 {
    windows_core::link!("kernel32.dll" "system" fn GetModuleFileNameW(module: HMODULE, output: *mut u16, capacity: u32) -> u32);
    // SAFETY: the complete bounded output remains writable through this synchronous Win32 call.
    unsafe {
        GetModuleFileNameW(
            module.unwrap_or_default(),
            output.as_mut_ptr(),
            output.len() as u32,
        )
    }
}

pub(crate) fn class_name(window: HWND, output: &mut [u16]) -> i32 {
    windows_core::link!("user32.dll" "system" fn GetClassNameW(window: HWND, output: *mut u16, capacity: i32) -> i32);
    // SAFETY: the complete bounded output remains writable through this synchronous Win32 call.
    unsafe { GetClassNameW(window, output.as_mut_ptr(), output.len() as i32) }
}

pub(crate) unsafe fn environment(name: PCWSTR, output: &mut [u16]) -> u32 {
    windows_core::link!("kernel32.dll" "system" fn GetEnvironmentVariableW(name: PCWSTR, output: *mut u16, capacity: u32) -> u32);
    // SAFETY: the caller supplies a terminated name, and output is complete writable storage.
    unsafe { GetEnvironmentVariableW(name, output.as_mut_ptr(), output.len() as u32) }
}

pub(crate) fn first_volume(output: &mut [u16]) -> Result<HANDLE> {
    windows_core::link!("kernel32.dll" "system" fn FindFirstVolumeW(output: *mut u16, capacity: u32) -> HANDLE);
    // SAFETY: the complete bounded output remains writable through this synchronous Win32 call.
    let handle = unsafe { FindFirstVolumeW(output.as_mut_ptr(), output.len() as u32) };
    if handle.is_invalid() {
        Err(Error::from_thread())
    } else {
        Ok(handle)
    }
}

pub(crate) fn next_volume(enumeration: HANDLE, output: &mut [u16]) -> Result<()> {
    windows_core::link!("kernel32.dll" "system" fn FindNextVolumeW(enumeration: HANDLE, output: *mut u16, capacity: u32) -> BOOL);
    // SAFETY: the caller owns the enumeration handle, and output is complete writable storage.
    unsafe { FindNextVolumeW(enumeration, output.as_mut_ptr(), output.len() as u32).ok() }
}

pub(crate) fn volume_filesystem(file: HANDLE, output: &mut [u16]) -> Result<()> {
    windows_core::link!("kernel32.dll" "system" fn GetVolumeInformationByHandleW(file: HANDLE, volume: *mut u16, volume_capacity: u32, serial: *mut u32, component_length: *mut u32, flags: *mut u32, filesystem: *mut u16, filesystem_capacity: u32) -> BOOL);
    // SAFETY: file is borrowed, optional unused outputs are null, and output is complete writable storage.
    unsafe {
        GetVolumeInformationByHandleW(
            file,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            output.as_mut_ptr(),
            output.len() as u32,
        )
        .ok()
    }
}

pub(crate) type KeyTranslation =
    unsafe extern "system" fn(u32, u32, *const u8, *mut u16, i32, u32, HKL) -> i32;

windows_core::link!("user32.dll" "system" fn ToUnicodeEx(key: u32, scan: u32, keyboard: *const u8, output: *mut u16, capacity: i32, flags: u32, layout: HKL) -> i32);

pub(crate) const KEY_TRANSLATION: KeyTranslation = ToUnicodeEx;

pub(crate) fn translate(
    translation: KeyTranslation,
    key: u32,
    scan: u32,
    keyboard: &[u8; 256],
    output: &mut [u16],
    flags: u32,
    layout: HKL,
) -> i32 {
    // SAFETY: the complete keyboard snapshot and bounded output remain live through the call.
    unsafe {
        translation(
            key,
            scan,
            keyboard.as_ptr(),
            output.as_mut_ptr(),
            output.len() as i32,
            flags,
            layout,
        )
    }
}
