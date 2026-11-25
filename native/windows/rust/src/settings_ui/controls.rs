use super::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::{EM_SETLIMITTEXT, SetScrollInfo};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows_core::{PCWSTR, w};

impl State {
    pub(super) fn add(
        &self,
        class: PCWSTR,
        text: &str,
        id: u16,
        style: WINDOW_STYLE,
        rect: Rect,
    ) -> windows_core::Result<()> {
        let text = wide(text);
        // SAFETY: the class, text and parent live across synchronous creation.
        let child = unsafe {
            CreateWindowExW(
                if matches!(id, EN_PATH | RU_PATH | IDENTITY | APPLICATIONS) {
                    WS_EX_CLIENTEDGE
                } else {
                    WINDOW_EX_STYLE::default()
                },
                class,
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                0,
                0,
                0,
                0,
                Some(self.window.get()),
                Some(HMENU(id as usize as *mut _)),
                None,
                None,
            )?
        };
        self.controls.borrow_mut().push((child, rect));
        Ok(())
    }

    pub(super) fn create_controls(&self) -> windows_core::Result<()> {
        let label = |text, y| {
            self.add(
                w!("STATIC"),
                text,
                0,
                WINDOW_STYLE::default(),
                Rect {
                    x: 20,
                    y,
                    width: 690,
                    height: 22,
                    stretch: true,
                },
            )
        };
        let button = |text, id, x, y, width| {
            self.add(
                w!("BUTTON"),
                text,
                id,
                WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
                Rect {
                    x,
                    y,
                    width,
                    height: 28,
                    stretch: false,
                },
            )
        };
        let check = |text, id, y| {
            self.add(
                w!("BUTTON"),
                text,
                id,
                WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                Rect {
                    x: 20,
                    y,
                    width: 690,
                    height: 25,
                    stretch: true,
                },
            )
        };
        let edit = |id, x, y, width, stretch| {
            self.add(
                w!("EDIT"),
                "",
                id,
                WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                Rect {
                    x,
                    y,
                    width,
                    height: 26,
                    stretch,
                },
            )?;
            send(self.control(id), EM_SETLIMITTEXT, EDIT_LIMIT, 0);
            Ok::<(), windows_core::Error>(())
        };
        check(
            "Enable &automatic correction in admitted controls",
            AUTOMATIC,
            16,
        )?;
        check(
            "Switch English/Russian input &source after automatic correction",
            SOURCE,
            46,
        )?;
        label("English dictionary", 88)?;
        check(
            "Use the &default English dictionary location",
            EN_DEFAULT,
            111,
        )?;
        self.add(
            w!("STATIC"),
            "English &file",
            0,
            WINDOW_STYLE::default(),
            Rect {
                x: 20,
                y: 144,
                width: 94,
                height: 22,
                stretch: false,
            },
        )?;
        edit(EN_PATH, 116, 140, 480, true)?;
        button("&Browse...", EN_BROWSE, 610, 140, 100)?;
        label("Russian dictionary", 180)?;
        check(
            "Use the default &Russian dictionary location",
            RU_DEFAULT,
            203,
        )?;
        self.add(
            w!("STATIC"),
            "Russian fi&le",
            0,
            WINDOW_STYLE::default(),
            Rect {
                x: 20,
                y: 236,
                width: 94,
                height: 22,
                stretch: false,
            },
        )?;
        edit(RU_PATH, 116, 232, 480, true)?;
        button("B&rowse...", RU_BROWSE, 610, 232, 100)?;
        label(
            "Application overrides. Select a row to edit its identity and policy.",
            273,
        )?;
        self.add(
            w!("LISTBOX"),
            "",
            APPLICATIONS,
            WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32 | LBS_NOINTEGRALHEIGHT as u32),
            Rect {
                x: 20,
                y: 299,
                width: 690,
                height: 105,
                stretch: true,
            },
        )?;
        self.add(
            w!("STATIC"),
            "Identity &type",
            0,
            WINDOW_STYLE::default(),
            Rect {
                x: 20,
                y: 417,
                width: 180,
                height: 20,
                stretch: false,
            },
        )?;
        self.add(
            w!("COMBOBOX"),
            "",
            IDENTITY_TYPE,
            WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            Rect {
                x: 20,
                y: 440,
                width: 180,
                height: 120,
                stretch: false,
            },
        )?;
        for item in ["Executable path", "Package family"] {
            combo_add(self.control(IDENTITY_TYPE), item);
        }
        send(self.control(IDENTITY_TYPE), CB_SETCURSEL, 0, 0);
        self.add(
            w!("STATIC"),
            "Application &identity",
            0,
            WINDOW_STYLE::default(),
            Rect {
                x: 214,
                y: 417,
                width: 380,
                height: 20,
                stretch: true,
            },
        )?;
        edit(IDENTITY, 214, 440, 382, true)?;
        button("Choose &exe...", EXE_BROWSE, 610, 440, 100)?;
        self.add(
            w!("STATIC"),
            "&Policy",
            0,
            WINDOW_STYLE::default(),
            Rect {
                x: 20,
                y: 479,
                width: 180,
                height: 20,
                stretch: false,
            },
        )?;
        self.add(
            w!("COMBOBOX"),
            "",
            POLICY,
            WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            Rect {
                x: 20,
                y: 502,
                width: 180,
                height: 120,
                stretch: false,
            },
        )?;
        for item in ["Automatic", "Manual commands only", "Off"] {
            combo_add(self.control(POLICY), item);
        }
        send(self.control(POLICY), CB_SETCURSEL, 1, 0);
        button("&Add / update", UPSERT, 214, 502, 140)?;
        button("&Remove selected", REMOVE, 368, 502, 160)?;
        button("&New", NEW, 542, 502, 100)?;
        label(
            "Automatic follows the global switch. Off prevents text inspection and composition.",
            543,
        )?;
        self.add(
            w!("STATIC"),
            "",
            READINESS,
            WINDOW_STYLE::default(),
            Rect {
                x: 20,
                y: 575,
                width: 690,
                height: 60,
                stretch: true,
            },
        )?;
        self.add(
            w!("STATIC"),
            "",
            STATUS,
            WINDOW_STYLE::default(),
            Rect {
                x: 20,
                y: 642,
                width: 690,
                height: 52,
                stretch: true,
            },
        )?;
        button("&Save", SAVE, 20, 708, 100)?;
        button("Re&load", RELOAD, 134, 708, 100)?;
        button("&Close", CLOSE, 610, 708, 100)?;
        // SAFETY: the UI owns this HWND and polls only content-free worker reports.
        if unsafe { SetTimer(Some(self.window.get()), 1, 100, None) } == 0 {
            return Err(windows_core::Error::from_thread());
        }
        self.layout();
        self.reload();
        Ok(())
    }

    pub(super) fn layout(&self) {
        if self.laying_out.replace(true) {
            return;
        }
        struct Layout<'a>(&'a Cell<bool>);
        impl Drop for Layout<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _layout = Layout(&self.laying_out);
        let window = self.window.get();
        let mut client = RECT::default();
        // SAFETY: the window belongs to this thread and the output is writable.
        unsafe {
            let _ = GetClientRect(window, &mut client);
        }
        if self.window.get() != window || window.is_invalid() {
            return;
        }
        // SAFETY: the HWND is live on this thread.
        let dpi = unsafe { GetDpiForWindow(window) }.max(96) as i32;
        let scale = |value: i32| value * dpi / 96;
        let page = (client.bottom * 96 / dpi).max(1);
        let scroll = self.scroll.get().clamp(0, (754 - page).max(0));
        self.scroll.set(scroll);
        let info = SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: 753,
            nPage: page as u32,
            nPos: scroll,
            ..Default::default()
        };
        // SAFETY: the stack descriptor remains valid throughout the native scroll-bar update.
        unsafe {
            SetScrollInfo(window, SB_VERT, &info, true);
            let _ = GetClientRect(window, &mut client);
        }
        if self.window.get() != window {
            return;
        }
        let extra = (client.right * 96 / dpi - 730).max(0);
        let controls = self.controls.borrow().clone();
        // SAFETY: create an ordinary DPI-sized UI font, retained until replacement or close.
        let font = unsafe {
            CreateFontW(
                -scale(16),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                DEFAULT_QUALITY,
                DEFAULT_PITCH.0 as u32,
                w!("Segoe UI"),
            )
        };
        let old = self.font.replace(font);
        for (child, rect) in controls {
            if self.window.get() != window {
                break;
            }
            let x = rect.x
                + if !rect.stretch && rect.x >= 610 {
                    extra
                } else {
                    0
                };
            let width = rect.width + if rect.stretch { extra } else { 0 };
            // SAFETY: the copied child HWNDs belong to this window. No Rust borrow crosses messages.
            unsafe {
                let _ = MoveWindow(
                    child,
                    scale(x),
                    scale(rect.y - scroll),
                    scale(width),
                    scale(rect.height),
                    true,
                );
            }
            if self.window.get() != window {
                break;
            }
            send(child, WM_SETFONT, font.0 as usize, 1);
        }
        if !old.is_invalid() {
            // SAFETY: all controls now use the replacement font.
            unsafe {
                let _ = DeleteObject(old.into());
            }
        }
    }

    pub(super) fn scroll_by(&self, amount: i32) {
        self.scroll.set(self.scroll.get().saturating_add(amount));
        self.layout();
    }

    pub(super) fn reveal_focus(&self) {
        // SAFETY: this reads only the current UI thread's focus handle.
        let focused = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() };
        let rect = self
            .controls
            .borrow()
            .iter()
            .find(|(child, _)| *child == focused)
            .map(|(_, rect)| *rect);
        if let Some(rect) = rect {
            let mut client = RECT::default();
            // SAFETY: window and output belong to this UI thread.
            let dpi = unsafe {
                let _ = GetClientRect(self.window.get(), &mut client);
                GetDpiForWindow(self.window.get())
            }
            .max(96) as i32;
            let page = client.bottom * 96 / dpi;
            let bottom = rect.y + rect.height.min(30) + 8;
            if rect.y < self.scroll.get() {
                self.scroll.set((rect.y - 8).max(0));
                self.layout();
            } else if bottom > self.scroll.get() + page {
                self.scroll.set(bottom - page);
                self.layout();
            }
        }
    }

    pub(super) fn dictionary_enabled(&self) {
        for (default, path, browse) in [
            (EN_DEFAULT, EN_PATH, EN_BROWSE),
            (RU_DEFAULT, RU_PATH, RU_BROWSE),
        ] {
            let explicit = !is_checked(self.control(default));
            enable(self.control(path), explicit);
            enable(self.control(browse), explicit);
        }
    }

    pub(super) fn rows(&self) {
        let applications = self.draft.borrow().applications.clone();
        send(self.control(APPLICATIONS), LB_RESETCONTENT, 0, 0);
        for (identity, policy) in applications {
            let row = wide(&format!(
                "{}   |   {}",
                identity.as_str(),
                policy_name(policy)
            ));
            send(
                self.control(APPLICATIONS),
                LB_ADDSTRING,
                0,
                row.as_ptr() as isize,
            );
        }
    }
}
