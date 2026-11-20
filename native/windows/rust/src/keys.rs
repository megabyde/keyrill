use crate::profiles::{PROFILES, SERVICE_ID};
use keyrill_ffi::NativeEvent;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::TextServices::{
    TF_INPUTPROCESSORPROFILE, TF_IPP_FLAG_ACTIVE, TF_MOD_IGNORE_ALL_MODIFIER, TF_MOD_LALT,
    TF_MOD_LCONTROL, TF_PRESERVEDKEY, TF_PROFILETYPE_INPUTPROCESSOR,
};
use windows_core::GUID;

pub(crate) struct Binding {
    pub(crate) id: GUID,
    pub(crate) key: TF_PRESERVEDKEY,
    pub(crate) role: u32,
    pub(crate) description: &'static str,
}

pub(crate) const BINDINGS: [Binding; 3] = [
    Binding {
        id: GUID::from_u128(0xd7abce42_c8a9_4a43_990a_4a8d6c011ebb),
        key: TF_PRESERVEDKEY {
            uVKey: b'K' as u32,
            uModifiers: TF_MOD_LCONTROL | TF_MOD_LALT,
        },
        role: 4,
        description: "Keyrill manual conversion",
    },
    Binding {
        id: GUID::from_u128(0x16531b70_516e_4dfb_9f18_cb760de590b4),
        key: TF_PRESERVEDKEY {
            uVKey: b'Z' as u32,
            uModifiers: TF_MOD_LCONTROL | TF_MOD_LALT,
        },
        role: 5,
        description: "Keyrill reversal",
    },
    Binding {
        id: GUID::from_u128(0x997f7561_053d_475a_abb9_b11fcf16bb48),
        key: TF_PRESERVEDKEY {
            uVKey: VK_MENU.0 as u32,
            uModifiers: TF_MOD_IGNORE_ALL_MODIFIER,
        },
        role: 0,
        description: "Keyrill Alt press observer",
    },
];

pub(crate) enum Key {
    Modifier,
    Event(NativeEvent),
}

pub(crate) struct PhysicalKey {
    pub(crate) key: WPARAM,
    pub(crate) flags: LPARAM,
    pub(crate) keyboard: [u8; 256],
}

pub(crate) fn handoff_letter(
    physical: &PhysicalKey,
    layout: HKL,
    fresh: &[u8; 256],
) -> Option<NativeEvent> {
    let key = physical.key.0;
    let word_key = (b'A' as usize..=b'Z' as usize).contains(&key)
        || [
            VK_OEM_4,
            VK_OEM_6,
            VK_OEM_1,
            VK_OEM_7,
            VK_OEM_COMMA,
            VK_OEM_PERIOD,
            VK_OEM_3,
        ]
        .iter()
        .any(|candidate| candidate.0 as usize == key);
    if physical.keyboard != *fresh
        || !word_key
        || physical.flags.0 as u32 & 0xff000000 != 0
        || physical.flags.0 as u16 != 1
    {
        return None;
    }
    let Key::Event(event) = translate(physical.key, physical.flags, layout, fresh).ok()? else {
        return None;
    };
    let view = event.view().ok()?;
    if view.role != 1
        || view.units.len() != 1
        || !char::from_u32(u32::from(view.units[0])).is_some_and(char::is_alphabetic)
    {
        return None;
    }
    Some(event)
}

fn down(state: &[u8; 256], key: VIRTUAL_KEY) -> bool {
    state[key.0 as usize] & 0x80 != 0
}

pub(crate) fn exact_command_modifiers(state: &[u8; 256]) -> bool {
    down(state, VK_LCONTROL)
        && down(state, VK_LMENU)
        && ![
            VK_RCONTROL,
            VK_RMENU,
            VK_SHIFT,
            VK_LSHIFT,
            VK_RSHIFT,
            VK_LWIN,
            VK_RWIN,
        ]
        .iter()
        .any(|key| down(state, *key))
}

fn printable(text: &[u16]) -> bool {
    for decoded in char::decode_utf16(text.iter().copied()) {
        let Ok(character) = decoded else {
            return false;
        };
        let scalar = character as u32;
        if matches!(scalar, 0..=0x1f | 0x7f..=0x9f | 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
        {
            return false;
        }
    }
    true
}

fn event(role: u32, mode: u32, units: &[u16]) -> Result<Key, u32> {
    NativeEvent::new(role, mode, 1, units).map(Key::Event)
}

pub(crate) fn translate(
    key: WPARAM,
    flags: LPARAM,
    layout: HKL,
    state: &[u8; 256],
) -> Result<Key, u32> {
    if [
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ]
    .iter()
    .any(|modifier| key.0 == modifier.0 as usize)
    {
        return Ok(Key::Modifier);
    }
    let layout_id = layout.0 as usize;
    let mode = PROFILES
        .iter()
        .find(|profile| {
            layout_id & 0xffff == profile.language as usize
                && (layout_id >> 16) & 0xffff == profile.language as usize
        })
        .map_or(0, |profile| profile.mode);
    if mode == 0 || flags.0 as u16 != 1 {
        return event(6, mode, &[]);
    }
    if exact_command_modifiers(state)
        && let Some(binding) = BINDINGS
            .iter()
            .find(|binding| binding.role != 0 && key.0 == binding.key.uVKey as usize)
    {
        return event(binding.role, mode, &[]);
    }
    if [
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ]
    .iter()
    .any(|modifier| down(state, *modifier))
    {
        return event(6, mode, &[]);
    }
    let shift = [VK_SHIFT, VK_LSHIFT, VK_RSHIFT]
        .iter()
        .any(|modifier| down(state, *modifier));
    if !shift && key.0 == VK_BACK.0 as usize {
        return event(2, mode, &[]);
    }
    if !shift && key.0 == VK_ESCAPE.0 as usize {
        return event(3, mode, &[]);
    }
    let mut text = [0; 8];
    // SAFETY: the fixed keyboard state and output buffer are live for this non-mutating translation.
    let count = crate::foreign::win32::translate(
        key.0 as u32,
        (flags.0 as u32 >> 16) & 0xff,
        state,
        &mut text,
        4,
        layout,
    );
    if count <= 0 || count as usize > text.len() || !printable(&text[..count as usize]) {
        return event(6, mode, &[]);
    }
    event(1, mode, &text[..count as usize])
}

pub(crate) fn active_profile(profile: &TF_INPUTPROCESSORPROFILE, mode: u32) -> bool {
    profile.dwProfileType == TF_PROFILETYPE_INPUTPROCESSOR
        && profile.clsid == SERVICE_ID
        && profile.dwFlags & TF_IPP_FLAG_ACTIVE != 0
        && PROFILES.iter().any(|entry| {
            entry.id == profile.guidProfile
                && entry.language == profile.langid
                && entry.mode == mode
        })
}

pub(crate) fn profile_layout(
    profile: &TF_INPUTPROCESSORPROFILE,
    mode: u32,
    substitute: HKL,
) -> HKL {
    if active_profile(profile, mode)
        && substitute.0 as usize == ((profile.langid as usize) << 16 | profile.langid as usize)
    {
        substitute
    } else {
        HKL::default()
    }
}
