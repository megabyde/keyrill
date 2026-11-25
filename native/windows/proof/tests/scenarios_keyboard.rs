use crate::support::{
    client::Client,
    dll::{Api, Event},
    oracle,
    providers::{PROFILE_EN, SERVICE_ID},
};
use windows::Win32::{
    Foundation::*,
    UI::{Input::KeyboardAndMouse::*, TextServices::*},
};
use windows_core::{HRESULT, Interface};

type Translate =
    unsafe extern "system" fn(WPARAM, LPARAM, HKL, *const u8, *mut Event, *mut i32) -> HRESULT;
fn translated(
    call: Translate,
    key: u16,
    flags: isize,
    layout: HKL,
    state: &[u8; 256],
) -> (Event, i32) {
    let mut event = Event::default();
    let mut modifier = -1;
    unsafe {
        assert_eq!(
            call(
                WPARAM(key as usize),
                LPARAM(flags),
                layout,
                state.as_ptr(),
                &mut event,
                &mut modifier
            ),
            S_OK
        );
    }
    (event, modifier)
}
#[test]
fn keyboard_actual_layout_shift_caps_repeat_and_modifier_axes() {
    let api = Api::load();
    let call: Translate = unsafe { api.export(b"KeyrillTestTranslate\0") };
    let en = HKL(0x04090409usize as *mut _);
    let ru = HKL(0x04190419usize as *mut _);
    let mut state = [0; 256];
    for (shift, caps, expected) in [(0, 0, 'g'), (0x80, 0, 'G'), (0, 1, 'G')] {
        state[VK_SHIFT.0 as usize] = shift;
        state[VK_CAPITAL.0 as usize] = caps;
        let (event, modifier) = translated(call, b'G' as u16, 0x220001, en, &state);
        assert_eq!(
            (event.role, event.length, event.text[0], modifier),
            (1, 1, expected as u16, 0)
        );
    }
    state.fill(0);
    state[VK_CONTROL.0 as usize] = 0x80;
    assert_eq!(
        translated(call, b'G' as u16, 0x220001, en, &state).0.role,
        6
    );
    state.fill(0);
    let c = Client::new();
    c.type_text("gh");
    let event = translated(call, b'G' as u16, 0x220003, en, &state).0;
    assert_eq!(event.role, 6);
    assert!(!c.event(event));
    assert!(c.a.active.borrow().is_none());
    oracle::document(&c, "gh", 9);
    assert_eq!(
        translated(call, b'G' as u16, 0x220000, en, &state).0.role,
        6
    );
    for key in [VK_TAB, VK_RETURN, VK_F12, VK_DELETE] {
        assert_eq!(translated(call, key.0, 1, en, &state).0.role, 6);
    }
    for key in [
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LWIN,
        VK_RWIN,
    ] {
        assert_eq!(translated(call, key.0, 1, en, &state).1, 1);
    }
    let mut layouts = [HKL::default(); 64];
    let count = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
    assert!((0..=64).contains(&count));
    let russian = layouts[..count as usize].contains(&ru);
    if russian {
        assert_eq!(
            translated(call, b'H' as u16, 0x230001, ru, &state).0.text[0],
            'р' as u16
        );
    } else {
        eprintln!("SKIP: RU layout translation; Russian layout is not already loaded");
    }
    let oem = [
        (VK_OEM_1, ";:", "жЖ"),
        (VK_OEM_PLUS, "=+", "=+"),
        (VK_OEM_COMMA, ",<", "бБ"),
        (VK_OEM_MINUS, "-_", "-_"),
        (VK_OEM_PERIOD, ".>", "юЮ"),
        (VK_OEM_2, "/?", ".,"),
        (VK_OEM_3, "`~", "ёЁ"),
        (VK_OEM_4, "[{", "хХ"),
        (VK_OEM_5, "\\|", "\\/"),
        (VK_OEM_6, "]}", "ъЪ"),
        (VK_OEM_7, "'\"", "эЭ"),
        (VK_OEM_102, "\\|", "\\/"),
    ];
    for layout in [en, ru] {
        if layout == ru && !russian {
            continue;
        }
        for (key, english, russian) in oem {
            for shift in [false, true] {
                for caps in [false, true] {
                    state[VK_SHIFT.0 as usize] = if shift { 0x80 } else { 0 };
                    state[VK_CAPITAL.0 as usize] = u8::from(caps);
                    let expected: Vec<_> = if layout == en { english } else { russian }
                        .encode_utf16()
                        .collect();
                    let cased = ('а' as u16..='ё' as u16).contains(&expected[0])
                        && expected[0] != expected[1];
                    let index = usize::from(if layout == ru && cased {
                        shift != caps
                    } else {
                        shift
                    });
                    let (event, modifier) = translated(call, key.0, 1, layout, &state);
                    assert_eq!(
                        (event.role, event.length, event.text[0], modifier),
                        (1, 1, expected[index], 0),
                        "key {:?} layout {:?} shift {shift} caps {caps}",
                        key,
                        layout
                    );
                }
            }
        }
    }
}

#[test]
fn keyboard_synthetic_tip_requires_exact_profile_and_substitute() {
    let api = Api::load();
    let call: unsafe extern "system" fn(
        *const TF_INPUTPROCESSORPROFILE,
        u32,
        HKL,
        *mut HKL,
    ) -> HRESULT = unsafe { api.export(b"KeyrillTestProfileLayout\0") };
    let en = HKL(0x04090409usize as *mut _);
    let ru = HKL(0x04190419usize as *mut _);
    let mut profile = TF_INPUTPROCESSORPROFILE {
        dwProfileType: TF_PROFILETYPE_INPUTPROCESSOR,
        clsid: SERVICE_ID,
        guidProfile: PROFILE_EN,
        langid: 0x409,
        dwFlags: TF_IPP_FLAG_ACTIVE,
        hkl: HKL(0xe0200409usize as *mut _),
        hklSubstitute: en,
        ..Default::default()
    };
    for (mode, substitute, expected) in [
        (1, en, en),
        (2, en, HKL::default()),
        (1, ru, HKL::default()),
    ] {
        profile.hklSubstitute = substitute;
        let mut output = HKL(std::ptr::dangling_mut());
        unsafe {
            assert_eq!(call(&profile, mode, substitute, &mut output), S_OK);
        }
        assert_eq!(output, expected);
    }
}

struct Keyboard {
    saved: [u8; 256],
    state: [u8; 256],
}
impl Keyboard {
    fn new() -> Self {
        let mut saved = [0; 256];
        unsafe {
            assert!(crate::support::keyboard_state(&mut saved));
            SetKeyboardState(&[0; 256]).unwrap();
        }
        Self {
            saved,
            state: [0; 256],
        }
    }
    fn set(&mut self, key: VIRTUAL_KEY, value: u8) {
        self.state[key.0 as usize] = value;
        for (both, left, right) in [
            (VK_CONTROL, VK_LCONTROL, VK_RCONTROL),
            (VK_MENU, VK_LMENU, VK_RMENU),
            (VK_SHIFT, VK_LSHIFT, VK_RSHIFT),
        ] {
            self.state[both.0 as usize] =
                self.state[left.0 as usize] | self.state[right.0 as usize];
        }
        unsafe {
            SetKeyboardState(&self.state).unwrap();
        }
    }
    fn down(&self, c: &Client, key: VIRTUAL_KEY) -> bool {
        let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
        let scan = unsafe { MapVirtualKeyW(key.0 as u32, MAPVK_VK_TO_VSC) } as isize;
        let mut eaten = false.into();
        unsafe {
            SetKeyboardState(&self.state).unwrap();
            assert_eq!(
                (sink.vtable().OnKeyDown)(
                    sink.as_raw(),
                    c.context_a.as_raw(),
                    WPARAM(key.0 as usize),
                    LPARAM((scan << 16) | 1),
                    &mut eaten
                ),
                S_OK
            );
        }
        c.host.check();
        eaten.as_bool()
    }
    fn up(&self, c: &Client, key: VIRTUAL_KEY) {
        let sink = c.service.cast::<ITfKeyEventSink>().unwrap();
        let mut eaten = true.into();
        unsafe {
            SetKeyboardState(&self.state).unwrap();
            assert_eq!(
                (sink.vtable().OnKeyUp)(
                    sink.as_raw(),
                    c.context_a.as_raw(),
                    WPARAM(key.0 as usize),
                    LPARAM(0xc0000001u32 as isize),
                    &mut eaten
                ),
                S_OK
            );
        }
        assert!(!eaten.as_bool());
        c.host.check();
    }
    fn text(&self, c: &Client, keys: &str) {
        for key in keys.encode_utf16() {
            assert!(self.down(c, VIRTUAL_KEY(key)));
            self.up(c, VIRTUAL_KEY(key));
        }
    }
    fn chord(&mut self, c: &Client, key: u16) -> bool {
        self.set(VK_LCONTROL, 0x80);
        assert!(!self.down(c, VK_CONTROL));
        self.set(VK_LMENU, 0x80);
        assert!(!self.down(c, VK_MENU));
        let eaten = self.down(c, VIRTUAL_KEY(key));
        self.up(c, VIRTUAL_KEY(key));
        self.set(VK_LMENU, 0);
        self.up(c, VK_MENU);
        self.set(VK_LCONTROL, 0);
        self.up(c, VK_CONTROL);
        eaten
    }
}
impl Drop for Keyboard {
    fn drop(&mut self) {
        unsafe {
            SetKeyboardState(&self.saved).unwrap();
        }
    }
}

#[test]
fn keyboard_native_snapshot_retains_mutable_output() {
    let mut keyboard = Keyboard::new();
    keyboard.set(VK_CAPITAL, 1);
    let mut observed = [0; 256];
    assert!(crate::support::keyboard_state(&mut observed));
    assert_eq!(observed[VK_CAPITAL.0 as usize], 1);
    assert!(observed[VK_CAPITAL.0 as usize] & 1 != 0);
}

#[test]
fn keyboard_oem_owned_words_convert_and_reverse_exact_case_and_punctuation() {
    for shifted in [false, true] {
        let c = Client::new();
        let mut keyboard = Keyboard::new();
        keyboard.set(VK_LSHIFT, if shifted { 0x80 } else { 0 });
        assert!(keyboard.down(&c, VK_OEM_4));
        keyboard.set(VK_LSHIFT, 0);
        keyboard.text(&c, "JHJIJ");
        assert!(keyboard.chord(&c, b'K' as u16));
        oracle::document(
            &c,
            if shifted {
                "Хорошо"
            } else {
                "хорошо"
            },
            13,
        );
        assert!(keyboard.chord(&c, b'Z' as u16));
        oracle::document(&c, if shifted { "{jhjij" } else { "[jhjij" }, 13);
    }
    let c = Client::new();
    let mut keyboard = Keyboard::new();
    assert!(keyboard.down(&c, VK_OEM_3));
    keyboard.text(&c, "KRF");
    assert!(keyboard.chord(&c, b'K' as u16));
    oracle::document(&c, "ёлка", 11);
    assert!(keyboard.chord(&c, b'Z' as u16));
    oracle::document(&c, "`krf", 11);
}

#[test]
fn keyboard_oem_ordinary_punctuation_finishes_before_native_passthrough() {
    let c = Client::new();
    let keyboard = Keyboard::new();
    keyboard.text(&c, "GHBDTN");
    assert!(!keyboard.down(&c, VK_OEM_MINUS));
    assert!(c.a.active.borrow().is_none());
    c.a.native_text(&[b'-' as u16]);
    oracle::document(&c, "ghbdtn-", 14);
}

#[test]
fn keyboard_loaded_russian_oem_manual_noop_preserves_first_receipt() {
    let ru = HKL(0x04190419usize as *mut _);
    let mut layouts = [HKL::default(); 64];
    let count = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
    assert!((0..=64).contains(&count));
    if !layouts[..count as usize].contains(&ru) {
        eprintln!("SKIP: Russian OEM command; layout is not already loaded");
        return;
    }
    let c = Client::new();
    let mut profile = c.profile_state.profile.get();
    profile.guidProfile = windows_core::GUID::from_u128(0x4c1e583c_f8e5_4b7d_b66c_b7e7319aed51);
    profile.langid = 0x419;
    c.profile_state.profile.set(profile);
    c.profile_state.substitute.set(ru);
    let sink = c
        .service
        .cast::<ITfInputProcessorProfileActivationSink>()
        .unwrap();
    unsafe {
        assert_eq!(
            (sink.vtable().OnActivated)(
                sink.as_raw(),
                TF_PROFILETYPE_INPUTPROCESSOR,
                0x419,
                &SERVICE_ID,
                &GUID_TFCAT_TIP_KEYBOARD,
                &profile.guidProfile,
                HKL::default(),
                TF_IPSINK_FLAG_ACTIVE
            ),
            S_OK
        );
    }
    let mut keyboard = Keyboard::new();
    assert!(keyboard.down(&c, VK_OEM_4));
    assert!(keyboard.chord(&c, b'K' as u16));
    oracle::document(&c, "[", 8);
    let writes = c.a.writes.get();
    assert!(keyboard.chord(&c, b'K' as u16));
    assert_eq!(c.a.writes.get(), writes);
    assert!(c.a.active.borrow().is_some());
    assert!(keyboard.chord(&c, b'Z' as u16));
    oracle::document(&c, "х", 8);
}
