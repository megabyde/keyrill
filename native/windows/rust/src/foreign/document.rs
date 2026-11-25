//! Typed document queries and effects. Values own memory, while callers retain editing authority.

use super::{Call, Dependencies, InterfaceError, Lease, Returned, query, returned_interface};
use crate::field_contract::{Facts, HostIdentity};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use windows::Win32::Foundation::{HWND, S_OK};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Variant::{VARIANT, VT_EMPTY, VT_I4, VT_UNKNOWN};
use windows::Win32::UI::TextServices::*;
use windows_core::{GUID, HRESULT, Interface};

fn pointer<T: Interface>(lease: &Lease<T>) -> *mut c_void {
    lease.owner.pointer.as_ptr()
}

pub(crate) fn focus(manager: &Lease<ITfThreadMgr>) -> Returned<Option<Lease<ITfDocumentMgr>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: the lease retains this typed interface and output is initialized writable storage.
    let result = unsafe {
        (super::vtable::<ITfThreadMgr>(pointer(manager)).GetFocus)(pointer(manager), &mut output)
    };
    returned_interface(Call { result }, output)
}

pub(crate) fn top(document: &Lease<ITfDocumentMgr>) -> Returned<Option<Lease<ITfContext>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: the lease retains this typed interface and output is initialized writable storage.
    let result = unsafe {
        (super::vtable::<ITfDocumentMgr>(pointer(document)).GetTop)(pointer(document), &mut output)
    };
    returned_interface(Call { result }, output)
}

pub(crate) fn view(context: &Lease<ITfContext>) -> Returned<Option<Lease<ITfContextView>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: the lease retains this typed interface and output is initialized writable storage.
    let result = unsafe {
        (super::vtable::<ITfContext>(pointer(context)).GetActiveView)(pointer(context), &mut output)
    };
    returned_interface(Call { result }, output)
}

pub(crate) fn property(
    context: &Lease<ITfContext>,
) -> Returned<Option<Lease<ITfReadOnlyProperty>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: context is retained, the generated SDK GUID is immutable, and output is writable.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfContext>(pointer(context)).GetAppProperty)(
                pointer(context),
                &GUID_PROP_INPUTSCOPE,
                &mut output,
            )
        },
    };
    returned_interface(call, output)
}

pub(crate) fn whitespace(unit: u16) -> Returned<bool> {
    unsafe extern "C" {
        fn iswspace(unit: u16) -> i32;
    }
    // SAFETY: UCRT classifies one UTF-16 unit without borrowing any document storage.
    let value = unsafe { iswspace(unit) != 0 };
    Returned {
        call: Call { result: S_OK },
        value,
    }
}
pub(crate) fn range_context(range: &Lease<ITfRange>) -> Returned<Option<Lease<ITfContext>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: the lease retains this typed interface and output is initialized writable storage.
    let result = unsafe {
        (super::vtable::<ITfRange>(pointer(range)).GetContext)(pointer(range), &mut output)
    };
    returned_interface(Call { result }, output)
}

pub(crate) fn clone_range(range: &Lease<ITfRange>) -> Returned<Option<Lease<ITfRange>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: the lease retains this typed interface and output is initialized writable storage.
    let result =
        unsafe { (super::vtable::<ITfRange>(pointer(range)).Clone)(pointer(range), &mut output) };
    returned_interface(Call { result }, output)
}

pub(crate) fn compositions(
    context: &Lease<ITfContextComposition>,
) -> Returned<Option<Lease<IEnumITfCompositionView>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: the lease retains this typed interface and output is initialized writable storage.
    let result = unsafe {
        (super::vtable::<ITfContextComposition>(pointer(context)).EnumCompositions)(
            pointer(context),
            &mut output,
        )
    };
    returned_interface(Call { result }, output)
}

pub(crate) fn composition_range(
    composition: &Lease<ITfComposition>,
) -> Returned<Option<Lease<ITfRange>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: the lease retains this typed interface and output is initialized writable storage.
    let result = unsafe {
        (super::vtable::<ITfComposition>(pointer(composition)).GetRange)(
            pointer(composition),
            &mut output,
        )
    };
    returned_interface(Call { result }, output)
}

pub(crate) fn window(view: &Lease<ITfContextView>) -> Returned<HWND> {
    let mut output = HWND::default();
    // SAFETY: view is retained and output is distinct initialized HWND storage.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfContextView>(pointer(view)).GetWnd)(pointer(view), &mut output)
        },
    };
    Returned {
        call,
        value: output,
    }
}

pub(crate) fn status(context: &Lease<ITfContext>) -> Returned<TS_STATUS> {
    let mut output = TS_STATUS::default();
    // SAFETY: context is retained and output is a complete initialized SDK status structure.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfContext>(pointer(context)).GetStatus)(pointer(context), &mut output)
        },
    };
    Returned {
        call,
        value: output,
    }
}

pub(crate) fn compartment(
    manager: &Lease<ITfCompartmentMgr>,
    id: &GUID,
) -> Returned<Option<Lease<ITfCompartment>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: manager and id are live, and output is distinct writable storage.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfCompartmentMgr>(pointer(manager)).GetCompartment)(
                pointer(manager),
                id,
                &mut output,
            )
        },
    };
    returned_interface(call, output)
}

pub(crate) struct Value {
    raw: VARIANT,
    needs_clear: bool,
}

impl Value {
    fn new() -> Self {
        Self {
            raw: VARIANT::default(),
            needs_clear: false,
        }
    }
    pub(crate) fn empty(&self) -> bool {
        // SAFETY: VARIANT is initialized before every host call, and vt is always a scalar field.
        unsafe { self.raw.Anonymous.Anonymous.vt == VT_EMPTY }
    }
    pub(crate) fn zero_integer(&self) -> bool {
        // SAFETY: lVal is read only when the initialized discriminator identifies its active field.
        unsafe {
            self.raw.Anonymous.Anonymous.vt == VT_I4
                && self.raw.Anonymous.Anonymous.Anonymous.lVal == 0
        }
    }
    pub(crate) fn unknown(&self) -> bool {
        // SAFETY: vt is an initialized scalar discriminator.
        unsafe { self.raw.Anonymous.Anonymous.vt == VT_UNKNOWN }
    }
    pub(crate) fn has_unknown(&self) -> bool {
        if !self.unknown() {
            return false;
        }
        // SAFETY: VT_UNKNOWN identifies this initialized interface slot.
        unsafe { self.raw.Anonymous.Anonymous.Anonymous.punkVal.is_some() }
    }
    pub(crate) fn input_scope(&self) -> Result<Lease<ITfInputScope>, InterfaceError> {
        if !self.unknown() {
            return Err(InterfaceError::Missing);
        }
        // SAFETY: VT_UNKNOWN identifies a borrowed interface slot kept alive by this VARIANT.
        let unknown = unsafe { &self.raw.Anonymous.Anonymous.Anonymous.punkVal };
        let raw = unknown
            .as_ref()
            .map_or(std::ptr::null_mut(), Interface::as_raw);
        if raw.is_null() {
            return Err(InterfaceError::Missing);
        }
        // SAFETY: the variant retains the live SDK interface for the complete QI.
        unsafe { query(raw) }
    }
    pub(crate) fn clear(mut self) -> Call {
        self.release()
    }
    fn release(&mut self) -> Call {
        if !self.needs_clear {
            return Call { result: S_OK };
        }
        self.needs_clear = false;
        windows_core::link!("oleaut32.dll" "system" fn VariantClear(value: *mut VARIANT) -> HRESULT);
        // SAFETY: typed GetValue transfers this initialized VARIANT to caller cleanup.
        Call {
            result: unsafe { VariantClear(&mut self.raw) },
        }
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

pub(crate) fn compartment_value(compartment: &Lease<ITfCompartment>) -> Returned<Value> {
    let mut value = Value::new();
    // SAFETY: compartment is retained and the initialized VARIANT is complete writable output storage.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfCompartment>(pointer(compartment)).GetValue)(
                pointer(compartment),
                &mut value.raw,
            )
        },
    };
    value.needs_clear = true;

    Returned { call, value }
}

pub(crate) fn property_value(
    property: &Lease<ITfReadOnlyProperty>,
    cookie: u32,
    range: &Lease<ITfRange>,
) -> Returned<Value> {
    let mut value = Value::new();
    // SAFETY: property/range are retained for this cookie, with initialized distinct VARIANT output.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfReadOnlyProperty>(pointer(property)).GetValue)(
                pointer(property),
                cookie,
                pointer(range),
                &mut value.raw,
            )
        },
    };
    value.needs_clear = true;

    Returned { call, value }
}

pub(crate) struct Scopes {
    pointer: *mut InputScope,
    pub(crate) count: u32,
}
impl Scopes {
    pub(crate) fn single(&self) -> Option<InputScope> {
        if self.count != 1 || self.pointer.is_null() {
            return None;
        }
        // SAFETY: count one from the normally returned SDK allocation permits exactly one scalar read.
        Some(unsafe { *self.pointer })
    }

    pub(crate) fn ordinary(&self) -> bool {
        self.single()
            .is_some_and(|scope| scope == IS_DEFAULT || scope == IS_TEXT)
    }
}
impl Drop for Scopes {
    fn drop(&mut self) {
        // SAFETY: GetInputScopes transfers this normally returned task-allocator buffer to the caller.
        unsafe { CoTaskMemFree(Some(self.pointer.cast())) };
    }
}
pub(crate) fn scopes(scope: &Lease<ITfInputScope>) -> Returned<Scopes> {
    let mut output = std::ptr::null_mut();
    let mut count = 0;
    // SAFETY: scope is retained and outputs are distinct writable pointer/count slots.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfInputScope>(pointer(scope)).GetInputScopes)(
                pointer(scope),
                &mut output,
                &mut count,
            )
        },
    };

    Returned {
        call,
        value: Scopes {
            pointer: output,
            count,
        },
    }
}

pub(crate) struct Selection {
    pub(crate) range: Option<Lease<ITfRange>>,
    pub(crate) style: TF_SELECTIONSTYLE,
    pub(crate) fetched: u32,
}
pub(crate) fn selection(context: &Lease<ITfContext>, cookie: u32) -> Returned<Selection> {
    let mut selection = TF_SELECTION::default();
    let mut fetched = 0;
    // SAFETY: context is retained, and the complete SDK structure/count are initialized writable outputs.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfContext>(pointer(context)).GetSelection)(
                pointer(context),
                cookie,
                TF_DEFAULT_SELECTION,
                1,
                &mut selection,
                &mut fetched,
            )
        },
    };
    let raw = selection
        .range
        .as_ref()
        .map_or(std::ptr::null_mut(), Interface::as_raw);
    let range = returned_interface(call, raw).value;
    Returned {
        call,
        value: Selection {
            range,
            style: selection.style,
            fetched,
        },
    }
}

pub(crate) fn set_selection(
    context: &Lease<ITfContext>,
    cookie: u32,
    range: &Lease<ITfRange>,
    style: TF_SELECTIONSTYLE,
) -> Call {
    // SAFETY: this is a borrowed ABI carrier. ManuallyDrop suppresses an implicit host Release;
    // the Lease retains the one real ownership reference throughout the synchronous call.
    let borrowed = unsafe { ITfRange::from_raw(pointer(range)) };
    let selection = TF_SELECTION {
        range: ManuallyDrop::new(Some(borrowed)),
        style,
    };
    // SAFETY: retained context/range and the local SDK selection remain live through the call.
    Call {
        result: unsafe {
            (super::vtable::<ITfContext>(pointer(context)).SetSelection)(
                pointer(context),
                cookie,
                1,
                &selection,
            )
        },
    }
}

pub(crate) fn extent(range: &Lease<ITfRangeACP>) -> Returned<(i32, i32)> {
    let mut start = 0;
    let mut length = 0;
    // SAFETY: range is retained and both output scalars are distinct initialized slots.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfRangeACP>(pointer(range)).GetExtent)(
                pointer(range),
                &mut start,
                &mut length,
            )
        },
    };
    Returned {
        call,
        value: (start, length),
    }
}

pub(crate) fn shift_start(range: &Lease<ITfRange>, cookie: u32, amount: i32) -> Returned<i32> {
    let mut shifted = 0;
    // SAFETY: the range is retained and shifted is distinct initialized output storage.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfRange>(pointer(range)).ShiftStart)(
                pointer(range),
                cookie,
                amount,
                &mut shifted,
                std::ptr::null(),
            )
        },
    };
    Returned {
        call,
        value: shifted,
    }
}
pub(crate) fn shift_end(range: &Lease<ITfRange>, cookie: u32, amount: i32) -> Returned<i32> {
    let mut shifted = 0;
    // SAFETY: the range is retained and shifted is distinct initialized output storage.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfRange>(pointer(range)).ShiftEnd)(
                pointer(range),
                cookie,
                amount,
                &mut shifted,
                std::ptr::null(),
            )
        },
    };
    Returned {
        call,
        value: shifted,
    }
}
pub(crate) fn collapse(range: &Lease<ITfRange>, cookie: u32, anchor: TfAnchor) -> Call {
    // SAFETY: the retained range and cookie are supplied by the currently guarded edit frame.
    Call {
        result: unsafe {
            (super::vtable::<ITfRange>(pointer(range)).Collapse)(pointer(range), cookie, anchor)
        },
    }
}

pub(crate) struct CompositionEntry {
    pub(crate) view: Option<Lease<ITfCompositionView>>,
    pub(crate) fetched: u32,
}
pub(crate) fn next(entries: &Lease<IEnumITfCompositionView>) -> Returned<CompositionEntry> {
    let mut output = std::ptr::null_mut();
    let mut fetched = 0;
    // SAFETY: entries is retained, and both output slots are initialized and distinct.
    let call = Call {
        result: unsafe {
            (super::vtable::<IEnumITfCompositionView>(pointer(entries)).Next)(
                pointer(entries),
                1,
                &mut output,
                &mut fetched,
            )
        },
    };
    let view = returned_interface(call, output).value;
    Returned {
        call,
        value: CompositionEntry { view, fetched },
    }
}

pub(crate) struct Text {
    pub(crate) units: Vec<u16>,
    pub(crate) count: u32,
}
pub(crate) fn text(range: &Lease<ITfRange>, cookie: u32, capacity: usize) -> Returned<Text> {
    let mut units = vec![0; capacity];
    let mut count = 0;
    // SAFETY: the bounded buffer is live, range retained, and count is a distinct initialized output.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfRange>(pointer(range)).GetText)(
                pointer(range),
                cookie,
                0,
                windows_core::PWSTR(units.as_mut_ptr()),
                capacity as u32,
                &mut count,
            )
        },
    };
    units.truncate((count as usize).min(capacity));
    Returned {
        call,
        value: Text { units, count },
    }
}
pub(crate) fn set_text(range: &Lease<ITfRange>, cookie: u32, units: &[u16]) -> Call {
    // SAFETY: the guarded frame retains range and bounded UTF-16 input through this single write.
    Call {
        result: unsafe {
            (super::vtable::<ITfRange>(pointer(range)).SetText)(
                pointer(range),
                cookie,
                0,
                windows_core::PCWSTR(units.as_ptr()),
                units.len() as i32,
            )
        },
    }
}
pub(crate) fn start_composition(
    context: &Lease<ITfContextComposition>,
    cookie: u32,
    range: &Lease<ITfRange>,
    sink: *mut c_void,
) -> Returned<Option<Lease<ITfComposition>>> {
    let mut output = std::ptr::null_mut();
    // SAFETY: context/range are retained and the detached frame retains its own generated sink object.
    let call = Call {
        result: unsafe {
            (super::vtable::<ITfContextComposition>(pointer(context)).StartComposition)(
                pointer(context),
                cookie,
                pointer(range),
                sink,
                &mut output,
            )
        },
    };
    returned_interface(call, output)
}
pub(crate) fn end_composition(composition: &Lease<ITfComposition>, cookie: u32) -> Call {
    // SAFETY: the frame retains and freshly validates its exact owned composition before this call.
    Call {
        result: unsafe {
            (super::vtable::<ITfComposition>(pointer(composition)).EndComposition)(
                pointer(composition),
                cookie,
            )
        },
    }
}

pub(crate) fn capture(
    dependencies: &Dependencies,
    window: HWND,
    host: &HostIdentity,
) -> Returned<Facts> {
    let mut output = Facts::default();
    // SAFETY: the dependency owner retains this typed callback/context, and outputs are complete objects.
    let capture = dependencies.capture.expect("validated field callback");
    let call = Call {
        result: unsafe { capture(dependencies.context, window, host, &mut output) },
    };
    Returned {
        call,
        value: output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_keeps_native_u180e_classification() {
        unsafe extern "C" {
            fn iswctype(unit: u16, mask: u16) -> i32;
        }
        // SAFETY: the UCRT SPACE character-class mask independently supplies the native expectation.
        let expected = unsafe { iswctype(0x180e, 0x8) != 0 };
        assert_eq!(whitespace(0x180e).value, expected);
        for (unit, expected) in [
            (0x20, true),
            (0x09, true),
            (0x0a, true),
            (0x41, false),
            (0, false),
            (0xd800, false),
        ] {
            let output = whitespace(unit);
            assert!(output.call.exact());
            assert_eq!(output.value, expected, "UTF-16 unit {unit:04x}");
        }
    }
}
