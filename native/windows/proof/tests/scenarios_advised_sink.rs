use crate::support::{client::Client, dll::snapshot, document::Effect, oracle};
use windows::Win32::{
    Foundation::S_OK,
    UI::{Input::KeyboardAndMouse::*, TextServices::*},
};
use windows_core::{BOOL, GUID, Interface};

struct Keyboard([u8; 256]);
impl Keyboard {
    fn new() -> Self {
        let mut previous = [0; 256];
        unsafe {
            assert!(crate::support::keyboard_state(&mut previous));
            SetKeyboardState(&[0; 256]).unwrap();
        }
        Self(previous)
    }
    fn command(&self) {
        let mut state = [0; 256];
        for key in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_LMENU] {
            state[key.0 as usize] = 0x80;
        }
        unsafe { SetKeyboardState(&state).unwrap() };
    }
}
impl Drop for Keyboard {
    fn drop(&mut self) {
        unsafe { SetKeyboardState(&self.0).unwrap() };
    }
}

fn preserved(c: &Client, id: GUID) -> bool {
    // Rc keeps the registration alive without a RefCell borrow across COM dispatch.
    let advised = c.manager.key_sink.borrow().clone().expect("advised sink");
    let mut eaten = BOOL(1);
    unsafe {
        assert_eq!(
            (advised.sink.vtable().OnPreservedKey)(
                advised.sink.as_raw(),
                c.context_a.as_raw(),
                &id,
                &mut eaten
            ),
            S_OK
        );
    }
    c.host.check();
    eaten.as_bool()
}

fn bindings(c: &Client, cleanup: bool) -> Vec<(u32, GUID, u32, u32)> {
    let registrations = if cleanup {
        &c.manager.unpreserved_keys
    } else {
        &c.manager.preserved_keys
    };
    registrations
        .borrow()
        .iter()
        .map(|r| (r.client, r.id, r.key.uVKey, r.key.uModifiers))
        .collect()
}

#[test]
fn preserved_commands_use_actual_advised_sink_across_activations() {
    let expected = vec![
        (
            42,
            GUID::from_u128(0xd7abce42_c8a9_4a43_990a_4a8d6c011ebb),
            75,
            0xc0,
        ),
        (
            42,
            GUID::from_u128(0x16531b70_516e_4dfb_9f18_cb760de590b4),
            90,
            0xc0,
        ),
        (
            42,
            GUID::from_u128(0x997f7561_053d_475a_abb9_b11fcf16bb48),
            18,
            TF_MOD_IGNORE_ALL_MODIFIER,
        ),
    ];
    let mut stable = None;
    for _ in 0..2 {
        let c = Client::new();
        let keyboard = Keyboard::new();
        assert_eq!(c.manager.advises[2].get(), 1);
        {
            let advised = c.manager.key_sink.borrow();
            let advised = advised
                .as_ref()
                .expect("activation captures actual advised sink");
            assert_eq!(advised.client, 42);
            assert_eq!(advised.foreground, BOOL(1));
        }
        let registrations = bindings(&c, false);
        assert_eq!(registrations, expected);
        if let Some(previous) = &stable {
            assert_eq!(&registrations, previous);
        } else {
            stable = Some(registrations.clone());
        }
        assert!(!preserved(&c, GUID::zeroed()));
        assert_eq!((c.a.reads.get(), c.a.request.get()), (0, 0));
        oracle::no_effects(&c);
        oracle::document(&c, "", 7);

        c.type_text("ghbdtn");
        oracle::document(&c, "ghbdtn", 13);
        let mark = c.a.active.borrow().as_ref().unwrap().pointer;
        let starts = c.a.starts.get();
        let ends = c.a.ends.get();
        keyboard.command();
        for (registration, text) in registrations[..2].iter().zip(["привет", "ghbdtn"]) {
            let writes = c.a.writes.get();
            let requests = c.a.request.get();
            let before = snapshot(&c.service);
            assert!(preserved(&c, registration.1));
            let after = snapshot(&c.service);
            assert_eq!(c.a.writes.get(), writes + 1);
            assert_eq!(c.a.request.get(), requests + 1);
            assert_eq!(
                c.a.operations
                    .borrow()
                    .iter()
                    .filter(|op| { op.request == c.a.request.get() && op.effect == Effect::Text })
                    .count(),
                1
            );
            assert_eq!(after.counts[8], before.counts[8] + 1);
            assert_eq!(after.counts[9], before.counts[9] + 1);
            assert_eq!(after.counts[11], before.counts[11]);
            oracle::document(&c, text, 13);
            assert_eq!(c.b.writes.get(), 0);
            assert_eq!(c.a.active.borrow().as_ref().unwrap().pointer, mark);
            assert_eq!((c.a.starts.get(), c.a.ends.get()), (starts, ends));
        }
        assert_eq!(c.deactivate(), S_OK);
        assert!(c.manager.key_sink.borrow().is_none());
        let cleanup = bindings(&c, true);
        assert_eq!(cleanup.len(), 3);
        for registration in &registrations {
            assert_eq!(cleanup.iter().filter(|r| *r == registration).count(), 1);
        }
        assert_eq!(c.deactivate(), S_OK);
        assert!(c.manager.key_sink.borrow().is_none());
        assert_eq!(bindings(&c, true), cleanup);
        assert_eq!(c.manager.unadvises[2].get(), 1);
        let host = std::rc::Rc::clone(&c.host);
        drop(c);
        assert_eq!(host.live_objects.get(), 0);
        assert_eq!(
            host.dependency_retains.get(),
            host.dependency_releases.get()
        );
        host.drained();
    }
}
