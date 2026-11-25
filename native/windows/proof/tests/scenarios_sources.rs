use crate::support::{
    client::Client,
    oracle,
    providers::{Diagnostic, Kind, Profile, Provider, source_profile},
};
use keyrill_windows_proof::{
    diagnostics::{self, ServiceSnapshot},
    source::{self, Failure, Probe, Status, TestHooks},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use windows::Win32::{
    Foundation::*,
    System::Threading::GetCurrentThreadId,
    UI::{Controls::RichEdit::*, TextServices::*, WindowsAndMessaging::*},
};
use windows_core::{HRESULT, IUnknown, Interface};

type ProbeHook = Rc<RefCell<Option<Box<dyn Fn(&Probe)>>>>;
type ClockHook = Rc<RefCell<Option<Box<dyn Fn()>>>>;
struct Fixture {
    probe: Rc<Probe>,
    c: Client,
    diagnostic: Rc<Diagnostic>,
    native: Rc<Profile>,
    word: Rc<Cell<Result<u32, Failure>>>,
    clock: Rc<Cell<u64>>,
    clock_calls: Rc<Cell<u32>>,
    read_hook: ProbeHook,
    acquire_hook: ProbeHook,
    clock_hook: ClockHook,
    acquire_result: Rc<Cell<HRESULT>>,
}
impl Fixture {
    fn new() -> Self {
        let c = Client::new();
        let diagnostic = Rc::new(Diagnostic::default());
        {
            let mut snapshot = diagnostic.snapshot.borrow_mut();
            snapshot[0] = 3;
            snapshot[1] = 1;
            snapshot[3] = 1;
        }
        let diagnostic_interface = unsafe {
            IUnknown::from_raw(Provider::create(
                Rc::clone(&c.host),
                Kind::Diagnostic(Rc::clone(&diagnostic)),
                IUnknown::IID,
            ))
        };
        *c.manager.diagnostic.borrow_mut() = Some(diagnostic_interface);
        let native = Rc::new(Profile::default());
        let native_interface = unsafe {
            IUnknown::from_raw(Provider::create(
                Rc::clone(&c.host),
                Kind::Profiles(Rc::clone(&native)),
                IUnknown::IID,
            ))
        };
        let word = Rc::new(Cell::new(Ok(0)));
        let clock = Rc::new(Cell::new(0));
        let clock_calls = Rc::new(Cell::new(0));
        let read_hook: ProbeHook = Rc::new(RefCell::new(None));
        let acquire_hook: ProbeHook = Rc::new(RefCell::new(None));
        let clock_hook: ClockHook = Rc::new(RefCell::new(None));
        let acquire_result = Rc::new(Cell::new(S_OK));
        let probe = Rc::new(Probe::with_hooks(
            HWND::default(),
            c.thread.clone(),
            unsafe { GetCurrentThreadId() },
            TestHooks {
                acquire: {
                    let hook = Rc::clone(&acquire_hook);
                    let result = Rc::clone(&acquire_result);
                    Box::new(move |probe| {
                        if let Some(hook) = hook.borrow().as_ref() {
                            hook(probe);
                        }
                        if result.get() != S_OK {
                            return Err(result.get());
                        }
                        Ok((
                            native_interface.cast().unwrap(),
                            native_interface.cast().unwrap(),
                        ))
                    })
                },
                read: {
                    let word = Rc::clone(&word);
                    let hook = Rc::clone(&read_hook);
                    Box::new(move |probe| {
                        if let Some(hook) = hook.borrow().as_ref() {
                            hook(probe);
                        }
                        word.get()
                    })
                },
                clock: {
                    let value = Rc::clone(&clock);
                    let calls = Rc::clone(&clock_calls);
                    let hook = Rc::clone(&clock_hook);
                    Box::new(move |now| {
                        calls.set(calls.get() + 1);
                        if let Some(hook) = hook.borrow().as_ref() {
                            hook();
                        }
                        if value.get() == 0 { now } else { value.get() }
                    })
                },
            },
        ));
        Self {
            probe,
            c,
            diagnostic,
            native,
            word,
            clock,
            clock_calls,
            read_hook,
            acquire_hook,
            clock_hook,
            acquire_result,
        }
    }
    fn bind(&self) {
        assert_eq!(self.probe.tick(0).0[0], 0);
        assert_eq!(self.diagnostic.reads.get(), 1);
    }
    fn word(&self, length: u32) {
        if self.diagnostic.snapshot.borrow()[56] == 0 {
            {
                let mut s = self.diagnostic.snapshot.borrow_mut();
                s[56] = 1;
                s[59] = 1;
                s[60] = 1;
            }
            record(&self.diagnostic, 1, 1, 0, 0);
        }
        self.word.set(Ok(length));
        self.diagnostic.snapshot.borrow_mut()[13] += 1;
    }
    fn arm(&self) {
        self.bind();
        self.word(6);
        assert_eq!(self.probe.tick(100).0[0], 1);
    }
    fn complete_at(&self, boundary: &'static str) {
        let diagnostic = Rc::clone(&self.diagnostic);
        let native = Rc::clone(&self.native);
        self.c
            .host
            .on(boundary, 0, move || complete(&diagnostic, &native, true));
    }
    fn snapshot(&self) -> ServiceSnapshot {
        snapshot(&self.diagnostic)
    }
    fn no_mutations(&self) {
        assert_eq!(self.native.language_calls.get(), 0);
        assert_eq!(self.native.activation_calls.get(), 0);
        oracle::document(&self.c, "", 7);
        oracle::no_effects(&self.c);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.probe.close();
        self.c.manager.diagnostic.borrow_mut().take();
    }
}
fn snapshot(d: &Diagnostic) -> ServiceSnapshot {
    let s = d.snapshot.borrow();
    ServiceSnapshot {
        base: s[..55].try_into().unwrap(),
        lifecycle: s[55..].try_into().unwrap(),
    }
}
fn record(d: &Diagnostic, kind: u32, subject: u32, cause: u32, unresolved: u32) {
    let mut s = d.snapshot.borrow_mut();
    let life = &mut s[55..];
    life[0] += 1;
    let count = life[3] as usize;
    if count == 16 {
        life.copy_within(11..86, 6);
    } else {
        life[3] += 1;
    }
    let start = 6 + life[3] as usize * 5 - 5;
    let sequence = life[0];
    life[start..start + 5].copy_from_slice(&[sequence, kind, subject, cause, unresolved]);
}
fn complete(d: &Diagnostic, native: &Profile, profile_event: bool) {
    record(d, 4, 1, 0, 0);
    if profile_event {
        record(d, 8, 0, 6, 0);
    }
    {
        let mut s = d.snapshot.borrow_mut();
        s[59] = 0;
        s[60] = 0;
        s[3] = 2;
    }
    native.profile.set(source_profile(1));
}

#[test]
fn source_probe_success_status_and_publication_contract() {
    for redundant in [false, true] {
        let f = Fixture::new();
        assert_eq!(f.diagnostic.reads.get(), 0);
        f.arm();
        f.complete_at(if redundant {
            "language.change"
        } else {
            "profile.activate"
        });
        assert_eq!(f.probe.tick(2099).0[0], 1);
        assert_eq!(f.native.language_calls.get(), 0);
        f.c.host.trace.borrow_mut().clear();
        let status = f.probe.tick(2100);
        assert_eq!(status.0[0], 3, "{status:?}");
        assert_eq!(
            (
                status.0[2],
                status.0[3],
                status.0[6],
                status.0[12],
                status.0[13],
                status.0[14]
            ),
            (
                1,
                u32::from(!redundant),
                1,
                1,
                if redundant { 1 } else { 2 },
                1
            )
        );
        let trace = f.c.host.trace.borrow();
        let source_calls: Vec<_> = trace
            .iter()
            .copied()
            .filter(|name| {
                matches!(
                    *name,
                    "copy"
                        | "language.read"
                        | "profile.read"
                        | "target.read"
                        | "enabled.read"
                        | "language.change"
                        | "profile.activate"
                        | "identity"
                        | "function"
                        | "release"
                        | "native.release"
                        | "GetFunctionProvider"
                )
            })
            .collect();
        for (index, name) in source_calls.iter().enumerate() {
            if matches!(*name, "language.change" | "profile.activate") {
                assert_eq!(
                    source_calls[index - 1],
                    "copy",
                    "source mutation must immediately follow actual diagnostic Copy"
                );
            }
        }
        drop(trace);
        f.probe.close();
        assert_eq!(f.probe.tick(5000), status);
        let text = status.format().unwrap();
        assert_eq!(Status::parse(&text), Some(status.clone()));
        for invalid in [
            format!("{text}; extra=1"),
            format!("unknown{text}"),
            text[..text.len() - 1].to_string(),
        ] {
            assert!(Status::parse(&invalid).is_none());
        }
        for scenario in 0..10 {
            let mut bad = status.clone();
            match scenario {
                0 => bad.0[2] = 0,
                1 => bad.0[3] = 2,
                2 => bad.0[6] = 0,
                3 => bad.0[1] = Failure::Closed as u32,
                4 => bad.0[10] = bad.0[7],
                5 => bad.0[9] = bad.0[7] + 17,
                6 => bad.0[12] = 0,
                7 => bad.0[14] = 0,
                8 => bad.0[4] = E_FAIL.0 as u32,
                _ => bad.0[11] = bad.0[10],
            }
            assert!(bad.format().is_none(), "invalid success {scenario}");
        }
        let first = diagnostics::Publication {
            publication: 1,
            source: 1,
            availability: 4,
            continuity: 2,
            snapshot: f.snapshot(),
            ..Default::default()
        };
        let mut second = first.clone();
        second.publication = 2;
        assert!(source::matches(&status, &first, &second));
        second.snapshot.base[3] = 1;
        assert!(!source::matches(&status, &first, &second));
        assert!(!source::matches(&status, &first, &first));
        oracle::document(&f.c, "", 7);
    }
}

#[test]
fn source_probe_word_readiness_and_armed_interference_matrix() {
    for scenario in 0..12 {
        let f = Fixture::new();
        f.bind();
        if scenario == 0 {
            f.word.set(Err(Failure::FocusChanged));
            record(&f.diagnostic, 8, 0, 1, 0);
            assert_eq!(f.probe.tick(20).0[0], 0);
            f.word.set(Ok(0));
            record(&f.diagnostic, 8, 0, 1, 0);
            assert_eq!(f.probe.tick(40).0[0], 0);
            f.word(6);
            f.complete_at("profile.activate");
            assert_eq!(f.probe.tick(100).0[0], 1);
            assert_eq!(f.probe.tick(2100).0[0], 3);
            continue;
        }
        f.word(if scenario == 1 { 1 } else { 6 });
        match scenario {
            1 => {
                assert_eq!(f.probe.tick(100).0[0], 0);
                assert_eq!(f.probe.tick(5000).0[0], 0);
                f.word(6);
            }
            2 => f.word.set(Err(Failure::WordChanged)),
            3 => f.word.set(Err(Failure::SetupUnavailable)),
            4 => f.word.set(Err(Failure::FocusChanged)),
            5 => {
                complete(&f.diagnostic, &f.native, true);
                f.word.set(Ok(0));
            }
            6 => record(&f.diagnostic, 8, 0, 1, 0),
            7 => f.diagnostic.snapshot.borrow_mut()[0] += 1,
            8 => f.diagnostic.snapshot.borrow_mut()[20] += 1,
            9 => f.diagnostic.snapshot.borrow_mut()[4] = 1,
            10 => f.diagnostic.result.set(diagnostics::E_PENDING),
            11 => f.diagnostic.snapshot.borrow_mut()[3] = 2,
            _ => {}
        }
        let status = f.probe.tick(100);
        assert_eq!(
            status.0[0],
            if scenario == 1 { 1 } else { 4 },
            "readiness {scenario}: {status:?}"
        );
        if scenario != 1 {
            assert_eq!(f.probe.tick(3000), status);
        }
        f.no_mutations();
    }
    for scenario in 0..14 {
        let f = Fixture::new();
        f.arm();
        match scenario {
            0..=4 => f.diagnostic.snapshot.borrow_mut()[9 + scenario] += 1,
            5 => record(&f.diagnostic, 8, 0, 1, 0),
            6 => f.native.enabled.set(false.into()),
            7 => {
                f.native.results.borrow_mut().insert("GetProfile", S_FALSE);
            }
            8 => f.diagnostic.result.set(diagnostics::E_PENDING),
            9 => {
                let replacement = Rc::new(Diagnostic::default());
                let interface = unsafe {
                    IUnknown::from_raw(Provider::create(
                        Rc::clone(&f.c.host),
                        Kind::Diagnostic(replacement),
                        IUnknown::IID,
                    ))
                };
                *f.c.manager.diagnostic.borrow_mut() = Some(interface);
            }
            10 => f.word.set(Ok(5)),
            11 => f.complete_at("copy"),
            12 => f.complete_at("enabled.read"),
            _ => {}
        }
        let status = f.probe.tick(if scenario == 13 { 2351 } else { 2100 });
        assert_eq!(status.0[0], 4, "armed {scenario}: {status:?}");
        f.no_mutations();
    }
}

#[test]
fn source_probe_language_and_activation_failure_matrix() {
    for scenario in 0..15 {
        let f = Fixture::new();
        f.arm();
        let native = Rc::clone(&f.native);
        let diagnostic = Rc::clone(&f.diagnostic);
        let manager = Rc::clone(&f.c.manager);
        let host = Rc::clone(&f.c.host);
        f.c.host.on("language.change", 0, move || match scenario {
            0 | 2 => {
                native
                    .results
                    .borrow_mut()
                    .insert("ChangeCurrentLanguage", E_FAIL);
            }
            1 => {
                native
                    .results
                    .borrow_mut()
                    .insert("ChangeCurrentLanguage", S_FALSE);
            }
            3 => native.language.set(0x409),
            4 => {
                let other = Rc::new(Diagnostic::default());
                let interface = unsafe {
                    IUnknown::from_raw(Provider::create(
                        Rc::clone(&host),
                        Kind::Diagnostic(other),
                        IUnknown::IID,
                    ))
                };
                *manager.diagnostic.borrow_mut() = Some(interface);
            }
            5 => diagnostic.snapshot.borrow_mut()[0] += 1,
            6 => diagnostic.snapshot.borrow_mut()[1] = 0,
            7 => diagnostic.snapshot.borrow_mut()[2] = 1,
            8 => diagnostic.result.set(diagnostics::E_PENDING),
            9 => {
                complete(&diagnostic, &native, true);
                diagnostic.snapshot.borrow_mut()[13] += 1;
            }
            10 => {
                let mut s = diagnostic.snapshot.borrow_mut();
                s[59] = 0;
                s[60] = 0;
            }
            11 => {
                native.profile.set(source_profile(1));
                diagnostic.snapshot.borrow_mut()[3] = 2;
            }
            12 => record(&diagnostic, 9, 1, 0, 0),
            13 => record(&diagnostic, 7, 1, 0, 1),
            _ => diagnostic.snapshot.borrow_mut()[20] += 1,
        });
        let status = f.probe.tick(2100);
        assert_eq!(status.0[0], 4, "language {scenario}: {status:?}");
        assert_eq!(f.native.language_calls.get(), 1);
        assert_eq!(f.native.activation_calls.get(), 0);
        assert_eq!(f.probe.tick(9999), status);
        oracle::document(&f.c, "", 7);
        oracle::no_effects(&f.c);
    }
    for scenario in 0..8 {
        let f = Fixture::new();
        f.arm();
        let native = Rc::clone(&f.native);
        let diagnostic = Rc::clone(&f.diagnostic);
        let word = Rc::clone(&f.word);
        f.c.host.on("profile.activate", 0, move || {
            complete(&diagnostic, &native, true);
            match scenario {
                0 => {
                    native
                        .results
                        .borrow_mut()
                        .insert("ActivateProfile", E_FAIL);
                }
                1 => {
                    native
                        .results
                        .borrow_mut()
                        .insert("ActivateProfile", S_FALSE);
                }
                2 => native.profile.set(source_profile(0)),
                3 => {
                    let mut p = native.profile.get();
                    p.guidProfile = windows_core::GUID::zeroed();
                    native.profile.set(p);
                }
                4 => {
                    native
                        .results
                        .borrow_mut()
                        .insert("GetActiveProfile", S_FALSE);
                }
                5 => diagnostic.snapshot.borrow_mut()[3] = 1,
                6 => diagnostic.snapshot.borrow_mut()[11] += 1,
                _ => word.set(Err(Failure::FocusChanged)),
            }
        });
        let status = f.probe.tick(2100);
        assert_eq!(status.0[0], 4, "activation {scenario}: {status:?}");
        assert_eq!((status.0[2], status.0[3]), (1, 1));
        assert_eq!(f.probe.tick(9999), status);
        oracle::document(&f.c, "", 7);
        oracle::no_effects(&f.c);
    }
}

#[test]
fn source_readiness_public_word_exact_utf16_matrix() {
    for scenario in 0..14 {
        let a: Vec<u16> = match scenario {
            1 => "g",
            2 => "",
            3 => "ghbdtx",
            4 => "ghbdtn ",
            6 => "ghbdtn\r",
            _ => "ghbdtn",
        }
        .encode_utf16()
        .collect();
        let b: Vec<u16> = if scenario == 5 { "x" } else { "" }
            .encode_utf16()
            .collect();
        let alive = Cell::new(true);
        let reads = Cell::new(0);
        let eligibility = Cell::new(0);
        let caret = a.len() as i32 - i32::from(scenario == 7);
        let result = source::read_public_word(
            || alive.get(),
            || {
                eligibility.set(eligibility.get() + 1);
                if scenario == 8 {
                    Err(Failure::SetupUnavailable)
                } else if scenario == 9 {
                    Err(Failure::FocusChanged)
                } else {
                    Ok(())
                }
            },
            |id, message, count, destination| {
                assert!(id == 101 || id == 102);
                reads.set(reads.get() + 1);
                let text = if id == 101 { &a } else { &b };
                if scenario == 10 {
                    alive.set(false);
                }
                if message == WM_GETTEXTLENGTH {
                    return text.len() as isize;
                }
                if message == WM_GETTEXT {
                    assert_eq!(count.0, 8);
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            text.as_ptr(),
                            destination.0 as *mut u16,
                            text.len(),
                        );
                        (destination.0 as *mut u16).add(text.len()).write(0);
                    }
                    return text.len() as isize - isize::from(scenario == 11);
                }
                assert_eq!((id, message), (101, EM_EXGETSEL));
                unsafe {
                    (destination.0 as *mut CHARRANGE).write(CHARRANGE {
                        cpMin: caret,
                        cpMax: caret - i32::from(scenario == 12),
                    });
                }
                if scenario == 13 {
                    alive.set(false);
                }
                0
            },
        );
        assert_eq!(
            result.is_ok(),
            scenario <= 2,
            "public readiness {scenario}: {result:?}"
        );
        if scenario <= 2 {
            assert_eq!(result, Ok(a.len() as u32));
        }
        if scenario == 8 || scenario == 9 {
            assert_eq!(reads.get(), 0);
            assert_eq!(eligibility.get(), 1);
        }
        if scenario == 10 {
            assert_eq!(reads.get(), 1);
        }
    }
}

#[test]
fn source_readiness_callback_and_initial_setup_matrix() {
    for boundary in ["target.read", "enabled.read", "GetFunctionProvider"] {
        let f = Fixture::new();
        f.arm();
        let before = f.snapshot().base;
        let word = Rc::clone(&f.word);
        f.c.host
            .on(boundary, 0, move || word.set(Err(Failure::WordChanged)));
        assert_eq!(f.probe.tick(2100).0[1], Failure::WordChanged as u32);
        assert_eq!(f.snapshot().base, before);
        f.no_mutations();
    }
    for last in [false, true] {
        let f = Fixture::new();
        f.arm();
        f.complete_at("profile.activate");
        let word = Rc::clone(&f.word);
        f.c.host
            .on("GetFunctionProvider", if last { 2 } else { 1 }, move || {
                word.set(Err(Failure::WordChanged))
            });
        let result = f.probe.tick(2100);
        assert_eq!(result.0[1], Failure::WordChanged as u32);
        assert_eq!(f.native.language_calls.get(), 1);
        assert_eq!(f.native.activation_calls.get(), u32::from(last));
        if !last {
            f.c.host
                .actions
                .borrow_mut()
                .retain(|action| action.boundary != "profile.activate");
        }
    }
    for scenario in 0..5 {
        let f = Fixture::new();
        match scenario {
            0 => {
                f.native
                    .results
                    .borrow_mut()
                    .insert("GetActiveProfile", S_FALSE);
            }
            1 => f.native.profile.set(source_profile(1)),
            2 => {
                f.c.manager
                    .results
                    .borrow_mut()
                    .insert("GetFunctionProvider", TF_E_NOPROVIDER);
            }
            3 => f.word.set(Err(Failure::FocusChanged)),
            _ => f.acquire_result.set(E_NOINTERFACE),
        }
        assert_eq!(
            f.probe.tick(0).0[0],
            if scenario == 4 { 4 } else { 0 },
            "initial {scenario}"
        );
        f.no_mutations();
    }
}

#[test]
fn source_review_deadline_clock_and_read_eligibility_matrix() {
    for boundary in ["target.read", "GetFunctionProvider", "read", "copy"] {
        let f = Fixture::new();
        f.arm();
        let clock = Rc::clone(&f.clock);
        if boundary == "read" {
            *f.read_hook.borrow_mut() = Some(Box::new(move |_| clock.set(2351)));
        } else {
            f.c.host.on(boundary, 0, move || clock.set(2351));
        }
        let result = f.probe.tick(2100);
        assert_eq!(result.0[1], Failure::Late as u32);
        assert_eq!(f.clock_calls.get(), 1);
        assert_eq!(f.probe.tick(4000), result);
        f.no_mutations();
    }
    for scenario in 0..4 {
        let f = Fixture::new();
        f.arm();
        if scenario == 0 {
            f.clock.set(2350);
        }
        if scenario == 1 {
            f.clock.set(2099);
        }
        if scenario >= 2 {
            let probe = Rc::downgrade(&f.probe);
            *f.clock_hook.borrow_mut() = Some(Box::new(move || {
                let probe = probe.upgrade().unwrap();
                if scenario == 2 {
                    probe.close();
                } else {
                    assert_eq!(probe.tick(2100).0[0], 2);
                }
            }));
        }
        if scenario == 0 || scenario == 3 {
            f.complete_at("profile.activate");
        }
        let status = f.probe.tick(2100);
        assert_eq!(f.clock_calls.get(), 1);
        if scenario == 1 || scenario == 2 {
            assert_eq!(
                status.0[1],
                if scenario == 1 {
                    Failure::Late
                } else {
                    Failure::Closed
                } as u32
            );
            f.no_mutations();
        } else {
            assert_eq!(status.0[0], 3);
            assert_eq!(
                (
                    f.native.language_calls.get(),
                    f.native.activation_calls.get()
                ),
                (1, 1)
            );
        }
    }
    for change in ["password", "read-only", "disabled", "focus-lost"] {
        let changed = Cell::new(false);
        let text_queries = Cell::new(0);
        let selections = Cell::new(0);
        let result = source::read_public_word(
            || true,
            || {
                if !changed.get() {
                    Ok(())
                } else {
                    Err(if change == "focus-lost" {
                        Failure::FocusChanged
                    } else {
                        Failure::SetupUnavailable
                    })
                }
            },
            |id, message, count, out| {
                assert_eq!(id, 101);
                if message == WM_GETTEXTLENGTH {
                    text_queries.set(text_queries.get() + 1);
                    return 6;
                }
                if message == WM_GETTEXT {
                    text_queries.set(text_queries.get() + 1);
                    assert_eq!(count.0, 8);
                    let text: Vec<u16> = "ghbdtn\0".encode_utf16().collect();
                    unsafe {
                        std::ptr::copy_nonoverlapping(text.as_ptr(), out.0 as *mut u16, text.len());
                    }
                    changed.set(true);
                    return 6;
                }
                assert_eq!(message, EM_EXGETSEL);
                selections.set(selections.get() + 1);
                0
            },
        );
        assert_eq!(
            result,
            Err(if change == "focus-lost" {
                Failure::FocusChanged
            } else {
                Failure::SetupUnavailable
            })
        );
        assert_eq!((text_queries.get(), selections.get()), (2, 0));
    }
}

#[test]
fn source_history_complete_interval_and_unsafe_history_matrix() {
    for scenario in 0..17 {
        let f = Fixture::new();
        f.arm();
        let before = f.snapshot();
        let mut middle = before.clone();
        match scenario {
            0 | 1 => {
                for _ in 0..8 {
                    record(&f.diagnostic, 5, 1, 0, 0);
                }
                middle = f.snapshot();
                for _ in 0..if scenario == 0 { 7 } else { 8 } {
                    record(&f.diagnostic, 5, 1, 0, 0);
                }
                complete(&f.diagnostic, &f.native, false);
            }
            2 => {
                record(&f.diagnostic, 2, 1, 0, 0);
                record(&f.diagnostic, 3, 1, 0, 0);
                let mut s = f.diagnostic.snapshot.borrow_mut();
                s[59] = 0;
                s[60] = 0;
                s[3] = 2;
            }
            3 => {
                record(&f.diagnostic, 3, 1, 0, 0);
                let mut s = f.diagnostic.snapshot.borrow_mut();
                s[59] = 0;
                s[60] = 0;
                s[3] = 2;
            }
            _ => {
                complete(&f.diagnostic, &f.native, true);
                match scenario {
                    4 => f.diagnostic.snapshot.borrow_mut()[62] = 2,
                    5 => f.diagnostic.snapshot.borrow_mut()[20] += 1,
                    6 => f.diagnostic.snapshot.borrow_mut()[57] = 1,
                    7 => record(&f.diagnostic, 9, 1, 0, 0),
                    8 => record(&f.diagnostic, 7, 1, 0, 3),
                    9 => record(&f.diagnostic, 8, 0, 8, 0),
                    10 => {
                        f.diagnostic.snapshot.borrow_mut()[56] += 1;
                        record(&f.diagnostic, 1, 2, 0, 0);
                    }
                    11..=15 => f.diagnostic.snapshot.borrow_mut()[9 + scenario - 11] += 1,
                    _ => f.diagnostic.snapshot.borrow_mut()[13] -= 1,
                }
            }
        }
        let mut status = Status::default();
        let result = source::evaluate(&before, &middle, &f.snapshot(), &mut status);
        assert_eq!(
            result.is_ok(),
            scenario == 0 || scenario == 2,
            "history {scenario}: {result:?}"
        );
        if scenario == 1 {
            assert_eq!(result, Err(Failure::HistoryLost));
        }
        if scenario == 2 {
            assert_eq!(status.0[12], 2);
        }
        f.no_mutations();
    }
}

#[test]
fn source_reentry_running_close_and_preparation_matrix() {
    for close in [false, true] {
        for boundary in [
            "read",
            "GetFunctionProvider",
            "identity",
            "copy",
            "release",
            "language.read",
            "profile.read",
            "target.read",
            "enabled.read",
            "language.change",
            "profile.activate",
            "native.release",
        ] {
            let f = Fixture::new();
            f.arm();
            let probe = Rc::downgrade(&f.probe);
            let observed = Rc::new(Cell::new(None));
            let observed_callback = Rc::clone(&observed);
            let diagnostic = Rc::clone(&f.diagnostic);
            let native = Rc::clone(&f.native);
            let run = move || {
                let probe = probe.upgrade().unwrap();
                if close {
                    probe.close();
                } else {
                    observed_callback.set(Some(probe.tick(2100).0[0]));
                }
                if boundary == "profile.activate" {
                    complete(&diagnostic, &native, true);
                }
            };
            if boundary == "read" {
                *f.read_hook.borrow_mut() = Some(Box::new(move |_| run()));
            } else if boundary == "release" || boundary == "native.release" {
                let host = Rc::clone(&f.c.host);
                let run = Rc::new(run);
                f.c.host.on("language.change", 0, move || {
                    let run = Rc::clone(&run);
                    host.on(boundary, 0, move || run());
                });
            } else {
                f.c.host.on(boundary, 0, run);
            }
            if boundary == "release" || boundary == "native.release" {
                f.native
                    .results
                    .borrow_mut()
                    .insert("ChangeCurrentLanguage", E_FAIL);
            } else if boundary != "profile.activate" {
                f.complete_at("profile.activate");
            }
            let status = f.probe.tick(2100);
            if !close {
                assert_eq!(
                    observed.get(),
                    Some(if boundary == "release" || boundary == "native.release" {
                        4
                    } else {
                        2
                    }),
                    "reentrant {boundary}"
                );
            }
            assert_eq!(
                status.0[0],
                if close || boundary == "release" || boundary == "native.release" {
                    4
                } else {
                    3
                },
                "{boundary}/{close}: {status:?}"
            );
            if close && boundary != "release" && boundary != "native.release" {
                assert_eq!(status.0[1], Failure::Closed as u32);
            }
            let pending = std::mem::take(&mut *f.c.host.actions.borrow_mut());
            let cancelled_activation =
                close && !matches!(boundary, "profile.activate" | "release" | "native.release");
            assert_eq!(
                pending.len(),
                usize::from(cancelled_activation),
                "{boundary}/{close}"
            );
            for action in &pending {
                assert_eq!(action.boundary, "profile.activate");
            }
            drop(pending);
            f.c.host.drained();
            oracle::document(&f.c, "", 7);
            oracle::no_effects(&f.c);
        }
    }
    for boundary in [
        "acquire",
        "read",
        "GetFunctionProvider",
        "identity",
        "function",
        "copy",
        "release",
    ] {
        let f = Fixture::new();
        if boundary == "acquire" {
            *f.acquire_hook.borrow_mut() = Some(Box::new(Probe::close));
        } else if boundary == "read" {
            *f.read_hook.borrow_mut() = Some(Box::new(Probe::close));
        } else {
            let probe = Rc::downgrade(&f.probe);
            f.c.host
                .on(boundary, 0, move || probe.upgrade().unwrap().close());
        }
        let status = f.probe.tick(0);
        assert_eq!(
            (status.0[0], status.0[1]),
            (4, Failure::Closed as u32),
            "prepare {boundary}"
        );
        assert_eq!(f.probe.tick(3000), status);
        f.no_mutations();
    }
}

#[test]
fn source_service_exact_termination_and_interfering_finish() {
    for input in [false, true] {
        let c = Rc::new(Client::new());
        let native = Rc::new(Profile::default());
        let native_interface = unsafe {
            IUnknown::from_raw(Provider::create(
                Rc::clone(&c.host),
                Kind::Profiles(Rc::clone(&native)),
                IUnknown::IID,
            ))
        };
        let word = Rc::new(Cell::new(0));
        let word_reader = Rc::clone(&word);
        let probe = Probe::with_hooks(
            HWND::default(),
            c.thread.clone(),
            unsafe { GetCurrentThreadId() },
            TestHooks {
                acquire: Box::new(move |_| {
                    Ok((
                        native_interface.cast().unwrap(),
                        native_interface.cast().unwrap(),
                    ))
                }),
                read: Box::new(move |_| Ok(word_reader.get())),
                clock: Box::new(|now| now),
            },
        );
        assert_eq!(probe.tick(0).0[0], 0);
        c.type_text("ghbdtn");
        word.set(6);
        assert_eq!(probe.tick(100).0[0], 1);
        let before = (
            c.a.writes.get(),
            c.a.reads.get(),
            c.a.starts.get(),
            c.a.ends.get(),
        );
        let weak = Rc::downgrade(&c);
        let native_state = Rc::clone(&native);
        c.host.on("language.change", 0, move || {
            let c = weak.upgrade().unwrap();
            if input {
                let mut finish = crate::support::dll::Event::command(6);
                finish.context = 0;
                c.event(finish);
            } else {
                let composition = c.a.active.borrow().as_ref().unwrap().pointer;
                let sink: ITfCompositionSink = c.service.cast().unwrap();
                unsafe {
                    assert_eq!(
                        (sink.vtable().OnCompositionTerminated)(sink.as_raw(), 123, composition),
                        S_OK
                    );
                }
            }
            native_state.profile.set(source_profile(1));
            let profile = source_profile(1);
            let sink: ITfInputProcessorProfileActivationSink = c.service.cast().unwrap();
            unsafe {
                assert_eq!(
                    (sink.vtable().OnActivated)(
                        sink.as_raw(),
                        profile.dwProfileType,
                        profile.langid,
                        &profile.clsid,
                        &profile.catid,
                        &profile.guidProfile,
                        profile.hkl,
                        TF_IPSINK_FLAG_ACTIVE
                    ),
                    S_OK
                );
            }
        });
        let result = probe.tick(2100);
        if input {
            assert_eq!(result.0[1], Failure::InputInterference as u32, "{result:?}");
        } else {
            assert_eq!(result.0[0], 3, "{result:?}");
        }
        assert_eq!(c.a.writes.get(), before.0);
        assert_eq!(c.a.starts.get(), before.2);
        assert_eq!(c.a.ends.get(), before.3 + u32::from(input));
        if !input {
            assert_eq!(c.a.reads.get(), before.1);
        }
        assert_eq!(
            (native.language_calls.get(), native.activation_calls.get()),
            (1, 0)
        );
        oracle::document(&c, "ghbdtn", 13);
        probe.close();
    }
}

#[test]
fn source_probe_dependency_lifetimes_drain_after_every_terminal_path() {
    for scenario in 0..4 {
        let f = Fixture::new();
        let host = Rc::clone(&f.c.host);
        if scenario > 0 {
            f.arm();
        }
        if scenario == 2 {
            f.native
                .results
                .borrow_mut()
                .insert("ChangeCurrentLanguage", E_FAIL);
            assert_eq!(f.probe.tick(2100).0[0], 4);
        }
        if scenario == 3 {
            f.complete_at("profile.activate");
            assert_eq!(f.probe.tick(2100).0[0], 3);
        }
        drop(f);
        host.drained();
        assert_eq!(host.live_objects.get(), 0);
        assert_eq!(
            host.dependency_retains.get(),
            host.dependency_releases.get()
        );
    }
}

#[test]
fn source_review_cross_thread_public_b_refuses_before_text_queries() {
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
    use windows_core::w;
    unsafe extern "system" fn default_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }
    unsafe {
        LoadLibraryW(w!("Msftedit.dll")).unwrap();
        let instance = GetModuleHandleW(None).unwrap();
        let class = WNDCLASSW {
            lpfnWndProc: Some(default_proc),
            hInstance: instance.into(),
            lpszClassName: w!("KeyrillProofEditor"),
            ..Default::default()
        };
        let atom = RegisterClassW(&class);
        assert!(atom != 0 || GetLastError() == ERROR_CLASS_ALREADY_EXISTS);
        let root = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("KeyrillProofEditor"),
            w!(""),
            WS_OVERLAPPED,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        let a = CreateWindowExW(
            WS_EX_NOPARENTNOTIFY,
            w!("RICHEDIT50W"),
            w!(""),
            WS_CHILD | WINDOW_STYLE(ES_MULTILINE as u32),
            0,
            0,
            10,
            10,
            Some(root),
            Some(HMENU(101usize as *mut _)),
            Some(instance.into()),
            None,
        )
        .unwrap();
        assert_eq!(GetDlgItem(Some(root), 101).unwrap(), a);
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let instance = GetModuleHandleW(None).unwrap();
            let worker_root = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WS_OVERLAPPED,
                0,
                0,
                10,
                10,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .unwrap();
            let b = CreateWindowExW(
                WS_EX_NOPARENTNOTIFY,
                w!("RICHEDIT50W"),
                w!(""),
                WS_CHILD | WINDOW_STYLE(ES_MULTILINE as u32),
                0,
                0,
                10,
                10,
                Some(worker_root),
                Some(HMENU(102usize as *mut _)),
                Some(instance.into()),
                None,
            )
            .unwrap();
            let mut message = MSG::default();
            let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            send.send((worker_root.0 as usize, b.0 as usize, GetCurrentThreadId()))
                .unwrap();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                DispatchMessageW(&message);
            }
            DestroyWindow(worker_root).unwrap();
        });
        let (worker_root, b, thread) = receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let b = HWND(b as *mut _);
        let worker_root = HWND(worker_root as *mut _);
        assert_eq!(SetParent(b, Some(root)).unwrap(), worker_root);
        assert_eq!(GetDlgItem(Some(root), 102).unwrap(), b);
        assert_ne!(thread, GetCurrentThreadId());
        assert_eq!(GetWindowThreadProcessId(b, None), thread);
        let reads = Cell::new(0);
        let result = source::read_public_word(
            || true,
            || source::public_controls(root, GetCurrentThreadId()),
            |_, _, _, _| {
                reads.set(reads.get() + 1);
                0
            },
        );
        assert_eq!(SetParent(b, Some(worker_root)).unwrap(), root);
        PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)).unwrap();
        worker.join().unwrap();
        DestroyWindow(root).unwrap();
        assert_eq!(result, Err(Failure::SetupUnavailable));
        assert_eq!(reads.get(), 0);
    }
}
