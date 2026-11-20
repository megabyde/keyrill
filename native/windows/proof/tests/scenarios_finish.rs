use crate::support::{
    client::Client,
    dll::Event,
    document::{Effect, Span},
    oracle,
};
use std::rc::Rc;
use windows::Win32::{Foundation::*, UI::TextServices::*};
fn corrected() -> Client {
    let c = Client::new();
    c.type_text("ghbdtn ");
    oracle::document(&c, "привет ", 14);
    c
}
fn stopped(c: &Client) -> bool {
    let mut value = 0;
    unsafe {
        assert_eq!((c.api.stopped)(c.handle, &mut value), S_OK);
    }
    value != 0
}
fn selection(start: i32, length: i32, ase: TfActiveSelEnd) -> (Span, TF_SELECTIONSTYLE) {
    (
        Span { start, length },
        TF_SELECTIONSTYLE {
            ase,
            fInterimChar: false.into(),
        },
    )
}
#[test]
fn finish_unchanged_range_preserves_moved_or_selected_caret() {
    for span in [
        Span {
            start: 12,
            length: 0,
        },
        Span {
            start: 11,
            length: 0,
        },
        Span {
            start: 0,
            length: 18,
        },
    ] {
        let c = Client::new();
        c.type_text("hello");
        c.a.selection.set(span);
        let writes = c.a.writes.get();
        let ends = c.a.ends.get();
        assert!(!c.event(Event::command(6)));
        oracle::selection(&c, "hello", span, c.a.style.get());
        assert_eq!(c.a.writes.get(), writes);
        assert_eq!(c.a.ends.get(), ends + 1);
        assert!(c.a.active.borrow().is_none());
        assert!(!stopped(&c));
    }
}
#[test]
fn unchanged_end_unknown_never_writes_retries_or_replans() {
    use windows_core::Interface;
    for case in 0..14 {
        let c = corrected();
        let before = crate::support::dll::snapshot(&c.service);
        let writes = c.a.writes.get();
        let ends = c.a.ends.get();
        let starts = c.a.starts.get();
        let d = Rc::clone(&c.a);
        let span = Rc::clone(&d.active.borrow().as_ref().unwrap().span);
        let other = c.context_b.as_raw() as usize;
        let focus: ITfThreadMgrEventSink = c.service.cast().unwrap();
        let profile: ITfInputProcessorProfileActivationSink = c.service.cast().unwrap();
        c.host.on("EndComposition.after", 0, move || match case {
            0 | 1 | 13 => {
                d.results
                    .borrow_mut()
                    .insert("EndComposition", if case == 1 { S_FALSE } else { E_FAIL });
            }
            2 => {}
            3 => d.text.borrow_mut()[7] = b'!' as u16,
            4 => {
                let mut value = span.get();
                value.length += 1;
                span.set(value);
            }
            5 => d.range_context.set(other as *mut _),
            6 => d.foreign.set(true),
            7 => d.status.set(TF_SD_READONLY),
            8 => unsafe {
                assert_eq!(
                    (focus.vtable().OnPushContext)(focus.as_raw(), other as *mut _),
                    S_OK
                );
            },
            9 => unsafe {
                assert_eq!(
                    (profile.vtable().OnActivated)(
                        profile.as_raw(),
                        TF_PROFILETYPE_INPUTPROCESSOR,
                        0x409,
                        &crate::support::providers::SERVICE_ID,
                        &GUID_TFCAT_TIP_KEYBOARD,
                        &crate::support::providers::PROFILE_EN,
                        Default::default(),
                        TF_IPSINK_FLAG_ACTIVE
                    ),
                    S_OK
                );
            },
            10 => {
                let mut style = d.style.get();
                style.fInterimChar = true.into();
                d.style.set(style);
            }
            11 | 12 => {
                d.selection.set(Span {
                    start: 7,
                    length: 0,
                });
                d.results.borrow_mut().insert("SetSelection", E_FAIL);
                if case == 12 {
                    d.results.borrow_mut().insert("EndComposition", E_FAIL);
                }
            }
            _ => unreachable!(),
        });
        if case == 2 || case == 13 {
            c.a.keep_composition.set(true);
        }
        assert!(!c.key(b'x' as u16, false), "case {case}");
        assert!(stopped(&c), "case {case}");
        assert_eq!(
            (c.a.writes.get(), c.a.ends.get(), c.a.starts.get()),
            (writes, ends + 1, starts)
        );
        let after = crate::support::dll::snapshot(&c.service);
        assert_eq!(after.counts[9], before.counts[9]);
        assert_eq!(after.counts[10], before.counts[10]);
        assert_eq!(after.counts[11], before.counts[11] + 1);
        if case != 11 {
            assert!(
                c.a.operations
                    .borrow()
                    .iter()
                    .filter(|o| o.request == c.a.request.get())
                    .all(|o| o.effect == Effect::End)
            );
        }
        let final_counts = counts(&c);
        assert!(!c.key(b'y' as u16, false));
        assert!(!c.event(Event::command(6)));
        assert_eq!(counts(&c), final_counts);
        assert_eq!(
            *c.a.text.borrow(),
            format!(
                "prefix {}suffix",
                if case == 3 {
                    "!ривет "
                } else {
                    "привет "
                }
            )
            .encode_utf16()
            .collect::<Vec<_>>()
        );
        assert_eq!(
            *c.b.text.borrow(),
            "prefix suffix".encode_utf16().collect::<Vec<_>>()
        );
        c.host.drained();
    }
}

#[test]
fn unchanged_end_precondition_refusal_has_no_effect() {
    use windows_core::Interface;
    for case in 0..5 {
        let c = corrected();
        let before = counts(&c);
        match case {
            0 => {
                c.a.invoke.set(false);
                c.a.results.borrow_mut().insert("session", TF_E_SYNCHRONOUS);
            }
            1 => c.a.status.set(TF_SD_READONLY),
            2 => c.a.text.borrow_mut()[7] = b'!' as u16,
            3 => c.a.foreign.set(true),
            4 => {
                let focus: ITfThreadMgrEventSink = c.service.cast().unwrap();
                let other = c.context_b.as_raw() as usize;
                c.host.on("Clone.after", 0, move || unsafe {
                    assert_eq!(
                        (focus.vtable().OnPushContext)(focus.as_raw(), other as *mut _),
                        S_OK
                    );
                });
            }
            _ => unreachable!(),
        }
        assert!(!c.event(Event::command(6)));
        let after = counts(&c);
        assert_eq!(
            (after.writes, after.ends, after.starts, after.selections),
            (before.writes, before.ends, before.starts, before.selections)
        );
        oracle::document(
            &c,
            if case == 2 {
                "!ривет "
            } else {
                "привет "
            },
            14,
        );
        c.host.drained();
    }
}
#[test]
fn finish_native_effects_restore_exact_selection_and_style() {
    for boundary in [None, Some("EndComposition.after")] {
        for (span, style) in [
            selection(14, 0, TF_AE_NONE),
            selection(11, 0, TF_AE_NONE),
            selection(3, 0, TF_AE_NONE),
            selection(9, 3, TF_AE_START),
            selection(6, 10, TF_AE_END),
            selection(0, 20, TF_AE_START),
        ] {
            let c = corrected();
            c.a.selection.set(span);
            c.a.style.set(style);
            let writes = c.a.writes.get();
            let ends = c.a.ends.get();
            let starts = c.a.starts.get();
            if let Some(boundary) = boundary {
                let d = Rc::clone(&c.a);
                c.host.on(boundary, 0, move || {
                    d.selection.set(Span {
                        start: 7,
                        length: 0,
                    });
                    d.style.set(TF_SELECTIONSTYLE {
                        ase: TF_AE_NONE,
                        fInterimChar: false.into(),
                    });
                });
            }
            assert!(!c.event(Event::command(6)));
            oracle::selection(&c, "привет ", span, style);
            assert_eq!(c.a.writes.get(), writes);
            assert_eq!(c.a.ends.get(), ends + 1);
            assert_eq!(c.a.starts.get(), starts);
            assert!(c.a.active.borrow().is_none());
            assert!(!stopped(&c));
        }
    }
}
#[test]
fn finish_continuation_replans_with_one_write_per_composition() {
    for boundary in [None, Some("EndComposition.after")] {
        let c = corrected();
        let writes = c.a.writes.get();
        let ends = c.a.ends.get();
        let starts = c.a.starts.get();
        if let Some(boundary) = boundary {
            let d = Rc::clone(&c.a);
            c.host.on(boundary, 0, move || {
                d.selection.set(Span {
                    start: 7,
                    length: 0,
                })
            });
        }
        assert!(c.key(b'x' as u16, false));
        oracle::document(&c, "привет x", 15);
        assert_eq!(c.a.writes.get(), writes + 1);
        assert_eq!(c.a.ends.get(), ends + 1);
        assert_eq!(c.a.starts.get(), starts + 1);
        let ops: Vec<_> =
            c.a.operations
                .borrow()
                .iter()
                .filter(|o| o.request == c.a.request.get() && o.effect != Effect::Selection)
                .cloned()
                .collect();
        assert_eq!(
            ops.iter().map(|o| o.effect).collect::<Vec<_>>(),
            [Effect::End, Effect::Start, Effect::Text]
        );
        assert_eq!(ops[0].composition, starts);
        assert_eq!(
            ops[0].span,
            Span {
                start: 7,
                length: 7
            }
        );
        assert_eq!(ops[1].composition, starts + 1);
        assert_eq!(
            ops[1].span,
            Span {
                start: 14,
                length: 0
            }
        );
        assert_eq!(ops[2].replacement, vec![b'x' as u16]);
        assert!(!stopped(&c));
    }
}
#[test]
fn finish_clone_gravity_never_reads_unowned_selection() {
    for gravity in [false, true] {
        for span in [
            Span {
                start: 14,
                length: 0,
            },
            Span {
                start: 3,
                length: 0,
            },
            Span {
                start: 9,
                length: 3,
            },
            Span {
                start: 6,
                length: 10,
            },
            Span {
                start: 0,
                length: 20,
            },
        ] {
            for ase in [TF_AE_NONE, TF_AE_START, TF_AE_END] {
                let c = corrected();
                let style = TF_SELECTIONSTYLE {
                    ase,
                    fInterimChar: false.into(),
                };
                c.a.selection.set(span);
                c.a.style.set(style);
                c.a.move_clones.set(gravity);
                let reads = c.a.text_reads.borrow().len();
                let selections =
                    c.a.operations
                        .borrow()
                        .iter()
                        .filter(|o| o.effect == Effect::Selection)
                        .count();
                let d = Rc::clone(&c.a);
                c.host.on("EndComposition.after", 0, move || {
                    d.selection.set(Span {
                        start: 7,
                        length: 0,
                    });
                    d.style.set(TF_SELECTIONSTYLE {
                        ase: TF_AE_NONE,
                        fInterimChar: false.into(),
                    });
                });
                assert!(!c.event(Event::command(6)));
                oracle::selection(&c, "привет ", span, style);
                assert!(!stopped(&c));
                assert_eq!(
                    c.a.operations
                        .borrow()
                        .iter()
                        .filter(|o| o.effect == Effect::Selection)
                        .count(),
                    selections + 1
                );
                assert!(c.a.text_reads.borrow()[reads..].iter().all(|r| *r
                    == Span {
                        start: 7,
                        length: 7
                    }));
            }
        }
    }
}
#[test]
fn finish_matching_selection_requires_no_selection_effect() {
    let c = Client::new();
    c.type_text("hello");
    let before =
        c.a.operations
            .borrow()
            .iter()
            .filter(|o| o.effect == Effect::Selection)
            .count();
    assert!(!c.key(b'!' as u16, false));
    oracle::document(&c, "hello", 12);
    assert_eq!(
        c.a.operations
            .borrow()
            .iter()
            .filter(|o| o.effect == Effect::Selection)
            .count(),
        before
    );
}
#[test]
fn finish_post_end_exclusion_stops_reads_and_continuation() {
    for (notepad, policy) in [false, true]
        .into_iter()
        .flat_map(|notepad| (0..3).map(move |policy| (notepad, policy)))
    {
        let c = if notepad {
            Client::notepad()
        } else {
            Client::new()
        };
        c.type_text("ghbdtn ");
        let d = Rc::clone(&c.a);
        let manager = Rc::clone(&c.manager);
        let host = Rc::clone(&c.host);
        let reads = Rc::new(std::cell::Cell::new(0));
        let record = Rc::clone(&reads);
        let end_trace = Rc::new(std::cell::Cell::new(None));
        let record_end = Rc::clone(&end_trace);
        c.host.on("EndComposition.after", 0, move || {
            record.set(d.reads.get());
            record_end.set(Some(host.trace.borrow().len()));
            match policy {
                0 => {
                    d.results
                        .borrow_mut()
                        .insert("scope.GetValue", E_ACCESSDENIED);
                }
                1 => d.status.set(TF_SD_READONLY),
                2 => manager.disabled.set(true),
                _ => unreachable!(),
            }
        });
        let ends = c.a.ends.get();
        let writes = c.a.writes.get();
        assert!(!c.key(b'x' as u16, false));
        {
            let trace = c.host.trace.borrow();
            let post_end = &trace[end_trace.get().expect("composition ended")..];
            assert!(post_end.contains(&"GetStatus"), "fresh admission after end");
            if policy == 0 {
                let scope = post_end
                    .iter()
                    .position(|name| *name == "GetAppProperty")
                    .expect("fresh application scope query");
                assert!(!post_end[scope..].contains(&"range.ACP"));
                assert!(!post_end[scope..].contains(&"GetText"));
            }
        }
        oracle::document(&c, "привет ", 14);
        assert_eq!(c.a.reads.get(), reads.get());
        assert_eq!(c.a.writes.get(), writes);
        assert_eq!(c.a.ends.get(), ends + 1);
        assert_eq!(c.a.starts.get(), 1);
        assert!(stopped(&c));
        assert!(!c.key(b'y' as u16, false));
        assert_eq!(c.a.reads.get(), reads.get());
        assert_eq!(c.a.writes.get(), writes);
        assert_eq!(c.a.starts.get(), 1);
        assert_eq!(c.a.ends.get(), ends + 1);
        oracle::document(&c, "привет ", 14);
    }
}
#[test]
fn finish_selection_failures_stop_without_replan_or_retry() {
    use windows_core::Interface;
    for case in 0..31 {
        let c = corrected();
        c.a.selection.set(Span {
            start: 6,
            length: 10,
        });
        c.a.style.set(TF_SELECTIONSTYLE {
            ase: TF_AE_START,
            fInterimChar: false.into(),
        });
        c.a.move_clones.set(true);
        let before = crate::support::dll::snapshot(&c.service);
        let writes = c.a.writes.get();
        let ends = c.a.ends.get();
        let starts = c.a.starts.get();
        let reads = c.a.text_reads.borrow().len();
        let selections =
            c.a.operations
                .borrow()
                .iter()
                .filter(|o| o.effect == Effect::Selection)
                .count();
        let attempted = !(case < 6 || (20..=25).contains(&case));
        if attempted {
            let d = Rc::clone(&c.a);
            c.host.on("EndComposition.after", 0, move || {
                d.selection.set(Span {
                    start: 7,
                    length: 0,
                });
                d.style.set(TF_SELECTIONSTYLE {
                    ase: TF_AE_NONE,
                    fInterimChar: false.into(),
                });
            });
        }
        let boundary = match case {
            0..=5 => "Clone.after",
            6 | 8 | 10 => "ShiftEnd",
            7 | 9 | 11 => "ShiftStart",
            20..=25 => "GetSelection",
            26 => "ShiftEnd.after",
            27 => "ShiftStart.after",
            _ => "SetSelection.after",
        };
        let d = Rc::clone(&c.a);
        let other = c.context_b.as_raw() as usize;
        let sink = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
        let composition_span = Rc::clone(&d.active.borrow().as_ref().unwrap().span);
        let excluded = Rc::new(std::cell::Cell::new(None));
        let excluded_in = Rc::clone(&excluded);
        c.host.on(
            boundary,
            usize::from((20..=25).contains(&case)),
            move || match case {
                0 => {
                    let mut value = d.selection.get();
                    value.start += 1;
                    d.selection.set(value);
                }
                1 => {
                    let mut value = d.selection.get();
                    value.length -= 1;
                    d.selection.set(value);
                }
                2 | 13 => {
                    let mut value = d.style.get();
                    value.ase = TF_AE_END;
                    d.style.set(value);
                }
                3 => {
                    let last = d.clones.borrow().last().unwrap().upgrade().unwrap();
                    let mut value = last.get();
                    value.start = 5;
                    last.set(value);
                }
                4 | 19 => {
                    unsafe {
                        assert_eq!(
                            (sink.vtable().OnPushContext)(sink.as_raw(), other as *mut _),
                            S_OK
                        );
                    }
                    excluded_in.set(Some(d.reads.get()));
                }
                5 => d.range_context.set(other as *mut _),
                6 => d.shift_end_limit.set(1),
                7 => d.shift_start_limit.set(1),
                8 | 10 => {
                    d.results
                        .borrow_mut()
                        .insert("ShiftEnd", if case == 8 { E_FAIL } else { S_FALSE });
                }
                9 | 11 => {
                    d.results
                        .borrow_mut()
                        .insert("ShiftStart", if case == 9 { E_FAIL } else { S_FALSE });
                }
                12 | 28 => {
                    d.results
                        .borrow_mut()
                        .insert("SetSelection", if case == 12 { E_FAIL } else { S_FALSE });
                }
                14 | 24 => {
                    let mut value = d.style.get();
                    value.fInterimChar = true.into();
                    d.style.set(value);
                }
                15 => {
                    let mut value = d.selection.get();
                    value.start += 1;
                    d.selection.set(value);
                }
                16 | 26 | 27 => d.foreign.set(true),
                17 => {
                    d.status.set(TF_SD_READONLY);
                    excluded_in.set(Some(d.reads.get()));
                }
                18 => {
                    d.scope.set(IS_PRIVATE);
                    excluded_in.set(Some(d.reads.get()));
                }
                20 | 21 => {
                    d.results
                        .borrow_mut()
                        .insert("GetSelection", if case == 20 { S_FALSE } else { E_FAIL });
                }
                22 => d.selection_fetched.set(2),
                23 => d.nulls.borrow_mut().push("GetSelection"),
                25 => {
                    let mut value = d.style.get();
                    value.ase = TfActiveSelEnd(99);
                    d.style.set(value);
                }
                29 => {
                    let mut value = composition_span.get();
                    value.length += 1;
                    composition_span.set(value);
                }
                30 => d.text.borrow_mut()[7] = b'!' as u16,
                _ => unreachable!(),
            },
        );
        assert!(!c.key(b'x' as u16, false), "case{case}");
        assert!(stopped(&c), "case{case}");
        assert_eq!(c.a.writes.get(), writes, "case{case}");
        assert_eq!(c.a.ends.get(), ends + u32::from(attempted), "case{case}");
        assert_eq!(c.a.starts.get(), starts);
        assert_eq!(
            c.a.operations
                .borrow()
                .iter()
                .filter(|o| o.effect == Effect::Selection)
                .count(),
            selections + usize::from((12..20).contains(&case) || case >= 28),
            "case{case}"
        );
        let after = crate::support::dll::snapshot(&c.service);
        assert_eq!(after.counts[11], before.counts[11] + u32::from(attempted));
        assert_eq!(after.counts[10], before.counts[10] + u32::from(!attempted));
        if let Some(reads) = excluded.get() {
            assert_eq!(c.a.reads.get(), reads);
        }
        assert_eq!(
            *c.a.text.borrow(),
            format!(
                "prefix {}suffix",
                if case == 30 {
                    "!ривет "
                } else {
                    "привет "
                }
            )
            .encode_utf16()
            .collect::<Vec<_>>()
        );
        assert_eq!(
            *c.b.text.borrow(),
            "prefix suffix".encode_utf16().collect::<Vec<_>>()
        );
        assert!(c.a.text_reads.borrow()[reads..].iter().all(|r| *r
            == Span {
                start: 7,
                length: 7
            }));
        let final_reads = c.a.reads.get();
        assert!(!c.key(b'y' as u16, false));
        assert_eq!(c.a.reads.get(), final_reads);
        assert_eq!(c.a.writes.get(), writes);
        c.host.drained();
    }
}
#[test]
fn finish_replacement_failures_stop_after_one_write() {
    use windows_core::Interface;
    for case in 0..10 {
        let c = corrected();
        let before = crate::support::dll::snapshot(&c.service);
        let writes = c.a.writes.get();
        let ends = c.a.ends.get();
        let starts = c.a.starts.get();
        let d = Rc::clone(&c.a);
        let span = Rc::clone(&d.active.borrow().as_ref().unwrap().span);
        let other = c.context_b.as_raw() as usize;
        let sink = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
        let termination = c.service.cast::<ITfCompositionSink>().unwrap();
        let service_event = c.api.event;
        let handle = c.handle as usize;
        let context = c.context_a.as_raw() as usize;
        let at_write = Rc::new(std::cell::Cell::new(0));
        let record = Rc::clone(&at_write);
        c.host.on("SetText.after", 0, move || {
            record.set(d.reads.get());
            match case {
                0 => {
                    d.results.borrow_mut().insert("SetText", E_FAIL);
                }
                1 => d.text.borrow_mut()[7] = b'!' as u16,
                2 => {
                    let mut value = span.get();
                    value.length += 1;
                    span.set(value);
                }
                3 => d.range_context.set(other as *mut _),
                4 => d.status.set(TF_SD_READONLY),
                5 => {
                    d.results
                        .borrow_mut()
                        .insert("scope.GetValue", E_ACCESSDENIED);
                }
                6 => unsafe {
                    assert_eq!(
                        (sink.vtable().OnPushContext)(sink.as_raw(), other as *mut _),
                        S_OK
                    );
                },
                7 => {
                    let active = d.active.borrow_mut().take().unwrap();
                    unsafe {
                        assert_eq!(
                            (termination.vtable().OnCompositionTerminated)(
                                termination.as_raw(),
                                123,
                                active.pointer
                            ),
                            S_OK
                        );
                    }
                }
                8 => {
                    let mut eaten = 1;
                    unsafe {
                        assert_eq!(
                            service_event(
                                handle as *mut _,
                                context as *mut _,
                                &Event::text(b'x' as u16),
                                &mut eaten
                            ),
                            S_OK
                        );
                    }
                    assert_eq!(eaten, 0);
                }
                9 => {
                    d.results.borrow_mut().insert("EndComposition", E_FAIL);
                }
                _ => unreachable!(),
            }
        });
        assert!(c.event(Event::command(3)), "case{case}");
        assert_eq!(c.a.writes.get(), writes + 1);
        assert_eq!(c.a.ends.get(), ends + u32::from(case == 9));
        assert_eq!(c.a.starts.get(), starts);
        assert!(stopped(&c));
        assert_eq!(
            crate::support::dll::snapshot(&c.service).counts[11],
            before.counts[11] + 1
        );
        if case == 4 || case == 5 {
            assert_eq!(c.a.reads.get(), at_write.get());
        }
        oracle::document(&c, if case == 1 { "!hbdtn " } else { "ghbdtn " }, 14);
        let reads = c.a.reads.get();
        assert!(!c.key(b'y' as u16, false));
        assert_eq!(c.a.reads.get(), reads);
        assert_eq!(c.a.writes.get(), writes + 1);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counts {
    reads: u32,
    writes: u32,
    starts: u32,
    ends: u32,
    selections: usize,
}
fn counts(c: &Client) -> Counts {
    Counts {
        reads: c.a.reads.get(),
        writes: c.a.writes.get(),
        starts: c.a.starts.get(),
        ends: c.a.ends.get(),
        selections: c
            .a
            .operations
            .borrow()
            .iter()
            .filter(|o| o.effect == Effect::Selection)
            .count(),
    }
}
fn prepare_selection(c: &Client) {
    c.type_text("ghbdtn ");
    c.a.selection.set(Span {
        start: 6,
        length: 10,
    });
    c.a.style.set(TF_SELECTIONSTYLE {
        ase: TF_AE_START,
        fInterimChar: false.into(),
    });
    c.a.move_clones.set(true);
    let d = Rc::clone(&c.a);
    c.host.on("EndComposition.after", 0, move || {
        d.selection.set(Span {
            start: 7,
            length: 0,
        });
        d.style.set(TF_SELECTIONSTYLE {
            ase: TF_AE_NONE,
            fInterimChar: false.into(),
        });
    });
}
fn inject_policy(
    c: &Client,
    boundary: &'static str,
    skip: usize,
    policy: usize,
) -> Rc<std::cell::Cell<Option<Counts>>> {
    use windows_core::Interface;
    let observed = Rc::new(std::cell::Cell::new(None));
    let record = Rc::clone(&observed);
    let d = Rc::clone(&c.a);
    let host = Rc::clone(&c.host);
    let manager = Rc::clone(&c.manager);
    let sink = c.service.cast::<ITfThreadMgrEventSink>().unwrap();
    let other = c.context_b.as_raw() as usize;
    c.host.on(boundary, skip, move || {
        record.set(Some(Counts {
            reads: d.reads.get(),
            writes: d.writes.get(),
            starts: d.starts.get(),
            ends: d.ends.get(),
            selections: d
                .operations
                .borrow()
                .iter()
                .filter(|o| o.effect == Effect::Selection)
                .count(),
        }));
        match policy {
            0 => d.status.set(TF_SD_READONLY),
            1 => d.disabled.set(true),
            2 => d.empty_disabled.set(true),
            3 => manager.disabled.set(true),
            4 => d.scope.set(IS_PRIVATE),
            5 => {
                d.results
                    .borrow_mut()
                    .insert("scope.GetValue", E_ACCESSDENIED);
            }
            6 => host.field_facts.borrow_mut().as_mut().unwrap().field.style |= 0x800,
            7 => unsafe {
                assert_eq!(
                    (sink.vtable().OnPushContext)(sink.as_raw(), other as *mut _),
                    S_OK
                );
            },
            _ => unreachable!(),
        }
    });
    observed
}
#[test]
fn finish_selection_policy_is_rechecked_after_native_preparation() {
    for effect in [
        "EndComposition",
        "ShiftEnd",
        "ShiftStart",
        "SetSelection",
        "GetText",
    ] {
        let skip = {
            let c = Client::notepad();
            prepare_selection(&c);
            c.host.trace.borrow_mut().clear();
            assert!(!c.event(Event::command(6)));
            assert!(!stopped(&c));
            let trace = c.host.trace.borrow();
            let operation = if effect == "GetText" {
                trace.iter().rposition(|s| *s == effect)
            } else {
                trace.iter().position(|s| *s == effect)
            }
            .unwrap();
            let preparation = trace[..=operation]
                .iter()
                .rposition(|s| *s == "GetExtent")
                .unwrap();
            trace[..preparation]
                .iter()
                .filter(|s| **s == "GetExtent")
                .count()
        };
        for policy in 0..8 {
            let c = Client::notepad();
            prepare_selection(&c);
            let initial = counts(&c);
            let before = crate::support::dll::snapshot(&c.service);
            let recorded = inject_policy(&c, "GetExtent", skip, policy);
            assert!(!c.event(Event::command(6)), "effect{effect},policy{policy}");
            let recorded = recorded.get().unwrap();
            assert_eq!(counts(&c), recorded, "effect{effect},policy{policy}");
            assert!(stopped(&c));
            let attempted = recorded.ends > initial.ends;
            let after = crate::support::dll::snapshot(&c.service);
            assert_eq!(after.counts[11], before.counts[11] + u32::from(attempted));
            assert_eq!(after.counts[10], before.counts[10] + u32::from(!attempted));
            assert_eq!(
                *c.a.text.borrow(),
                "prefix привет suffix".encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(
                *c.b.text.borrow(),
                "prefix suffix".encode_utf16().collect::<Vec<_>>()
            );
            assert!(!c.key(b'y' as u16, false));
            assert_eq!(counts(&c), recorded);
            let pending = c.host.actions.take();
            assert!(
                pending.iter().all(|a| a.boundary == "EndComposition.after"),
                "only unreached movement hook"
            );
        }
    }
}
#[test]
fn fresh_policy_blocks_all_character_access_and_effects() {
    for (operation, preparation, owned, finish) in [
        ("GetText", "ShiftStart", false, false),
        ("GetText", "GetExtent", true, false),
        ("StartComposition", "enumerator.Next", false, false),
        ("SetText.before", "GetExtent", true, false),
        ("SetSelection", "Collapse", true, false),
        ("EndComposition", "GetText.after", true, true),
    ] {
        let event = if finish {
            Event::command(6)
        } else {
            Event::text(b'b' as u16)
        };
        let skip = {
            let c = Client::notepad();
            if owned {
                c.type_text("hello");
            }
            c.host.trace.borrow_mut().clear();
            c.event(event);
            let trace = c.host.trace.borrow();
            let position = trace.iter().position(|s| *s == operation).unwrap();
            let preparation_index = trace[..=position]
                .iter()
                .rposition(|s| *s == preparation)
                .unwrap();
            trace[..preparation_index]
                .iter()
                .filter(|s| **s == preparation)
                .count()
        };
        for policy in 0..8 {
            let c = Client::notepad();
            if owned {
                c.type_text("hello");
            }
            let initial = counts(&c);
            let before = crate::support::dll::snapshot(&c.service);
            let recorded = inject_policy(&c, preparation, skip, policy);
            let eaten = c.event(event);
            let recorded = recorded.get().unwrap();
            let attempted = recorded.ends > initial.ends || recorded.writes > initial.writes;
            assert_eq!(eaten, attempted && !finish, "{operation}/{policy}");
            assert_eq!(counts(&c), recorded, "{operation}/{policy}");
            assert_eq!(
                crate::support::dll::snapshot(&c.service).counts[11],
                before.counts[11] + u32::from(attempted)
            );
            if attempted {
                assert!(stopped(&c));
            }
            oracle::document(
                &c,
                if attempted && !finish {
                    "hellob"
                } else if owned {
                    "hello"
                } else {
                    ""
                },
                if owned { 12 } else { 7 },
            );
            if policy != 7 || owned {
                assert!(!c.key(b'x' as u16, false));
                assert_eq!(counts(&c), recorded);
            }
        }
    }
}
#[test]
fn owned_scope_drift_cannot_borrow_selection_authority() {
    let c = Client::notepad();
    c.type_text("ghbdtn ");
    c.a.selection.set(Span {
        start: 6,
        length: 10,
    });
    let before = counts(&c);
    let diagnostics = crate::support::dll::snapshot(&c.service);
    let d = Rc::clone(&c.a);
    let reads = Rc::new(std::cell::Cell::new(0));
    let record = Rc::clone(&reads);
    c.host.on("GetSelection", 2, move || {
        d.excluded_scope.set(Some(Span {
            start: 7,
            length: 7,
        }));
        record.set(d.reads.get());
    });
    assert!(!c.key(b'x' as u16, false));
    assert_eq!(c.a.reads.get(), reads.get());
    assert_eq!(c.a.writes.get(), before.writes);
    assert_eq!(c.a.ends.get(), before.ends);
    assert!(stopped(&c));
    assert_eq!(
        crate::support::dll::snapshot(&c.service).counts[10],
        diagnostics.counts[10] + 1
    );
    oracle::selection(
        &c,
        "привет ",
        Span {
            start: 6,
            length: 10,
        },
        c.a.style.get(),
    );
}
#[test]
fn empty_start_exclusion_stops_before_character_access() {
    for readonly in [false, true] {
        let c = Client::notepad();
        let d = Rc::clone(&c.a);
        let reads = Rc::new(std::cell::Cell::new(0));
        let record = Rc::clone(&reads);
        let before = crate::support::dll::snapshot(&c.service);
        c.host.on("composition.QI", 0, move || {
            record.set(d.reads.get());
            if readonly {
                d.status.set(TF_SD_READONLY);
            } else {
                d.results
                    .borrow_mut()
                    .insert("scope.GetValue", E_ACCESSDENIED);
            }
        });
        assert!(!c.key(b'g' as u16, false));
        assert_eq!(c.a.reads.get(), reads.get());
        assert_eq!(
            (c.a.starts.get(), c.a.writes.get(), c.a.ends.get()),
            (1, 0, 0)
        );
        assert!(stopped(&c));
        let after = crate::support::dll::snapshot(&c.service);
        assert_eq!(after.counts[10], before.counts[10] + 1);
        assert_eq!(after.counts[11], before.counts[11]);
        oracle::document(&c, "", 7);
        assert!(!c.key(b'x' as u16, false));
        assert_eq!(c.a.reads.get(), reads.get());
    }
}
