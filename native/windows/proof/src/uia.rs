//! Explicit-target UIA evidence. Character reads follow native and provider metadata.
use crate::field::{self, Metadata, NativeFocus, Provider, ReadOnly, Refusal, WindowFacts};
use crate::native::{Target, ThreadCheck, ThreadState, capture_thread, started, window_long};
use crate::public_fixture::Fixture;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::Variant::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, BSTR, HRESULT, IUnknown, Interface};

pub(crate) fn exact_value<T: Default>(call: impl FnOnce(*mut T) -> HRESULT) -> Result<T, Refusal> {
    let mut output = T::default();
    if call(&mut output) == S_OK {
        Ok(output)
    } else {
        Err(Refusal::Provider)
    }
}
pub(crate) fn exact_interface<T: Interface>(
    call: impl FnOnce(*mut *mut std::ffi::c_void) -> HRESULT,
) -> Result<T, Refusal> {
    let mut raw = std::ptr::null_mut();
    let result = call(&mut raw);
    if raw.is_null() {
        return Err(Refusal::Provider);
    }
    let output = unsafe { T::from_raw(raw) };
    if result == S_OK {
        Ok(output)
    } else {
        Err(Refusal::Provider)
    }
}
macro_rules! value {
    ($object:expr, $method:ident $(, $argument:expr)*) => {{
        let object = &$object;
        exact_value(|output| unsafe { (object.vtable().$method)(object.as_raw(), $($argument,)* output) })
    }};
}
macro_rules! interface {
    ($object:expr, $method:ident $(, $argument:expr)*) => {{
        let object = &$object;
        exact_interface(|output| unsafe { (object.vtable().$method)(object.as_raw(), $($argument,)* output) })
    }};
}

/// Capture native window metadata without inspecting text.
pub fn describe(handle: HWND) -> WindowFacts {
    let mut result = WindowFacts {
        handle: handle.0 as usize,
        ..Default::default()
    };
    if handle.is_invalid() || !unsafe { IsWindow(Some(handle)) }.as_bool() {
        return result;
    }
    result.tid = unsafe { GetWindowThreadProcessId(handle, Some(&mut result.pid)) };
    let mut name = [0; 128];
    let length = unsafe { GetClassNameW(handle, &mut name) };
    result.class_truncated = length == 127;
    result.class_name = name[..length.max(0) as usize].to_vec();
    unsafe {
        SetLastError(ERROR_SUCCESS);
    }
    result.style = unsafe { window_long(handle, GWL_STYLE) };
    let style_error = unsafe { GetLastError() };
    unsafe {
        SetLastError(ERROR_SUCCESS);
    }
    result.exstyle = unsafe { window_long(handle, GWL_EXSTYLE) };
    let exstyle_error = unsafe { GetLastError() };
    unsafe {
        SetLastError(ERROR_SUCCESS);
    }
    result.control = unsafe { GetDlgCtrlID(handle) };
    let control_error = unsafe { GetLastError() };
    result.enabled = unsafe { IsWindowEnabled(handle) }.as_bool();
    result.complete = result.tid != 0
        && length > 0
        && !result.class_truncated
        && style_error == ERROR_SUCCESS
        && exstyle_error == ERROR_SUCCESS
        && control_error == ERROR_SUCCESS;
    result
}

/// UIA objects and native lifetime evidence for one explicit foreground target.
pub struct NativeProvider<'a> {
    target: &'a Target,
    foreground_thread: ThreadCheck,
    focused_thread: ThreadCheck,
    foreground_started: u64,
    focused_started: u64,
    native: NativeFocus,
    automation: IUIAutomation,
    element: Option<IUIAutomationElement>,
    text: Option<IUIAutomationTextPattern>,
    document: Option<IUIAutomationTextRange>,
    fixture: Option<Fixture>,
    runtime_id: Option<Vec<i32>>,
}
impl<'a> NativeProvider<'a> {
    /// Acquire the retained native identities required by this observer.
    ///
    /// # Errors
    /// Returns the first failed native operation or evidence predicate.
    pub fn open(target: &'a Target) -> Result<Self, Refusal> {
        if target.identity.kind == 0 || !target.current() {
            return Err(Refusal::Target);
        }
        let foreground = unsafe { GetForegroundWindow() };
        let mut pid = 0;
        let tid = unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid)) };
        if foreground.is_invalid() || pid != target.identity.process || tid == 0 {
            return Err(Refusal::NativeFocus);
        }
        let foreground_thread = capture_thread(target.process.0, pid, tid);
        let foreground_started = foreground_thread
            .handle
            .as_ref()
            .filter(|_| foreground_thread.state == ThreadState::Live)
            .and_then(|handle| started(handle.0, true).ok())
            .ok_or(Refusal::Stale)?;
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetGUIThreadInfo(tid, &mut info) }.map_err(|_| Refusal::NativeFocus)?;
        if info.hwndFocus.is_invalid()
            || unsafe { GetAncestor(info.hwndFocus, GA_ROOT) } != foreground
        {
            return Err(Refusal::NativeFocus);
        }
        let focused_tid = unsafe { GetWindowThreadProcessId(info.hwndFocus, Some(&mut pid)) };
        if focused_tid == 0 || pid != target.identity.process {
            return Err(Refusal::NativeFocus);
        }
        let focused_thread = capture_thread(target.process.0, pid, focused_tid);
        let focused_started = focused_thread
            .handle
            .as_ref()
            .filter(|_| focused_thread.state == ThreadState::Live)
            .and_then(|handle| started(handle.0, true).ok())
            .ok_or(Refusal::Stale)?;
        let native = sample(
            target,
            &foreground_thread,
            foreground_started,
            &focused_thread,
            focused_started,
        )?;
        let safety = field::native_safety(&native);
        if safety != Refusal::None {
            return Err(safety);
        }
        let automation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| Refusal::Provider)?;
        let result = Self {
            target,
            foreground_thread,
            focused_thread,
            foreground_started,
            focused_started,
            native,
            automation,
            element: None,
            text: None,
            document: None,
            fixture: None,
            runtime_id: None,
        };
        result.guard()?;
        Ok(result)
    }
    /// Bind subsequent reads to one explicitly selected public fixture control.
    pub fn for_fixture(mut self, fixture: Fixture) -> Self {
        self.fixture = Some(fixture);
        self
    }
    fn read_ready(&mut self) -> Result<(), Refusal> {
        if self.target.identity.kind == 3 && self.fixture.is_none() {
            return Err(Refusal::Binding);
        }
        let metadata = self.metadata_inner()?;
        let refusal = if self.fixture.is_some() {
            field::eligible_public(&metadata)
        } else {
            field::eligible(&metadata)
        };
        if refusal != Refusal::None {
            return Err(refusal);
        }
        self.focused_guard()
    }
    fn focused_guard(&self) -> Result<(), Refusal> {
        self.guard()?;
        if let Some(element) = &self.element {
            let focused: IUIAutomationElement =
                self.call(|| interface!(self.automation, GetFocusedElement))?;
            let same: BOOL = self.call(|| {
                value!(
                    self.automation,
                    CompareElements,
                    focused.as_raw(),
                    element.as_raw()
                )
            })?;
            if same != BOOL(1) {
                return Err(Refusal::Stale);
            }
            if let Some(expected) = &self.runtime_id
                && self.call(|| runtime_id(&focused))? != *expected
            {
                return Err(Refusal::Stale);
            }
        }
        self.guard()
    }
    fn public_binding(
        &self,
        focused: &IUIAutomationElement,
        fixture: Fixture,
    ) -> Result<Vec<i32>, Refusal> {
        let name = self.call(|| element_name(focused))?;
        let name = String::from_utf16(&name).map_err(|_| Refusal::Binding)?;
        if !fixture.control_name_matches(&name) {
            return Err(Refusal::Binding);
        }
        let id = self.call(|| runtime_id(focused))?;
        let walker: IUIAutomationTreeWalker =
            self.call(|| interface!(self.automation, RawViewWalker))?;
        let mut element = focused.clone();
        let mut public_document = false;
        let mut native_binding = false;
        for _ in 0..32 {
            let pid: i32 = self.call(|| value!(element, CurrentProcessId))?;
            if pid != self.target.identity.process as i32 {
                break;
            }
            let kind: UIA_CONTROLTYPE_ID = self.call(|| value!(element, CurrentControlType))?;
            let name = self.call(|| element_name(&element))?;
            let name = String::from_utf16(&name).map_err(|_| Refusal::Binding)?;
            public_document |=
                kind == UIA_DocumentControlTypeId && fixture.document_name_matches(&name);
            let window: HWND = self.call(|| value!(element, CurrentNativeWindowHandle))?;
            if !window.is_invalid()
                && [self.native.focused.handle, self.native.root.handle]
                    .contains(&(window.0 as usize))
            {
                let from_handle: IUIAutomationElement =
                    self.call(|| interface!(self.automation, ElementFromHandle, window))?;
                let same: BOOL = self.call(|| {
                    value!(
                        self.automation,
                        CompareElements,
                        element.as_raw(),
                        from_handle.as_raw()
                    )
                })?;
                native_binding |= same == BOOL(1);
            }
            if public_document && native_binding {
                return Ok(id);
            }
            element = self.call(|| interface!(walker, GetParentElement, element.as_raw()))?;
        }
        Err(Refusal::Binding)
    }
    fn guard(&self) -> Result<(), Refusal> {
        let now = sample(
            self.target,
            &self.foreground_thread,
            self.foreground_started,
            &self.focused_thread,
            self.focused_started,
        )?;
        if now == self.native {
            Ok(())
        } else {
            Err(Refusal::Stale)
        }
    }
    fn call<T>(&self, function: impl FnOnce() -> Result<T, Refusal>) -> Result<T, Refusal> {
        self.guard()?;
        let result = function();
        self.guard()?;
        result
    }
    fn attribute(&self, value: &VARIANT) -> ReadOnly {
        let raw = unsafe { &value.Anonymous.Anonymous };
        if raw.vt == VT_BOOL {
            return field::attribute_boolean(unsafe { raw.Anonymous.boolVal }.0);
        }
        if raw.vt != VT_UNKNOWN {
            return ReadOnly::Unknown;
        }
        let Some(unknown) = (unsafe { &*raw.Anonymous.punkVal }) else {
            return ReadOnly::Unknown;
        };
        let identity: Result<IUnknown, Refusal> = self
            .call(|| exact_interface(|output| unsafe { unknown.query(&IUnknown::IID, output) }));
        let Ok(identity) = identity else {
            return ReadOnly::Unknown;
        };
        let mixed: Result<IUnknown, _> =
            self.call(|| interface!(self.automation, ReservedMixedAttributeValue));
        let unsupported: Result<IUnknown, _> =
            self.call(|| interface!(self.automation, ReservedNotSupportedValue));
        if mixed.ok().as_ref() == Some(&identity) {
            ReadOnly::Mixed
        } else if unsupported.ok().as_ref() == Some(&identity) {
            ReadOnly::Unsupported
        } else {
            ReadOnly::Unknown
        }
    }
    fn metadata_inner(&mut self) -> Result<Metadata, Refusal> {
        self.guard()?;
        let mut result = Metadata {
            native: self.native.clone(),
            ..Default::default()
        };
        let safety = field::native_safety(&result.native);
        if safety != Refusal::None {
            return Err(safety);
        }
        let focused: IUIAutomationElement =
            self.call(|| interface!(self.automation, GetFocusedElement))?;
        result.provider_pid = self.call(|| value!(focused, CurrentProcessId))?;
        let window: HWND = self.call(|| value!(focused, CurrentNativeWindowHandle))?;
        result.provider_window = window.0 as usize;
        if result.provider_pid != self.target.identity.process as i32 {
            return Err(Refusal::Binding);
        }
        if let Some(fixture) = self.fixture {
            let id = self.public_binding(&focused, fixture)?;
            if self
                .runtime_id
                .as_ref()
                .is_some_and(|previous| *previous != id)
            {
                return Err(Refusal::Stale);
            }
            self.runtime_id = Some(id);
        } else {
            if result.provider_window == 0 || result.provider_window != self.native.focused.handle {
                return Err(Refusal::Binding);
            }
            let from_handle: IUIAutomationElement =
                self.call(|| interface!(self.automation, ElementFromHandle, window))?;
            let same: BOOL = self.call(|| {
                value!(
                    self.automation,
                    CompareElements,
                    focused.as_raw(),
                    from_handle.as_raw()
                )
            })?;
            if !field::bound(
                &self.native,
                result.provider_pid,
                result.provider_window,
                same.0,
            ) {
                return Err(Refusal::Binding);
            }
        }
        if let Some(element) = &self.element {
            let same: BOOL = self.call(|| {
                value!(
                    self.automation,
                    CompareElements,
                    focused.as_raw(),
                    element.as_raw()
                )
            })?;
            if same != BOOL(1) {
                return Err(Refusal::Stale);
            }
        }
        self.element = Some(focused.clone());
        result.exact_binding = true;
        result.control_type = self
            .call(|| value!(focused, CurrentControlType))
            .map(|id: UIA_CONTROLTYPE_ID| id.0)?;
        let password: BOOL = self.call(|| value!(focused, CurrentIsPassword))?;
        if password != BOOL(0) {
            return Err(Refusal::Password);
        }
        result.nonpassword = true;
        let enabled: BOOL = self.call(|| value!(focused, CurrentIsEnabled))?;
        if enabled != BOOL(1) {
            return Err(Refusal::Disabled);
        }
        result.enabled = true;
        result.value_readonly = ReadOnly::Unavailable;
        let value: Result<IUIAutomationValuePattern, _> = self.call(|| {
            interface!(
                focused,
                GetCurrentPatternAs,
                UIA_ValuePatternId,
                &IUIAutomationValuePattern::IID
            )
        });
        if let Ok(value) = value {
            result.value_readonly = match self
                .call(|| value!(value, CurrentIsReadOnly))
                .map(|value: BOOL| value.0)
            {
                Ok(0) => ReadOnly::Writable,
                Ok(1) => ReadOnly::ReadOnly,
                Ok(_) => ReadOnly::Unknown,
                Err(_) => ReadOnly::Unavailable,
            };
        }
        result.text_readonly = ReadOnly::Unavailable;
        let text: Result<IUIAutomationTextPattern, _> = self.call(|| {
            interface!(
                focused,
                GetCurrentPatternAs,
                UIA_TextPatternId,
                &IUIAutomationTextPattern::IID
            )
        });
        let mut document: Option<IUIAutomationTextRange> = None;
        if let Ok(text) = &text {
            result.text_pattern = true;
            document = self.call(|| interface!(text, DocumentRange)).ok();
            if let Some(document) = &document
                && let Ok(attribute) =
                    self.call(|| value!(document, GetAttributeValue, UIA_IsReadOnlyAttributeId))
            {
                result.text_readonly = self.attribute(&attribute);
            }
        }
        self.guard()?;
        self.text = text.ok();
        self.document = document;
        result.refusal = Refusal::None;
        Ok(result)
    }
}
impl Provider for NativeProvider<'_> {
    fn metadata(&mut self) -> Metadata {
        match self.metadata_inner() {
            Ok(result) => result,
            Err(refusal) => Metadata {
                native: self.native.clone(),
                refusal,
                ..Default::default()
            },
        }
    }
    fn document(&mut self, bound: usize) -> Result<Vec<u16>, Refusal> {
        if self.target.identity.kind == 3 && self.fixture.is_none() {
            return Err(Refusal::Binding);
        }
        if bound == 0 || bound > 4096 {
            return Err(Refusal::TextUnavailable);
        }
        self.read_ready()?;
        let document = self.document.as_ref().ok_or(Refusal::TextUnavailable)?;
        let result = self.call(|| {
            let mut raw = std::ptr::null_mut();
            let result =
                unsafe { (document.vtable().GetText)(document.as_raw(), bound as i32, &mut raw) };
            let text = unsafe { BSTR::from_raw(raw.cast()) };
            field::copy_document_text(result.0, (!raw.is_null()).then_some(&text), bound)
        });
        self.focused_guard()?;
        result
    }
    fn end_caret(&mut self) -> Result<(), Refusal> {
        let text = self.text.as_ref().ok_or(Refusal::TextUnavailable)?;
        let document = self.document.as_ref().ok_or(Refusal::TextUnavailable)?;
        let result = self.call(|| {
            let selections: IUIAutomationTextRangeArray = interface!(text, GetSelection)?;
            if value!(selections, Length)? != 1 {
                return Err(Refusal::Selection);
            }
            let selection: IUIAutomationTextRange = interface!(selections, GetElement, 0)?;
            let collapsed: i32 = value!(
                selection,
                CompareEndpoints,
                TextPatternRangeEndpoint_Start,
                selection.as_raw(),
                TextPatternRangeEndpoint_End
            )?;
            let end: i32 = value!(
                selection,
                CompareEndpoints,
                TextPatternRangeEndpoint_End,
                document.as_raw(),
                TextPatternRangeEndpoint_End
            )?;
            if field::end_caret(field::Selection {
                complete: true,
                count: 1,
                collapsed,
                at_end: end,
            }) {
                Ok(())
            } else {
                Err(Refusal::Selection)
            }
        });
        result.map_err(|failure| {
            if failure == Refusal::Stale {
                failure
            } else {
                Refusal::Selection
            }
        })
    }
    fn public_selection(
        &mut self,
        before: &[u16],
        selected: &[u16],
        after: &[u16],
    ) -> Result<(), Refusal> {
        if before
            .len()
            .saturating_add(selected.len())
            .saturating_add(after.len())
            > 4096
        {
            return Err(Refusal::TextUnavailable);
        }
        self.read_ready()?;
        let text = self.text.clone().ok_or(Refusal::TextUnavailable)?;
        let document = self.document.clone().ok_or(Refusal::TextUnavailable)?;
        let selections: IUIAutomationTextRangeArray =
            self.call(|| interface!(text, GetSelection))?;
        if self.call(|| value!(selections, Length))? != 1 {
            return Err(Refusal::Selection);
        }
        let selection: IUIAutomationTextRange =
            self.call(|| interface!(selections, GetElement, 0))?;
        let collapsed: i32 = self.call(|| {
            value!(
                selection,
                CompareEndpoints,
                TextPatternRangeEndpoint_Start,
                selection.as_raw(),
                TextPatternRangeEndpoint_End
            )
        })?;
        if selected.is_empty() && collapsed != 0 {
            return Err(Refusal::Selection);
        }
        let prefix: IUIAutomationTextRange = self.call(|| interface!(document, Clone))?;
        let suffix: IUIAutomationTextRange = self.call(|| interface!(document, Clone))?;
        for (range, endpoint, target_endpoint) in [
            (
                &prefix,
                TextPatternRangeEndpoint_End,
                TextPatternRangeEndpoint_Start,
            ),
            (
                &suffix,
                TextPatternRangeEndpoint_Start,
                TextPatternRangeEndpoint_End,
            ),
        ] {
            self.call(|| {
                let moved = unsafe {
                    (range.vtable().MoveEndpointByRange)(
                        range.as_raw(),
                        endpoint,
                        selection.as_raw(),
                        target_endpoint,
                    )
                };
                if moved == S_OK {
                    Ok(())
                } else {
                    Err(Refusal::Selection)
                }
            })?;
        }
        for (range, expected) in [(&prefix, before), (&selection, selected), (&suffix, after)] {
            self.read_ready()?;
            self.call(|| {
                let mut raw = std::ptr::null_mut();
                let status = unsafe {
                    (range.vtable().GetText)(range.as_raw(), (expected.len() + 1) as i32, &mut raw)
                };
                let text = unsafe { BSTR::from_raw(raw.cast()) };
                if field::copy_document_text(
                    status.0,
                    (!raw.is_null()).then_some(&text),
                    expected.len() + 1,
                )? == expected
                {
                    Ok(())
                } else {
                    Err(Refusal::Selection)
                }
            })?;
            self.focused_guard()?;
        }
        let current: IUIAutomationTextRangeArray = self.call(|| interface!(text, GetSelection))?;
        if self.call(|| value!(current, Length))? != 1 {
            return Err(Refusal::Selection);
        }
        let current: IUIAutomationTextRange = self.call(|| interface!(current, GetElement, 0))?;
        for endpoint in [TextPatternRangeEndpoint_Start, TextPatternRangeEndpoint_End] {
            let same: i32 = self.call(|| {
                value!(
                    current,
                    CompareEndpoints,
                    endpoint,
                    selection.as_raw(),
                    endpoint
                )
            })?;
            if same != 0 {
                return Err(Refusal::Selection);
            }
        }
        self.focused_guard()
    }
}
fn sample(
    target: &Target,
    foreground_thread: &ThreadCheck,
    foreground_started: u64,
    focused_thread: &ThreadCheck,
    focused_started: u64,
) -> Result<NativeFocus, Refusal> {
    if !target.current() {
        return Err(Refusal::Stale);
    }
    for (thread, timestamp) in [
        (foreground_thread, foreground_started),
        (focused_thread, focused_started),
    ] {
        let handle = thread.handle.as_ref().ok_or(Refusal::Stale)?.0;
        let lifetime = field::LifetimeFacts {
            process: unsafe { GetProcessId(target.process.0) },
            process_started: started(target.process.0, false).map_err(|_| Refusal::Stale)?,
            process_wait: unsafe { WaitForSingleObject(target.process.0, 0) }.0,
            thread_owner: unsafe { GetProcessIdOfThread(handle) },
            thread: unsafe { GetThreadId(handle) },
            thread_started: started(handle, true).map_err(|_| Refusal::Stale)?,
            thread_wait: unsafe { WaitForSingleObject(handle, 0) }.0,
        };
        if !field::current(lifetime, &target.identity, thread.tid, timestamp) {
            return Err(Refusal::Stale);
        }
    }
    let foreground = unsafe { GetForegroundWindow() };
    let mut pid = 0;
    let foreground_tid = unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid)) };
    if foreground.is_invalid()
        || pid != target.identity.process
        || foreground_tid != foreground_thread.tid
    {
        return Err(Refusal::Stale);
    }
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(foreground_tid, &mut info) }.map_err(|_| Refusal::Stale)?;
    if info.hwndFocus.is_invalid() || unsafe { GetAncestor(info.hwndFocus, GA_ROOT) } != foreground
    {
        return Err(Refusal::Stale);
    }
    let focused = describe(info.hwndFocus);
    let root = describe(unsafe { GetAncestor(info.hwndFocus, GA_ROOT) });
    let mut focused_info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(focused_thread.tid, &mut focused_info) }
        .map_err(|_| Refusal::Stale)?;
    if focused.pid != pid
        || focused.tid != focused_thread.tid
        || !field::exact_queues(
            field::QueueFocus {
                available: true,
                focused: info.hwndFocus.0 as usize,
            },
            field::QueueFocus {
                available: true,
                focused: focused_info.hwndFocus.0 as usize,
            },
            root.handle,
            foreground.0 as usize,
        )
    {
        return Err(Refusal::Stale);
    }
    let mut parent = unsafe { GetAncestor(info.hwndFocus, GA_PARENT) };
    let mut parents = Vec::new();
    while !parent.is_invalid() && parents.len() != 8 {
        parents.push(describe(parent));
        parent = unsafe { GetAncestor(parent, GA_PARENT) };
    }
    Ok(NativeFocus {
        pid,
        foreground_tid,
        process_started: target.identity.started,
        foreground_thread_started: foreground_started,
        focused_thread_started: focused_started,
        foreground: foreground.0 as usize,
        focused,
        root,
        parents,
        parents_truncated: !parent.is_invalid(),
    })
}

fn runtime_id(element: &IUIAutomationElement) -> Result<Vec<i32>, Refusal> {
    let array: *mut SAFEARRAY = value!(element, GetRuntimeId)?;
    if array.is_null() {
        return Err(Refusal::Binding);
    }
    struct OwnedArray(*mut SAFEARRAY);
    impl Drop for OwnedArray {
        fn drop(&mut self) {
            let _ = unsafe { SafeArrayDestroy(self.0) };
        }
    }
    let array = OwnedArray(array);
    if unsafe { SafeArrayGetDim(array.0) } != 1
        || unsafe { SafeArrayGetVartype(array.0) }.ok() != Some(VT_I4)
    {
        return Err(Refusal::Binding);
    }
    let start = unsafe { SafeArrayGetLBound(array.0, 1) }.map_err(|_| Refusal::Binding)?;
    let end = unsafe { SafeArrayGetUBound(array.0, 1) }.map_err(|_| Refusal::Binding)?;
    if end < start || i64::from(end) - i64::from(start) >= 32 {
        return Err(Refusal::Binding);
    }
    let mut values = Vec::new();
    for index in start..=end {
        let mut value = 0i32;
        unsafe { SafeArrayGetElement(array.0, &index, std::ptr::from_mut(&mut value).cast()) }
            .map_err(|_| Refusal::Binding)?;
        values.push(value);
    }
    Ok(values)
}

fn element_name(element: &IUIAutomationElement) -> Result<BSTR, Refusal> {
    let mut raw = std::ptr::null_mut();
    let status = unsafe { (element.vtable().CurrentName)(element.as_raw(), &mut raw) };
    let name = unsafe { BSTR::from_raw(raw.cast()) };
    if status != S_OK || raw.is_null() || name.len() > 512 {
        return Err(Refusal::Binding);
    }
    Ok(name)
}

#[cfg(test)]
#[path = "uia_tests.rs"]
mod public_provider_tests;
