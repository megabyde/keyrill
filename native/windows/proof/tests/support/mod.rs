#![allow(dead_code)]
pub mod client;
pub mod com_slots;
pub mod dll;
pub mod document;
pub mod oracle;
pub mod providers;
pub mod schedule;

pub fn keyboard_state(output: &mut [u8; 256]) -> bool {
    windows_core::link!("user32.dll" "system" fn GetKeyboardState(output: *mut u8) -> windows_core::BOOL);
    // The pinned SDK projection derives its output pointer from as_ptr(), losing mutable provenance.
    // SAFETY: output is a complete writable keyboard snapshot retained through the native call.
    unsafe { GetKeyboardState(output.as_mut_ptr()).as_bool() }
}
