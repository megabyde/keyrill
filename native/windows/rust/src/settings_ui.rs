//! On-demand Win32 editor for complete per-user preferences.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
mod controls;
mod native;
mod readiness;
#[cfg(test)]
mod tests;

pub use native::run;
use native::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::settings::{
    AppPolicy, ApplicationIdentity, DictionaryPath, LocalPath, Preferences, SaveOutcome,
    SettingsError, Snapshot, Store,
};

const AUTOMATIC: u16 = 101;
const SOURCE: u16 = 102;
const EN_DEFAULT: u16 = 103;
const EN_PATH: u16 = 104;
const EN_BROWSE: u16 = 105;
const RU_DEFAULT: u16 = 106;
const RU_PATH: u16 = 107;
const RU_BROWSE: u16 = 108;
const APPLICATIONS: u16 = 109;
const IDENTITY_TYPE: u16 = 110;
const IDENTITY: u16 = 111;
const EXE_BROWSE: u16 = 112;
const POLICY: u16 = 113;
const UPSERT: u16 = 114;
const REMOVE: u16 = 115;
const SAVE: u16 = 116;
const RELOAD: u16 = 117;
const CLOSE: u16 = 118;
const STATUS: u16 = 119;
const READINESS: u16 = 120;
const NEW: u16 = 121;
const EDIT_LIMIT: usize = 4095;

enum Source {
    CurrentUser,
    Preview(RefCell<Snapshot>),
    #[cfg(test)]
    Test(Rc<crate::settings::testing::Isolated>),
    #[cfg(test)]
    Preflight(Rc<crate::settings::testing::Isolated>),
}

impl Source {
    fn load(&self) -> Result<Snapshot, SettingsError> {
        match self {
            Self::CurrentUser => Store::current_user()?.load(),
            Self::Preview(snapshot) => Ok(snapshot.borrow().clone()),
            #[cfg(test)]
            Self::Test(store) | Self::Preflight(store) => store.0.load(),
        }
    }

    fn save(&self, opened: &Snapshot, draft: Preferences) -> SaveOutcome {
        match self {
            Self::CurrentUser => match Store::current_user() {
                Ok(store) => store.save(opened.revision, draft),
                Err(error) => SaveOutcome::Rejected(error),
            },
            Self::Preview(snapshot) => {
                let saved = Snapshot {
                    value: Arc::new(draft),
                    ..opened.clone()
                };
                *snapshot.borrow_mut() = saved.clone();
                SaveOutcome::Saved(saved)
            }
            #[cfg(test)]
            Self::Test(store) | Self::Preflight(store) => store.0.save(opened.revision, draft),
        }
    }

    fn preview(&self) -> bool {
        matches!(self, Self::Preview(_))
    }
}

#[derive(Default)]
struct Draft {
    opened: Option<Snapshot>,
    applications: BTreeMap<ApplicationIdentity, AppPolicy>,
    conflicted: bool,
}

struct State {
    source: Source,
    window: Cell<HWND>,
    owner: Cell<HWND>,
    busy: Cell<bool>,
    failed: Cell<bool>,
    font: Cell<HFONT>,
    scroll: Cell<i32>,
    laying_out: Cell<bool>,
    controls: RefCell<Vec<(HWND, Rect)>>,
    draft: RefCell<Draft>,
    readiness: RefCell<readiness::Readiness>,
}

#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    stretch: bool,
}

struct Busy<'a>(&'a Cell<bool>);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl State {
    fn new(source: Source) -> Rc<Self> {
        Rc::new(Self {
            source,
            window: Cell::new(HWND::default()),
            owner: Cell::new(HWND::default()),
            busy: Cell::new(false),
            failed: Cell::new(false),
            font: Cell::new(HFONT::default()),
            scroll: Cell::new(0),
            laying_out: Cell::new(false),
            controls: RefCell::new(Vec::new()),
            draft: RefCell::new(Draft::default()),
            readiness: RefCell::new(readiness::Readiness::default()),
        })
    }

    fn control(&self, id: u16) -> HWND {
        // SAFETY: children are created and used on the owning UI thread.
        unsafe { GetDlgItem(Some(self.window.get()), id as i32) }.unwrap_or_default()
    }

    fn status(&self, text: &str) {
        let text = if self.source.preview() {
            format!("Preview. {text}")
        } else {
            text.to_owned()
        };
        set_text(self.control(STATUS), &text);
    }

    fn reload(&self) {
        match self.source.load() {
            Ok(snapshot) => {
                self.restart_readiness(&snapshot);
                let value = snapshot.value.clone();
                *self.draft.borrow_mut() = Draft {
                    applications: value.applications.clone(),
                    opened: Some(snapshot),
                    conflicted: false,
                };
                checked(self.control(AUTOMATIC), value.automatic);
                checked(self.control(SOURCE), value.switch_source);
                for (choice, default, path) in [
                    (&value.dictionaries[0], EN_DEFAULT, EN_PATH),
                    (&value.dictionaries[1], RU_DEFAULT, RU_PATH),
                ] {
                    checked(
                        self.control(default),
                        matches!(choice, DictionaryPath::Default),
                    );
                    set_text(
                        self.control(path),
                        match choice {
                            DictionaryPath::Default => "",
                            DictionaryPath::Explicit(path) => path.as_str(),
                        },
                    );
                }
                set_text(self.control(IDENTITY), "");
                self.rows();
                self.dictionary_enabled();
                enable(self.control(SAVE), true);
                self.status("Loaded complete settings. Save applies the complete draft. Close discards unsaved edits.");
            }
            Err(_) => {
                self.readiness.borrow_mut().previous_settings = true;
                self.render_readiness();
                self.draft.borrow_mut().conflicted = true;
                enable(self.control(SAVE), false);
                self.status(
                    "Settings could not be loaded. The visible draft is retained. Reload to retry.",
                );
            }
        }
    }

    fn selected(&self) -> Option<(ApplicationIdentity, AppPolicy)> {
        let row = send(self.control(APPLICATIONS), LB_GETCURSEL, 0, 0);
        usize::try_from(row).ok().and_then(|row| {
            self.draft
                .borrow()
                .applications
                .iter()
                .nth(row)
                .map(|(identity, policy)| (identity.clone(), *policy))
        })
    }

    fn select(&self) {
        if let Some((identity, policy)) = self.selected() {
            send(
                self.control(IDENTITY_TYPE),
                CB_SETCURSEL,
                usize::from(LocalPath::new(identity.as_str()).is_err()),
                0,
            );
            set_text(self.control(IDENTITY), identity.as_str());
            send(self.control(POLICY), CB_SETCURSEL, policy_index(policy), 0);
        }
    }

    fn upsert(&self) {
        let selected = self.selected().map(|(identity, _)| identity);
        let result = (|| {
            let text = get_text(self.control(IDENTITY))?;
            let kind = send(self.control(IDENTITY_TYPE), CB_GETCURSEL, 0, 0);
            let policy = match send(self.control(POLICY), CB_GETCURSEL, 0, 0) {
                0 => AppPolicy::Automatic,
                1 => AppPolicy::Manual,
                2 => AppPolicy::Off,
                _ => return Err(SettingsError::Invalid),
            };
            let identity = native_identity(kind, &text)?;
            Ok((identity, policy))
        })();
        let Ok((identity, policy)) = result else {
            self.status("Application identity is invalid or unavailable. Choose an existing executable or enter an installed package family. The draft was retained.");
            return;
        };
        let mut draft = self.draft.borrow_mut();
        if selected
            .as_ref()
            .is_some_and(|selected| selected != &identity)
            && draft.applications.contains_key(&identity)
        {
            drop(draft);
            self.status("That application already has an override. Select its row to edit it, or choose a different identity. Both rows and the editor were retained.");
            return;
        }
        if selected.is_none()
            && draft.applications.len() >= 256
            && !draft.applications.contains_key(&identity)
        {
            drop(draft);
            self.status("The application draft already has 256 overrides. Remove an override before adding another.");
            return;
        }
        if let Some(selected) = selected {
            draft.applications.remove(&selected);
        }
        let canonical = identity.as_str().to_owned();
        draft.applications.insert(identity, policy);
        drop(draft);
        set_text(self.control(IDENTITY), &canonical);
        self.rows();
        self.status("Application draft updated. Save applies it with the other preferences.");
    }

    fn read_draft(&self) -> Result<Preferences, SettingsError> {
        let dictionary = |default, path| {
            if is_checked(self.control(default)) {
                Ok(DictionaryPath::Default)
            } else {
                Ok(DictionaryPath::Explicit(LocalPath::new(&get_text(
                    self.control(path),
                )?)?))
            }
        };
        Ok(Preferences {
            automatic: is_checked(self.control(AUTOMATIC)),
            switch_source: is_checked(self.control(SOURCE)),
            dictionaries: [
                dictionary(EN_DEFAULT, EN_PATH)?,
                dictionary(RU_DEFAULT, RU_PATH)?,
            ],
            applications: self.draft.borrow().applications.clone(),
        })
    }

    fn save(&self) {
        let opened = {
            let draft = self.draft.borrow();
            if draft.conflicted {
                None
            } else {
                draft.opened.clone()
            }
        };
        let Some(opened) = opened else {
            self.status("Reload current settings before saving. The visible draft is retained.");
            return;
        };
        let Ok(identity) = get_text(self.control(IDENTITY)) else {
            self.status(
                "Application identity could not be read. Correct the editor and retry Save.",
            );
            return;
        };
        let policy = send(self.control(POLICY), CB_GETCURSEL, 0, 0);
        let kind = send(self.control(IDENTITY_TYPE), CB_GETCURSEL, 0, 0);
        let selected = self.selected();
        let application_applied = identity.is_empty()
            || self.draft.borrow().applications.iter().any(|(key, value)| {
                selected
                    .as_ref()
                    .is_none_or(|(selected, _)| selected == key)
                    && key.as_str() == identity
                    && policy_index(*value) as isize == policy
                    && isize::from(LocalPath::new(key.as_str()).is_err()) == kind
            });
        if !application_applied {
            self.status("Apply the application editor with Add / update before Save, or clear its identity to discard that edit.");
            return;
        }
        let draft = match self.read_draft() {
            Ok(draft) => draft,
            Err(_) => {
                self.status("Dictionary paths must be absolute local file paths. Correct the complete draft and retry Save.");
                return;
            }
        };
        match self.source.save(&opened, draft) {
            SaveOutcome::Saved(saved) => {
                self.restart_readiness(&saved);
                self.draft.borrow_mut().opened = Some(saved);
                let status = if self.source.preview() {
                    "Draft saved in preview memory. User preferences were untouched."
                } else {
                    "Saved. The complete settings value matched fresh storage readback. Live adoption is not reported here."
                };
                self.status(status);
            }
            SaveOutcome::Conflict(_) => {
                self.readiness.borrow_mut().previous_settings = true;
                self.render_readiness();
                self.draft.borrow_mut().conflicted = true;
                enable(self.control(SAVE), false);
                self.status("Settings changed in another window. Your draft is retained. Reload replaces the draft with current settings before Save can proceed.");
            }
            SaveOutcome::Rejected(error) => {
                if error == SettingsError::Readback {
                    self.draft.borrow_mut().conflicted = true;
                    enable(self.control(SAVE), false);
                }
                let status = match error {
                    SettingsError::Invalid => {
                        "The complete draft failed validation. Correct it and retry Save."
                    }
                    SettingsError::Readback => {
                        "Save could not be verified. Reload current settings before saving again."
                    }
                    _ => {
                        "Settings storage is unavailable or its security was rejected. The draft is retained. Reload to retry."
                    }
                };
                self.status(status);
            }
        }
    }

    fn command(&self, id: u16, notification: u16) {
        if self.busy.replace(true) {
            return;
        }
        let _busy = Busy(&self.busy);
        match id {
            SAVE => self.save(),
            RELOAD => self.reload(),
            CLOSE => {
                // SAFETY: closing the owning thread's window releases its native state owner.
                unsafe {
                    let _ = DestroyWindow(self.window.get());
                }
            }
            EN_DEFAULT | RU_DEFAULT => self.dictionary_enabled(),
            EN_BROWSE | RU_BROWSE | EXE_BROWSE => {
                if let Some(path) = choose_file(self.window.get(), id == EXE_BROWSE) {
                    let target = match id {
                        EN_BROWSE => EN_PATH,
                        RU_BROWSE => RU_PATH,
                        _ => IDENTITY,
                    };
                    if id == EXE_BROWSE {
                        send(self.control(IDENTITY_TYPE), CB_SETCURSEL, 0, 0);
                    }
                    set_text(self.control(target), &path);
                }
            }
            APPLICATIONS if notification == LBN_SELCHANGE as u16 => self.select(),
            UPSERT => self.upsert(),
            NEW => {
                send(self.control(APPLICATIONS), LB_SETCURSEL, usize::MAX, 0);
                set_text(self.control(IDENTITY), "");
                send(self.control(IDENTITY_TYPE), CB_SETCURSEL, 0, 0);
                send(self.control(POLICY), CB_SETCURSEL, 1, 0);
                self.status("Enter a new application identity and policy, then use Add / update.");
            }
            REMOVE => {
                if let Some((identity, _)) = self.selected() {
                    self.draft.borrow_mut().applications.remove(&identity);
                    set_text(self.control(IDENTITY), "");
                    self.rows();
                    self.status("Application removed from the draft. Save applies the change.");
                }
            }
            _ => {}
        }
    }
}

fn policy_index(policy: AppPolicy) -> usize {
    match policy {
        AppPolicy::Automatic => 0,
        AppPolicy::Manual => 1,
        AppPolicy::Off => 2,
    }
}
fn policy_name(policy: AppPolicy) -> &'static str {
    match policy {
        AppPolicy::Automatic => "Automatic",
        AppPolicy::Manual => "Manual",
        AppPolicy::Off => "Off",
    }
}
