use super::schedule::HostCapsule;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    ffi::c_void,
    rc::{Rc, Weak},
};
use windows::Win32::{
    Foundation::{HWND, S_OK},
    UI::TextServices::*,
};
use windows_core::{HRESULT, IUnknown};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: i32,
    pub length: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Start,
    Text,
    End,
    Selection,
}
#[derive(Clone, Debug)]
pub struct Operation {
    pub request: u32,
    pub composition: u32,
    pub effect: Effect,
    pub span: Span,
    pub replacement: Vec<u16>,
}
pub struct Active {
    pub pointer: *mut c_void,
    pub span: Rc<Cell<Span>>,
    pub serial: u32,
}
pub struct Document {
    pub host: Rc<HostCapsule>,
    pub window: HWND,
    pub view_window: Cell<HWND>,
    pub request_flags: Cell<TF_CONTEXT_EDIT_CONTEXT_FLAGS>,
    pub context: Cell<*mut c_void>,
    pub text: RefCell<Vec<u16>>,
    pub selection: Cell<Span>,
    pub style: Cell<TF_SELECTIONSTYLE>,
    pub status: Cell<u32>,
    pub scope: Cell<InputScope>,
    pub scope_type: Cell<windows::Win32::System::Variant::VARENUM>,
    pub scope_value_present: Cell<bool>,
    pub scope_second: Cell<InputScope>,
    pub excluded_scope: Cell<Option<Span>>,
    pub compartment_types: Cell<[windows::Win32::System::Variant::VARENUM; 2]>,
    pub compartment_results: Cell<[HRESULT; 2]>,
    pub disabled: Cell<bool>,
    pub empty_disabled: Cell<bool>,
    pub queried_empty: Cell<bool>,
    pub foreign: Cell<bool>,
    pub reject_composition: Cell<bool>,
    pub invoke: Cell<bool>,
    pub repeat: Cell<bool>,
    pub retain_callback: Cell<bool>,
    pub retained: RefCell<Option<IUnknown>>,
    pub active: RefCell<Option<Active>>,
    pub request: Cell<u32>,
    pub starts: Cell<u32>,
    pub text_reads: RefCell<Vec<Span>>,
    pub finish_selection: Cell<Option<(u32, Span, TF_SELECTIONSTYLE)>>,
    pub reads: Cell<u32>,
    pub writes: Cell<u32>,
    pub ends: Cell<u32>,
    pub operations: RefCell<Vec<Operation>>,
    pub results: RefCell<BTreeMap<&'static str, HRESULT>>,
    pub nulls: RefCell<Vec<&'static str>>,
    pub selection_fetched: Cell<u32>,
    pub scope_count: Cell<u32>,
    pub acp: Cell<bool>,
    pub keep_composition: Cell<bool>,
    pub clones: RefCell<Vec<Weak<Cell<Span>>>>,
    pub move_clones: Cell<bool>,
    pub shift_start_limit: Cell<i32>,
    pub shift_end_limit: Cell<i32>,
    pub range_context: Cell<*mut c_void>,
}
impl Document {
    pub fn new(host: Rc<HostCapsule>, window: usize) -> Rc<Self> {
        Rc::new(Self {
            host,
            window: HWND(window as *mut _),
            view_window: Cell::new(HWND(window as *mut _)),
            request_flags: Cell::new(TF_ES_SYNC | TF_ES_READWRITE),
            context: Cell::new(std::ptr::null_mut()),
            text: RefCell::new("prefix suffix".encode_utf16().collect()),
            selection: Cell::new(Span {
                start: 7,
                length: 0,
            }),
            style: Cell::new(TF_SELECTIONSTYLE {
                ase: TF_AE_NONE,
                fInterimChar: false.into(),
            }),
            status: Cell::new(0),
            scope: Cell::new(IS_DEFAULT),
            scope_type: Cell::new(windows::Win32::System::Variant::VT_UNKNOWN),
            scope_value_present: Cell::new(true),
            scope_second: Cell::new(IS_TEXT),
            excluded_scope: Cell::new(None),
            compartment_types: Cell::new([windows::Win32::System::Variant::VT_I4; 2]),
            compartment_results: Cell::new([S_OK; 2]),
            disabled: Cell::new(false),
            empty_disabled: Cell::new(false),
            queried_empty: Cell::new(false),
            foreign: Cell::new(false),
            reject_composition: Cell::new(false),
            invoke: Cell::new(true),
            repeat: Cell::new(false),
            retain_callback: Cell::new(false),
            retained: RefCell::new(None),
            active: RefCell::new(None),
            request: Cell::new(0),
            starts: Cell::new(0),
            text_reads: RefCell::new(Vec::new()),
            finish_selection: Cell::new(None),
            reads: Cell::new(0),
            writes: Cell::new(0),
            ends: Cell::new(0),
            operations: RefCell::new(Vec::new()),
            results: RefCell::new(BTreeMap::new()),
            nulls: RefCell::new(Vec::new()),
            selection_fetched: Cell::new(1),
            scope_count: Cell::new(1),
            acp: Cell::new(true),
            keep_composition: Cell::new(false),
            clones: RefCell::new(Vec::new()),
            move_clones: Cell::new(false),
            shift_start_limit: Cell::new(i32::MAX),
            shift_end_limit: Cell::new(i32::MAX),
            range_context: Cell::new(std::ptr::null_mut()),
        })
    }
    pub fn result(&self, name: &'static str) -> HRESULT {
        self.results.borrow().get(name).copied().unwrap_or(S_OK)
    }
    pub fn null(&self, name: &'static str) -> bool {
        self.nulls.borrow().contains(&name)
    }
    pub fn native_text(&self, text: &[u16]) {
        let span = self.selection.get();
        self.text.borrow_mut().splice(
            span.start as usize..(span.start + span.length) as usize,
            text.iter().copied(),
        );
        self.selection.set(Span {
            start: span.start + text.len() as i32,
            length: 0,
        });
    }
    pub fn effect(&self, effect: Effect, span: Span, replacement: Vec<u16>) {
        self.operations.borrow_mut().push(Operation {
            request: self.request.get(),
            composition: self.starts.get(),
            effect,
            span,
            replacement,
        });
    }
    pub fn drain_retained(&self) {
        let old = self.retained.borrow_mut().take();
        drop(old);
    }
}
