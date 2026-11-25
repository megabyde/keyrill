//! Public fixture expectations and independent metadata-first field evidence.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Compiled public document cases accepted by the inspector.
pub enum PublicCase {
    /// Expected document is empty.
    Empty,
    /// Expected document is `g`.
    ProbeG,
    /// Expected document is `ghbdtn `, including its trailing space.
    FixtureEnBoundary,
    /// Expected document is `привет `, including its trailing space.
    FixtureEnCorrected,
    /// Expected document is `ghbdtn`.
    FixtureEnWord,
    /// Expected document is `привет`.
    FixtureRuWord,
    /// Expected document is `привет x`.
    FixtureContinuation,
    /// Expected document is `привет привет `, including its trailing space.
    FixtureTwoWords,
    /// Expected document is `хорошо`.
    ManualProvisional,
    /// Expected document is `[jhjij`.
    ManualProvisionalLiteral,
    /// Expected document is `ёлка`.
    ManualPc,
    /// Expected document is `` `krf ``, including its trailing space.
    ManualPcLiteral,
    /// Expected document is `список `, including its trailing space.
    ModelEn,
    /// Expected document is `cgbcjr `, including its trailing space.
    ModelEnLiteral,
    /// Expected document is `world `, including its trailing space.
    ModelRu,
    /// Expected document is `цщкдв `, including its trailing space.
    ModelRuLiteral,
    /// Expected document is `left привет right`.
    PrefixSuffix,
}
impl PublicCase {
    /// Every accepted public case in stable enumeration order.
    pub const ALL: [Self; 17] = [
        Self::Empty,
        Self::ProbeG,
        Self::FixtureEnBoundary,
        Self::FixtureEnCorrected,
        Self::FixtureEnWord,
        Self::FixtureRuWord,
        Self::FixtureContinuation,
        Self::FixtureTwoWords,
        Self::ManualProvisional,
        Self::ManualProvisionalLiteral,
        Self::ManualPc,
        Self::ManualPcLiteral,
        Self::ModelEn,
        Self::ModelEnLiteral,
        Self::ModelRu,
        Self::ModelRuLiteral,
        Self::PrefixSuffix,
    ];
    /// Parse an exact canonical command, case name, or status representation.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|case| case.name() == name)
    }
    /// Return the compiled public case name.
    pub fn name(self) -> &'static str {
        [
            "empty",
            "probe-g",
            "fixture-en-boundary",
            "fixture-en-corrected",
            "fixture-en-word",
            "fixture-ru-word",
            "fixture-continuation",
            "fixture-two-words",
            "manual-provisional",
            "manual-provisional-literal",
            "manual-pc",
            "manual-pc-literal",
            "model-en",
            "model-en-literal",
            "model-ru",
            "model-ru-literal",
            "prefix-suffix",
        ][self as usize]
    }
    /// Return the compiled public UTF-16 document expectation as Unicode text.
    pub fn text(self) -> &'static str {
        [
            "",
            "g",
            "ghbdtn ",
            "привет ",
            "ghbdtn",
            "привет",
            "привет x",
            "привет привет ",
            "хорошо",
            "[jhjij",
            "ёлка",
            "`krf ",
            "список ",
            "cgbcjr ",
            "world ",
            "цщкдв ",
            "left привет right",
        ][self as usize]
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Provider evidence about document writability.
pub enum ReadOnly {
    #[default]
    /// The provider supplied no conclusive fact.
    Unknown,
    /// The native query failed or returned no usable evidence.
    Unavailable,
    /// The provider explicitly reports an unsupported attribute.
    Unsupported,
    /// The provider reports writable content.
    Writable,
    /// The provider or native style reports read-only content.
    ReadOnly,
    /// The document exposes a mixed attribute value.
    Mixed,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// The first failed field-evidence predicate.
pub enum Refusal {
    /// No refusal or failure has been observed.
    None,
    #[default]
    /// The retained target process is no longer current.
    Target,
    /// Native foreground or focused-window evidence could not be captured.
    NativeFocus,
    /// Required native window facts or ancestor evidence are incomplete.
    NativeFacts,
    /// A native window is disabled or has an excluded style.
    NativeExcluded,
    /// Captured lifetime, native focus, or provider facts changed.
    Stale,
    /// A required UI Automation operation or provider was unavailable.
    Provider,
    /// Provider identity did not match the captured native focus.
    Binding,
    /// The provider did not report the required edit control type.
    ControlType,
    /// The control is password-protected.
    Password,
    /// The control reports disabled.
    Disabled,
    /// The provider or native style reports read-only content.
    ReadOnly,
    /// The document exposes a mixed attribute value.
    Mixed,
    /// Provider evidence did not establish writable content.
    Writability,
    /// Document text was unavailable, unsuccessful, or exceeded its bound.
    TextUnavailable,
    /// The document text did not match the selected public case.
    TextMismatch,
    /// The required end-of-document caret could not be established.
    Selection,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Native window metadata captured without reading characters.
pub struct WindowFacts {
    /// Retained native handle or captured HWND value.
    pub handle: usize,
    /// Explicit target process identifier.
    pub pid: u32,
    /// Captured native thread identifier.
    pub tid: u32,
    /// Native dialog control ID.
    pub control: i32,
    /// Native GWL_STYLE value.
    pub style: isize,
    /// Native GWL_EXSTYLE value.
    pub exstyle: isize,
    /// Whether the native control or provider reports enabled.
    pub enabled: bool,
    /// Whether all required native facts were captured exactly.
    pub complete: bool,
    /// Whether the class-name buffer was exhausted.
    pub class_truncated: bool,
    /// Exact UTF-16 native class name without its terminator.
    pub class_name: Vec<u16>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Native focus queues and retained process/thread lifetime evidence.
pub struct NativeFocus {
    /// Explicit target process identifier.
    pub pid: u32,
    /// Thread owning the foreground queue.
    pub foreground_tid: u32,
    /// Captured process creation FILETIME.
    pub process_started: u64,
    /// Creation FILETIME of the retained foreground thread.
    pub foreground_thread_started: u64,
    /// Creation FILETIME of the retained focused thread.
    pub focused_thread_started: u64,
    /// Captured foreground HWND.
    pub foreground: usize,
    /// Captured focused-control metadata.
    pub focused: WindowFacts,
    /// Captured root-window metadata.
    pub root: WindowFacts,
    /// Bounded native ancestor chain.
    pub parents: Vec<WindowFacts>,
    /// Whether the ancestor chain exceeded the capture bound.
    pub parents_truncated: bool,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Provider metadata bound to the complete native focus snapshot.
pub struct Metadata {
    /// The first failed eligibility predicate.
    pub refusal: Refusal,
    /// Complete native focus evidence used to bind the provider.
    pub native: NativeFocus,
    /// Process ID reported by UIA.
    pub provider_pid: i32,
    /// HWND reported by UIA.
    pub provider_window: usize,
    /// UIA control-type identifier.
    pub control_type: i32,
    /// Whether both UIA lookups identify the captured focused window.
    pub exact_binding: bool,
    /// Whether UIA reports an exact false password flag.
    pub nonpassword: bool,
    /// Whether the native control or provider reports enabled.
    pub enabled: bool,
    /// ValuePattern writability evidence.
    pub value_readonly: ReadOnly,
    /// TextPattern document-attribute writability evidence.
    pub text_readonly: ReadOnly,
    /// Whether a TextPattern was acquired.
    pub text_pattern: bool,
}
/// Retained process/thread facts required before a UIA provider may be used.
#[derive(Clone, Copy, Debug, Default)]
pub struct LifetimeFacts {
    /// Process identifier from the retained native handle.
    pub process: u32,
    /// Creation FILETIME from the retained process handle.
    pub process_started: u64,
    /// Native process wait result.
    pub process_wait: u32,
    /// Owning process identifier from the retained thread handle.
    pub thread_owner: u32,
    /// Thread identifier from the retained thread handle.
    pub thread: u32,
    /// Creation FILETIME from the retained thread handle.
    pub thread_started: u64,
    /// Native thread wait result.
    pub thread_wait: u32,
}
/// Require exact live process and thread identities from both retained handles.
pub fn current(
    now: LifetimeFacts,
    target: &crate::protocol::Identity,
    thread: u32,
    thread_started: u64,
) -> bool {
    now.process_wait == 258
        && now.thread_wait == 258
        && now.process == target.process
        && now.process_started == target.started
        && now.thread_owner == target.process
        && now.thread == thread
        && now.thread_started == thread_started
}
/// Focus reported by one independently queried GUI queue.
#[derive(Clone, Copy, Debug, Default)]
pub struct QueueFocus {
    /// Whether the queue query completed successfully.
    pub available: bool,
    /// Focused native window reported by that queue.
    pub focused: usize,
}
/// Require both GUI queues to identify the same nonzero field under the foreground root.
pub fn exact_queues(
    foreground: QueueFocus,
    focused: QueueFocus,
    root: usize,
    foreground_window: usize,
) -> bool {
    foreground.available
        && focused.available
        && foreground.focused != 0
        && foreground.focused == focused.focused
        && root != 0
        && root == foreground_window
}
/// Require exact provider PID, HWND, and UIA comparison evidence.
pub fn bound(native: &NativeFocus, pid: i32, window: usize, compared: i32) -> bool {
    pid == native.pid as i32 && window != 0 && window == native.focused.handle && compared == 1
}
/// Exact UIA selection endpoint comparison results.
#[derive(Clone, Copy, Debug, Default)]
pub struct Selection {
    /// Whether every endpoint query completed successfully.
    pub complete: bool,
    /// Number of selections returned by the provider.
    pub count: i32,
    /// Start-to-end comparison, with zero establishing a collapsed selection.
    pub collapsed: i32,
    /// Selection-end to document-end comparison.
    pub at_end: i32,
}
/// Require exactly one complete collapsed end-of-document caret.
pub fn end_caret(value: Selection) -> bool {
    value.complete && value.count == 1 && value.collapsed == 0 && value.at_end == 0
}
/// Interpret native Boolean evidence only for exact S_OK and Boolean values.
pub fn boolean(result: i32, value: i32) -> ReadOnly {
    if result != 0 {
        return ReadOnly::Unavailable;
    }
    match value {
        0 => ReadOnly::Writable,
        1 => ReadOnly::ReadOnly,
        _ => ReadOnly::Unknown,
    }
}
/// Interpret exact VARIANT Boolean values without coercion.
pub fn attribute_boolean(value: i16) -> ReadOnly {
    match value {
        0 => ReadOnly::Writable,
        -1 => ReadOnly::ReadOnly,
        _ => ReadOnly::Unknown,
    }
}
/// Preserve every returned UTF-16 code unit while enforcing exact success and the read bound.
///
/// # Errors
/// Refuses a failed HRESULT, null returned BSTR, or provider text beyond the requested bound.
pub fn copy_document_text(
    result: i32,
    value: Option<&[u16]>,
    bound: usize,
) -> Result<Vec<u16>, Refusal> {
    let value = value
        .filter(|value| result == 0 && value.len() <= bound)
        .ok_or(Refusal::TextUnavailable)?;
    Ok(value.to_vec())
}
/// Refuse incomplete, disabled, password, and read-only native controls.
pub fn native_safety(value: &NativeFocus) -> Refusal {
    if !value.focused.complete || !value.root.complete || value.parents_truncated {
        return Refusal::NativeFacts;
    }
    if !value.focused.enabled || value.focused.style & (0x20 | 0x800) != 0 {
        return Refusal::NativeExcluded;
    }
    for parent in &value.parents {
        if !parent.complete {
            return Refusal::NativeFacts;
        }
        if !parent.enabled {
            return Refusal::NativeExcluded;
        }
    }
    if value.root.enabled {
        Refusal::None
    } else {
        Refusal::NativeExcluded
    }
}
/// Combine independent ValuePattern and TextPattern writability evidence.
pub fn writable(value: ReadOnly, text: ReadOnly) -> Refusal {
    use self::ReadOnly::*;
    if [value, text].contains(&ReadOnly) {
        return Refusal::ReadOnly;
    }
    if [value, text].contains(&Mixed) {
        return Refusal::Mixed;
    }
    if [value, text].contains(&Unknown) {
        return Refusal::Writability;
    }
    if [value, text].contains(&Writable) {
        Refusal::None
    } else {
        Refusal::Writability
    }
}
/// Check every metadata predicate before permitting a character read.
pub fn eligible(value: &Metadata) -> Refusal {
    eligible_control(value, &[50030])
}
/// Check ordinary Edit or Document metadata for explicitly bound public fixtures.
pub fn eligible_public(value: &Metadata) -> Refusal {
    eligible_control(value, &[50004, 50030])
}
fn eligible_control(value: &Metadata, controls: &[i32]) -> Refusal {
    if value.refusal != Refusal::None {
        return value.refusal;
    }
    let native = native_safety(&value.native);
    if native != Refusal::None {
        return native;
    }
    if !value.exact_binding {
        return Refusal::Binding;
    }
    if !controls.contains(&value.control_type) {
        return Refusal::ControlType;
    }
    if !value.nonpassword {
        return Refusal::Password;
    }
    if !value.enabled {
        return Refusal::Disabled;
    }
    writable(value.value_readonly, value.text_readonly)
}
/// Boundary used by the independent whole-document and caret checker.
pub trait Provider {
    /// Capture provider metadata without reading document characters.
    fn metadata(&mut self) -> Metadata;
    /// Read at most the explicit public-case UTF-16 bound.
    ///
    /// # Errors
    /// Refuses unavailable or changed document evidence.
    fn document(&mut self, bound: usize) -> Result<Vec<u16>, Refusal>;
    /// Require one collapsed selection at the complete document end.
    ///
    /// # Errors
    /// Refuses an unavailable, noncollapsed, or displaced caret.
    fn end_caret(&mut self) -> Result<(), Refusal>;
    /// Compare the exact public UTF-16 text before, inside, and after the selection.
    ///
    /// # Errors
    /// Refuses providers without independent exact selection evidence.
    fn public_selection(
        &mut self,
        _before: &[u16],
        _selected: &[u16],
        _after: &[u16],
    ) -> Result<(), Refusal> {
        Err(Refusal::Selection)
    }
}
/// Validate stable metadata, the complete public document, and its exact caret.
///
/// # Errors
/// Returns the first failed native operation or evidence predicate.
pub fn check_public_case(provider: &mut impl Provider, case: PublicCase) -> Result<(), Refusal> {
    let evidence = provider.metadata();
    let refusal = eligible(&evidence);
    if refusal != Refusal::None {
        return Err(refusal);
    }
    if !evidence.text_pattern {
        return Err(Refusal::TextUnavailable);
    }
    let revalidate = |provider: &mut dyn Provider| {
        let now = provider.metadata();
        let failure = eligible(&now);
        if failure != Refusal::None {
            Err(failure)
        } else if now != evidence {
            Err(Refusal::Stale)
        } else {
            Ok(())
        }
    };
    revalidate(provider)?;
    let expected: Vec<u16> = case.text().encode_utf16().collect();
    let actual = provider.document(expected.len() + 1)?;
    if actual != expected {
        return Err(Refusal::TextMismatch);
    }
    revalidate(provider)?;
    provider.end_caret()?;
    revalidate(provider)
}

#[cfg(test)]
#[path = "field_tests.rs"]
mod baseline_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        meta: Metadata,
        text: Vec<u16>,
        reads: u32,
        caret: bool,
    }
    impl Provider for Fake {
        fn metadata(&mut self) -> Metadata {
            self.meta.clone()
        }
        fn document(&mut self, _: usize) -> Result<Vec<u16>, Refusal> {
            self.reads += 1;
            Ok(self.text.clone())
        }
        fn end_caret(&mut self) -> Result<(), Refusal> {
            if self.caret {
                Ok(())
            } else {
                Err(Refusal::Selection)
            }
        }
    }
    fn safe() -> Fake {
        let window = WindowFacts {
            enabled: true,
            complete: true,
            ..Default::default()
        };
        Fake {
            meta: Metadata {
                refusal: Refusal::None,
                native: NativeFocus {
                    focused: window.clone(),
                    root: window,
                    ..Default::default()
                },
                exact_binding: true,
                control_type: 50030,
                nonpassword: true,
                enabled: true,
                value_readonly: ReadOnly::Unsupported,
                text_readonly: ReadOnly::Writable,
                text_pattern: true,
                ..Default::default()
            },
            caret: true,
            ..Default::default()
        }
    }
    #[test]
    fn secure_excluded_and_uncertain_fields_have_zero_character_reads() {
        for refusal in [
            Refusal::Target,
            Refusal::NativeFocus,
            Refusal::NativeFacts,
            Refusal::NativeExcluded,
            Refusal::Stale,
            Refusal::Provider,
            Refusal::Binding,
            Refusal::ControlType,
            Refusal::Password,
            Refusal::Disabled,
            Refusal::ReadOnly,
            Refusal::Mixed,
            Refusal::Writability,
        ] {
            let mut provider = safe();
            provider.meta.refusal = refusal;
            assert_eq!(
                check_public_case(&mut provider, PublicCase::Empty),
                Err(refusal)
            );
            assert_eq!(provider.reads, 0);
        }
        for state in [
            ReadOnly::ReadOnly,
            ReadOnly::Mixed,
            ReadOnly::Unknown,
            ReadOnly::Unavailable,
            ReadOnly::Unsupported,
        ] {
            let mut provider = safe();
            provider.meta.value_readonly = ReadOnly::Unsupported;
            provider.meta.text_readonly = state;
            assert!(check_public_case(&mut provider, PublicCase::Empty).is_err());
            assert_eq!(provider.reads, 0);
        }
    }
    #[test]
    fn named_case_requires_complete_document_and_exact_end_caret() {
        for case in PublicCase::ALL {
            let mut provider = safe();
            provider.text = case.text().encode_utf16().collect();
            assert_eq!(check_public_case(&mut provider, case), Ok(()));
            provider.text.push(0);
            assert_eq!(
                check_public_case(&mut provider, case),
                Err(Refusal::TextMismatch)
            );
            provider.text.pop();
            provider.caret = false;
            assert_eq!(
                check_public_case(&mut provider, case),
                Err(Refusal::Selection)
            );
        }
    }
}
