//! Independent UIA assertions for named proof-editor cases and diagnostic rows.
#[cfg(test)]
#[path = "proof_denial_tests.rs"]
mod denial_tests;
use crate::diagnostics::{Publication, comparable, format_runtime};
use crate::editor::{Apartment, DENIAL_STATUS, FIXTURE_CLASS};
use crate::native::Target;
use crate::{routing, shutdown, source};
use std::cell::Cell;
use std::process::ExitCode;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

struct Search {
    pid: u32,
    window: Cell<HWND>,
}
unsafe extern "system" fn find_window(window: HWND, argument: LPARAM) -> windows::core::BOOL {
    let result = std::panic::catch_unwind(|| {
        let search = unsafe { &*(argument.0 as *const Search) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
        }
        let mut name = [0; 64];
        let length = unsafe { GetClassNameW(window, &mut name) };
        if pid == search.pid
            && length > 0
            && &name[..length as usize] == unsafe { FIXTURE_CLASS.as_wide() }
        {
            search.window.set(window);
            false
        } else {
            true
        }
    });
    result.unwrap_or(false).into()
}
struct Reader<'a> {
    target: &'a Target,
    root: HWND,
    automation: IUIAutomation,
}
impl<'a> Reader<'a> {
    fn open(target: &'a Target) -> Option<Self> {
        if target.identity.kind != 1 || !target.current() {
            return None;
        }
        let search = Search {
            pid: target.identity.process,
            window: Cell::new(HWND::default()),
        };
        let _ = unsafe {
            EnumWindows(
                Some(find_window),
                LPARAM((&search as *const Search) as isize),
            )
        };
        let root = search.window.get();
        if root.is_invalid() {
            return None;
        }
        let automation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.ok()?;
        Some(Self {
            target,
            root,
            automation,
        })
    }
    fn current(&self, window: HWND, id: i32) -> bool {
        if !self.target.current() {
            return false;
        }
        let root = crate::uia::describe(self.root);
        let child = crate::uia::describe(window);
        root.complete
            && root.pid == self.target.identity.process
            && root.class_name == unsafe { FIXTURE_CLASS.as_wide() }
            && child.complete
            && child.pid == root.pid
            && (if id == 0 {
                window == self.root
            } else {
                child.control == id && unsafe { GetParent(window) }.ok() == Some(self.root)
            })
    }
    fn element(&self, window: HWND, id: i32) -> Option<IUIAutomationElement> {
        if !self.current(window, id) {
            return None;
        }
        let element = unsafe { self.automation.ElementFromHandle(window) }.ok()?;
        if unsafe { element.CurrentProcessId() }.ok()? != self.target.identity.process as i32
            || unsafe { element.CurrentNativeWindowHandle() }.ok()? != window
            || !self.current(window, id)
        {
            return None;
        }
        Some(element)
    }
    fn name(&self, id: i32) -> Option<String> {
        let window = if id == 0 {
            self.root
        } else {
            unsafe { GetDlgItem(Some(self.root), id) }.ok()?
        };
        if id != 0
            && !String::from_utf16_lossy(&crate::uia::describe(window).class_name)
                .eq_ignore_ascii_case("STATIC")
        {
            return None;
        }
        let element = self.element(window, id)?;
        let value = unsafe { element.CurrentName() }.ok()?;
        if !self.current(window, id) {
            return None;
        }
        String::from_utf16(&value).ok()
    }
    fn publication(&self) -> Option<Publication> {
        Publication::parse(&self.name(106)?)
    }
    fn check_edit(&self, id: i32, expected: &str, caret: bool) -> bool {
        self.check_edit_inner(id, expected, caret).is_some()
    }
    fn check_edit_inner(&self, id: i32, expected: &str, caret: bool) -> Option<()> {
        if ![101, 102].contains(&id) {
            return None;
        }
        let window = unsafe { GetDlgItem(Some(self.root), id) }.ok()?;
        let facts = crate::uia::describe(window);
        if !facts.complete
            || !facts.enabled
            || facts.style & (ES_READONLY as isize | ES_PASSWORD as isize) != 0
            || !String::from_utf16_lossy(&facts.class_name).eq_ignore_ascii_case("RICHEDIT50W")
        {
            return None;
        }
        let guard = || self.current(window, id) && crate::uia::describe(window) == facts;
        let element = self.element(window, id)?;
        if unsafe { element.CurrentIsPassword() }.ok()?.0 != 0
            || unsafe { element.CurrentIsEnabled() }.ok()?.0 != 1
            || !guard()
        {
            return None;
        }
        let value: IUIAutomationValuePattern =
            unsafe { element.GetCurrentPatternAs(UIA_ValuePatternId) }.ok()?;
        if unsafe { value.CurrentIsReadOnly() }.ok()?.0 != 0 || !guard() {
            return None;
        }
        let actual = unsafe { value.CurrentValue() }.ok()?;
        let expected_rich_edit: Vec<u16> = expected.encode_utf16().chain(Some(13)).collect();
        if actual.to_vec() != expected_rich_edit || !guard() {
            return None;
        }
        if !caret {
            return Some(());
        }
        let pattern: IUIAutomationTextPattern =
            unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }.ok()?;
        if !guard() {
            return None;
        }
        let selections = unsafe { pattern.GetSelection() }.ok()?;
        if unsafe { selections.Length() }.ok()? != 1 || !guard() {
            return None;
        }
        let selection = unsafe { selections.GetElement(0) }.ok()?;
        let document = unsafe { pattern.DocumentRange() }.ok()?;
        if unsafe {
            selection.CompareEndpoints(
                TextPatternRangeEndpoint_Start,
                &selection,
                TextPatternRangeEndpoint_End,
            )
        }
        .ok()?
            != 0
            || !guard()
        {
            return None;
        }
        if unsafe {
            selection.CompareEndpoints(
                TextPatternRangeEndpoint_End,
                &document,
                TextPatternRangeEndpoint_End,
            )
        }
        .ok()?
            != 0
            || !guard()
        {
            return None;
        }
        Some(())
    }
    fn shutdown_ready(&self) -> bool {
        if self.name(0).as_deref() != Some(shutdown::TITLE)
            || self.name(shutdown::MODE_ID).as_deref() != Some(shutdown::MODE_STATUS)
        {
            return false;
        }
        let Some(first) = self.publication() else {
            return false;
        };
        if first.availability != 4
            || first.saturated != 0
            || shutdown::capturable(&first.snapshot) != shutdown::Verdict::Verified
        {
            return false;
        }
        let mode = first.snapshot.base[3];
        let expected = match mode {
            1 => "ghbdtn",
            2 => "руддщ",
            _ => return false,
        };
        if !self.check_edit(101, expected, true) || !self.check_edit(102, "", false) {
            return false;
        }
        let Some(second) = self.publication() else {
            return false;
        };
        second.availability == 4
            && second.saturated == 0
            && shutdown::capturable(&second.snapshot) == shutdown::Verdict::Verified
            && second.source == first.source
            && second.snapshot.base[0] == first.snapshot.base[0]
            && second.snapshot.base[3] == mode
            && second.snapshot.lifecycle[5] == first.snapshot.lifecycle[5]
            && self.name(0).as_deref() == Some(shutdown::TITLE)
            && self.name(shutdown::MODE_ID).as_deref() == Some(shutdown::MODE_STATUS)
    }
    fn source_result(&self) -> Option<source::Status> {
        if self.name(0).as_deref() != Some(source::TITLE)
            || self.name(source::MODE_ID).as_deref() != Some(source::MODE_STATUS)
        {
            return None;
        }
        source::Status::parse(&self.name(source::RESULT_ID)?)
    }
    fn source_activated(&self) -> bool {
        let Some(result) = self.source_result() else {
            return false;
        };
        let Some(first) = self.publication() else {
            return false;
        };
        if result.0[0] != 3
            || !self.check_edit(101, "ghbdtn", true)
            || !self.check_edit(102, "", false)
        {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(600));
        let Some(second) = self.publication() else {
            return false;
        };
        source::matches(&result, &first, &second)
            && self.check_edit(101, "ghbdtn", true)
            && self.check_edit(102, "", false)
            && self.source_result() == Some(result)
    }
    fn denial(&self) -> bool {
        if !denial_setup_confirmed(self.name(107).as_deref()) {
            return false;
        }
        let Some(first) = self.publication() else {
            return false;
        };
        if !self.check_edit(101, "ghbdtn ", true) || !self.check_edit(102, "", false) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(600));
        let Some(second) = self.publication() else {
            return false;
        };
        denial_setup_confirmed(self.name(107).as_deref())
            && denial_matches(&first, &second)
            && self.check_edit(101, "ghbdtn ", true)
            && self.check_edit(102, "", false)
    }
    fn runtime(&self, require_ready: bool) -> bool {
        let Some(text) = self.name(111) else {
            return false;
        };
        let Some(value) = parse_runtime(&text) else {
            return false;
        };
        if !require_ready {
            println!("{text}");
            return true;
        }
        let Some(service) = self.publication() else {
            return false;
        };
        value[1] == 2
            && value[2] == 2
            && value[3] == 1
            && [2, 3].contains(&value[8])
            && [2, 3].contains(&value[10])
            && service.availability == 4
            && service.saturated == 0
            && service.snapshot.base[4] == 0
            && service.snapshot.base[1] != 0
            && [1, 2].contains(&service.snapshot.base[3])
            && service.snapshot.base[2] == 0
            && service.snapshot.base[0] == value[0]
    }
}
fn denial_setup_confirmed(status: Option<&str>) -> bool {
    status == Some(DENIAL_STATUS)
}
/// Require failed native requests with zero session-entry and editing counters.
pub fn denial_matches(first: &Publication, second: &Publication) -> bool {
    if !comparable(first, second) {
        return false;
    }
    for publication in [first, second] {
        let s = &publication.snapshot.base;
        if publication.availability != 4
            || publication.saturated != 0
            || publication.snapshot.saturated()
            || s[0] == 0
            || s[1] == 0
            || s[2] != 0
            || s[3] != 1
            || s[14] == 0
            || s[5] == 0
            || [15, 16, 17, 18, 20, 12].iter().any(|&index| s[index] != 0)
        {
            return false;
        }
        let request_failed = (s[6] as i32) < 0;
        let stage = if request_failed { 7 } else { 8 };
        let result = if request_failed { s[6] } else { s[8] };
        if (!request_failed && (s[6] != 0 || s[7] == 0 || s[8] as i32 >= 0))
            || s[51] == 0
            || s[52] != stage
            || s[53] != 1
            || s[54] != result
            || s[21 + stage as usize] == 0
        {
            return false;
        }
    }
    true
}
/// Parse a complete canonical runtime status row and validate every field.
#[must_use]
pub fn parse_runtime(text: &str) -> Option<[u32; 12]> {
    let fields: Vec<_> = text.split("; ").collect();
    if fields.len() != 12 || fields[0] != "Keyrill runtime v1" {
        return None;
    }
    let values: Option<Vec<u64>> = fields[1..]
        .iter()
        .map(|field| field.split_once('=')?.1.parse().ok())
        .collect();
    let values = values?;
    let mut result = [0; 12];
    for index in 0..4 {
        result[index] = values[index].try_into().ok()?;
    }
    result[4] = values[4] as u32;
    result[5] = (values[4] >> 32) as u32;
    for index in 6..12 {
        result[index] = values[index - 1].try_into().ok()?;
    }
    if result[1] > 2
        || result[2] > 2
        || result[3] > 1
        || result[6] > 9
        || result[7] > 9
        || result[8] > 3
        || result[9] > 9
        || result[10] > 3
        || result[11] > 9
    {
        return None;
    }
    (format_runtime(&result) == text).then_some(result)
}
fn expected_case(name: &str) -> Option<(&'static str, &'static str)> {
    Some((
        match name {
            "correction" => "привет ",
            "reversal" => "ghbdtn ",
            "continuation" => "привет x",
            "focus" => "привет ghbdtn ",
            "empty" => "",
            "manual" => "привет",
            "manual-reversed" => "ghbdtn",
            "manual-provisional" => "хорошо",
            "manual-pc" => "ёлка",
            "model" => "список ",
            "model-ru" => "world ",
            "model-reversed" | "vocabulary" => "cgbcjr ",
            "model-ru-reversed" => "цщкдв ",
            _ => return None,
        },
        if name == "focus" { "x" } else { "" },
    ))
}

#[cfg(test)]
pub(crate) fn check_owned_fixture(root: HWND, expected: &str) -> bool {
    let Ok(target) =
        Target::open(unsafe { windows::Win32::System::Threading::GetCurrentProcessId() })
    else {
        return false;
    };
    let Ok(automation) = (unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) })
    else {
        return false;
    };
    let reader = Reader {
        target: &target,
        root,
        automation,
    };
    reader.check_edit(101, expected, true) && reader.check_edit(102, "", false)
}
/// Execute one established proof-inspector assertion against an explicit fixture PID.
pub fn inspect_proof(arguments: &[String]) -> ExitCode {
    if arguments.len() != 2 {
        eprintln!("Usage: KeyrillProofInspect FIXTURE_PID CASE");
        return ExitCode::from(2);
    }
    let Some(pid) = crate::options::number(&arguments[0]) else {
        return ExitCode::from(2);
    };
    let name = arguments[1].as_str();
    let expected = expected_case(name);
    if expected.is_none()
        && ![
            "routing",
            "service",
            "denial",
            "shutdown-ready",
            "source-activated",
            "runtime",
            "model-ready",
        ]
        .contains(&name)
    {
        return ExitCode::from(2);
    }
    let Ok(target) = Target::open(pid) else {
        return ExitCode::from(2);
    };
    let Ok(_apartment) = Apartment::initialize() else {
        return ExitCode::FAILURE;
    };
    let Some(reader) = Reader::open(&target) else {
        return ExitCode::from(2);
    };
    let success = if let Some((a, b)) = expected {
        reader.check_edit(101, a, true) && reader.check_edit(102, b, false)
    } else {
        match name {
            "routing" => reader
                .name(105)
                .and_then(|text| routing::Snapshot::parse(&text))
                .map(|snapshot| println!("{}", snapshot.format()))
                .is_some(),
            "service" => reader
                .publication()
                .and_then(|value| value.format())
                .map(|text| println!("{text}"))
                .is_some(),
            "denial" => reader.denial(),
            "shutdown-ready" => reader.shutdown_ready(),
            "source-activated" => reader.source_activated(),
            "runtime" => reader.runtime(false),
            "model-ready" => reader.runtime(true),
            _ => false,
        }
    };
    if !["routing", "service", "runtime"].contains(&name) || !success {
        println!(
            "Fixture {name} oracle {}. No text recorded.",
            if success {
                "matched"
            } else {
                "mismatch or unavailable evidence"
            }
        );
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_public_cases_and_runtime_text_are_canonical() {
        for name in [
            "correction",
            "reversal",
            "continuation",
            "focus",
            "empty",
            "manual",
            "manual-reversed",
            "manual-provisional",
            "manual-pc",
            "model",
            "model-ru",
            "model-reversed",
            "model-ru-reversed",
            "vocabulary",
        ] {
            assert!(expected_case(name).is_some());
        }
        assert!(expected_case("arbitrary input").is_none());
        let value = [1, 2, 2, 1, 0xffffffff, 1, 0, 0, 2, 0, 3, 0];
        let text = format_runtime(&value);
        assert_eq!(parse_runtime(&text), Some(value));
        assert!(parse_runtime(&(text + " ")).is_none());
    }
}
