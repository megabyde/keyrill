use super::*;
use crate::protocol::Identity;

struct Fixture {
    target: Identity,
    foreground_lifetime: LifetimeFacts,
    focused_lifetime: LifetimeFacts,
    foreground_queue: QueueFocus,
    focused_queue: QueueFocus,
    meta: Metadata,
    compared: i32,
    text: Option<Vec<u16>>,
    selection: Selection,
    read_failure: Option<Refusal>,
    observations: u32,
    reads: u32,
    selections: u32,
    bound: usize,
    change_at: u32,
    change: Option<fn(&mut Fixture)>,
}
impl Default for Fixture {
    fn default() -> Self {
        let window = WindowFacts {
            handle: 1,
            complete: true,
            enabled: true,
            ..Default::default()
        };
        Self {
            target: Identity {
                process: 123,
                started: 100,
                kind: 1,
                ..Default::default()
            },
            foreground_lifetime: LifetimeFacts {
                process: 123,
                process_started: 100,
                process_wait: 258,
                thread_owner: 123,
                thread: 456,
                thread_started: 200,
                thread_wait: 258,
            },
            focused_lifetime: LifetimeFacts {
                process: 123,
                process_started: 100,
                process_wait: 258,
                thread_owner: 123,
                thread: 789,
                thread_started: 300,
                thread_wait: 258,
            },
            foreground_queue: QueueFocus {
                available: true,
                focused: 1,
            },
            focused_queue: QueueFocus {
                available: true,
                focused: 1,
            },
            meta: Metadata {
                refusal: Refusal::None,
                native: NativeFocus {
                    pid: 123,
                    foreground: 2,
                    focused: window.clone(),
                    root: WindowFacts {
                        handle: 2,
                        ..window
                    },
                    ..Default::default()
                },
                provider_pid: 123,
                provider_window: 1,
                control_type: 50030,
                nonpassword: true,
                enabled: true,
                value_readonly: ReadOnly::Writable,
                text_readonly: ReadOnly::Writable,
                text_pattern: true,
                ..Default::default()
            },
            compared: 1,
            text: Some(Vec::new()),
            selection: Selection {
                complete: true,
                count: 1,
                collapsed: 0,
                at_end: 0,
            },
            read_failure: None,
            observations: 0,
            reads: 0,
            selections: 0,
            bound: 0,
            change_at: 0,
            change: None,
        }
    }
}
impl Provider for Fixture {
    fn metadata(&mut self) -> Metadata {
        self.observations += 1;
        if self.observations == self.change_at
            && let Some(change) = self.change
        {
            change(self);
        }
        let mut result = self.meta.clone();
        if !current(self.foreground_lifetime, &self.target, 456, 200)
            || !current(self.focused_lifetime, &self.target, 789, 300)
        {
            result.refusal = Refusal::Stale;
        }
        if !exact_queues(
            self.foreground_queue,
            self.focused_queue,
            result.native.root.handle,
            result.native.foreground,
        ) {
            result.refusal = Refusal::NativeFocus;
        }
        result.exact_binding = bound(
            &result.native,
            result.provider_pid,
            result.provider_window,
            self.compared,
        );
        result
    }
    fn document(&mut self, limit: usize) -> Result<Vec<u16>, Refusal> {
        self.reads += 1;
        self.bound = limit;
        if let Some(failure) = self.read_failure {
            return Err(failure);
        }
        copy_document_text(
            0,
            self.text
                .as_ref()
                .map(|text| &text[..text.len().min(limit)]),
            limit,
        )
    }
    fn end_caret(&mut self) -> Result<(), Refusal> {
        self.selections += 1;
        if end_caret(self.selection) {
            Ok(())
        } else {
            Err(Refusal::Selection)
        }
    }
}
fn refused_without_read(provider: &mut Fixture, expected: Refusal) {
    assert_eq!(
        check_public_case(provider, PublicCase::Empty),
        Err(expected)
    );
    assert_eq!((provider.reads, provider.selections), (0, 0));
}
#[test]
fn metadata_only() {
    let mut provider = Fixture {
        text: Some("private text must not be read".encode_utf16().collect()),
        ..Default::default()
    };
    assert_eq!(provider.metadata().refusal, Refusal::None);
    assert_eq!(
        (provider.observations, provider.reads, provider.selections),
        (1, 0, 0)
    );
}
#[test]
fn identity_and_exclusions_all_baseline_axes() {
    for schedule in 0..7 {
        let mut p = Fixture::default();
        match schedule {
            0 => p.foreground_queue.available = false,
            1 => p.focused_queue.available = false,
            2 => p.foreground_queue.focused = 0,
            3 => p.focused_queue.focused = 0,
            4 => p.focused_queue.focused = 3,
            5 => p.meta.native.root.handle = 0,
            6 => p.meta.native.root.handle = 3,
            _ => unreachable!(),
        }
        refused_without_read(&mut p, Refusal::NativeFocus);
    }
    for schedule in 0..16 {
        let mut p = Fixture::default();
        let lifetime = if schedule < 8 {
            &mut p.foreground_lifetime
        } else {
            &mut p.focused_lifetime
        };
        match schedule % 8 {
            0 => lifetime.process_wait = 0,
            1 => lifetime.thread_wait = 0,
            2 => lifetime.thread_wait = u32::MAX,
            3 => lifetime.process += 1,
            4 => lifetime.process_started += 1,
            5 => lifetime.thread_owner += 1,
            6 => lifetime.thread += 1,
            7 => lifetime.thread_started += 1,
            _ => unreachable!(),
        }
        refused_without_read(&mut p, Refusal::Stale);
    }
    for schedule in 0..4 {
        let mut p = Fixture::default();
        match schedule {
            0 => p.meta.provider_window = 0,
            1 => p.meta.provider_window = 2,
            2 => p.meta.provider_pid += 1,
            3 => p.compared = 0,
            _ => unreachable!(),
        };
        refused_without_read(&mut p, Refusal::Binding);
    }
    for reason in [
        Refusal::Target,
        Refusal::NativeFocus,
        Refusal::Stale,
        Refusal::Provider,
    ] {
        let mut p = Fixture::default();
        p.meta.refusal = reason;
        refused_without_read(&mut p, reason);
    }
    for schedule in 0..6 {
        let mut p = Fixture::default();
        match schedule {
            0 => p.meta.native.focused.style = 0x20,
            1 => p.meta.native.focused.style = 0x800,
            2 => p.meta.native.focused.enabled = false,
            3 => p.meta.native.root.enabled = false,
            4 => p.meta.nonpassword = false,
            5 => p.meta.enabled = false,
            _ => unreachable!(),
        };
        refused_without_read(
            &mut p,
            match schedule {
                0..=3 => Refusal::NativeExcluded,
                4 => Refusal::Password,
                _ => Refusal::Disabled,
            },
        );
    }
    for schedule in 0..4 {
        let mut p = Fixture::default();
        match schedule {
            0 => p.meta.native.focused.complete = false,
            1 => p.meta.native.root.complete = false,
            2 => p.meta.native.parents_truncated = true,
            3 => p.meta.native.parents.push(WindowFacts::default()),
            _ => unreachable!(),
        };
        refused_without_read(&mut p, Refusal::NativeFacts);
    }
}
#[test]
fn writability_all_baseline_channels_and_boolean_results() {
    for (result, value, expected) in [
        (0, 0, ReadOnly::Writable),
        (0, 1, ReadOnly::ReadOnly),
        (0, 2, ReadOnly::Unknown),
        (-1, 0, ReadOnly::Unavailable),
        (1, 0, ReadOnly::Unavailable),
    ] {
        assert_eq!(boolean(result, value), expected);
    }
    assert_eq!(attribute_boolean(0), ReadOnly::Writable);
    assert_eq!(attribute_boolean(-1), ReadOnly::ReadOnly);
    assert_eq!(attribute_boolean(1), ReadOnly::Unknown);
    for value in [
        ReadOnly::Unavailable,
        ReadOnly::Unsupported,
        ReadOnly::Writable,
    ] {
        for text in [
            ReadOnly::Unavailable,
            ReadOnly::Unsupported,
            ReadOnly::Writable,
        ] {
            let mut p = Fixture::default();
            p.meta.value_readonly = value;
            p.meta.text_readonly = text;
            let writable = value == ReadOnly::Writable || text == ReadOnly::Writable;
            assert_eq!(
                check_public_case(&mut p, PublicCase::Empty).is_ok(),
                writable
            );
            assert_eq!(p.reads, u32::from(writable));
        }
    }
    for veto in [ReadOnly::Unknown, ReadOnly::ReadOnly, ReadOnly::Mixed] {
        for value_channel in [false, true] {
            let mut p = Fixture::default();
            if value_channel {
                p.meta.value_readonly = veto;
            } else {
                p.meta.text_readonly = veto;
            }
            refused_without_read(
                &mut p,
                match veto {
                    ReadOnly::ReadOnly => Refusal::ReadOnly,
                    ReadOnly::Mixed => Refusal::Mixed,
                    _ => Refusal::Writability,
                },
            );
        }
    }
    let mut p = Fixture::default();
    p.meta.text_pattern = false;
    refused_without_read(&mut p, Refusal::TextUnavailable);
    for control in [0, 50004, 50000] {
        let mut p = Fixture::default();
        p.meta.control_type = control;
        refused_without_read(&mut p, Refusal::ControlType);
    }
}
#[test]
fn revalidation_before_after_text_and_before_success() {
    let mut before = Fixture {
        change_at: 2,
        change: Some(|p| p.foreground_lifetime.thread_wait = 0),
        ..Default::default()
    };
    refused_without_read(&mut before, Refusal::Stale);
    let mut focus = Fixture {
        change_at: 2,
        change: Some(|p| p.meta.native.focused.control += 1),
        ..Default::default()
    };
    refused_without_read(&mut focus, Refusal::Stale);
    for observation in [2, 3, 4] {
        let mut p = Fixture {
            change_at: observation,
            change: Some(|p| p.meta.text_readonly = ReadOnly::ReadOnly),
            ..Default::default()
        };
        assert_eq!(
            check_public_case(&mut p, PublicCase::Empty),
            Err(Refusal::ReadOnly)
        );
        assert!(p.reads <= 1);
    }
}
#[test]
fn exact_public_cases_suffixes_and_all_six_selection_axes() {
    for case in PublicCase::ALL {
        assert_eq!(PublicCase::parse(case.name()), Some(case));
        let expected: Vec<u16> = case.text().encode_utf16().collect();
        let mut p = Fixture {
            text: Some(expected.clone()),
            ..Default::default()
        };
        assert_eq!(check_public_case(&mut p, case), Ok(()));
        assert_eq!(
            (p.reads, p.bound, p.selections, p.observations),
            (1, expected.len() + 1, 1, 4)
        );
        for suffix in [u16::from(b'x'), 0, 13] {
            let mut actual = expected.clone();
            actual.push(suffix);
            let mut p = Fixture {
                text: Some(actual),
                ..Default::default()
            };
            assert_eq!(check_public_case(&mut p, case), Err(Refusal::TextMismatch));
            assert_eq!(p.selections, 0);
        }
        for selection in [
            Selection {
                complete: false,
                count: 1,
                collapsed: 0,
                at_end: 0,
            },
            Selection {
                complete: true,
                count: 0,
                collapsed: 0,
                at_end: 0,
            },
            Selection {
                complete: true,
                count: 2,
                collapsed: 0,
                at_end: 0,
            },
            Selection {
                complete: true,
                count: 1,
                collapsed: 1,
                at_end: 0,
            },
            Selection {
                complete: true,
                count: 1,
                collapsed: 0,
                at_end: -1,
            },
            Selection {
                complete: true,
                count: 1,
                collapsed: 0,
                at_end: 1,
            },
        ] {
            let mut p = Fixture {
                text: Some(expected.clone()),
                selection,
                ..Default::default()
            };
            assert_eq!(check_public_case(&mut p, case), Err(Refusal::Selection));
            assert_eq!(p.reads, 1);
        }
    }
    let mut p = Fixture {
        text: Some(vec![103, 0, 98, 100, 116, 110, 32]),
        ..Default::default()
    };
    assert_eq!(
        check_public_case(&mut p, PublicCase::FixtureEnBoundary),
        Err(Refusal::TextMismatch)
    );
    for (actual, expected) in [
        (
            PublicCase::FixtureEnBoundary,
            PublicCase::FixtureEnCorrected,
        ),
        (
            PublicCase::FixtureEnCorrected,
            PublicCase::FixtureEnBoundary,
        ),
    ] {
        let mut p = Fixture {
            text: Some(actual.text().encode_utf16().collect()),
            ..Default::default()
        };
        assert_eq!(
            check_public_case(&mut p, expected),
            Err(Refusal::TextMismatch)
        );
    }
    let mut p = Fixture {
        read_failure: Some(Refusal::TextUnavailable),
        ..Default::default()
    };
    assert_eq!(
        check_public_case(&mut p, PublicCase::Empty),
        Err(Refusal::TextUnavailable)
    );
    assert_eq!((p.reads, p.selections), (1, 0));
}
#[test]
fn null_and_returned_text_boundary_all_baseline_axes() {
    let mut p = Fixture {
        text: None,
        ..Default::default()
    };
    assert_eq!(
        check_public_case(&mut p, PublicCase::Empty),
        Err(Refusal::TextUnavailable)
    );
    assert_eq!((p.reads, p.selections), (1, 0));
    assert_eq!(copy_document_text(0, Some(&[]), 1), Ok(Vec::new()));
    for result in [-1, 1] {
        assert_eq!(
            copy_document_text(result, Some(&[103]), 2),
            Err(Refusal::TextUnavailable)
        );
    }
    for text in [[103, 0], [103, 120], [103, 13]] {
        assert_eq!(copy_document_text(0, Some(&text), 2), Ok(text.to_vec()));
        assert_eq!(
            copy_document_text(0, Some(&text), 1),
            Err(Refusal::TextUnavailable)
        );
    }
}
