#![allow(non_snake_case, unused_variables)]
mod composition;
mod context;
mod manager;
mod profiles;
mod range;
mod source;
use super::{com_slots::*, document::*, schedule::HostCapsule};
use composition::*;
use context::*;
use manager::*;
use profiles::*;
use range::*;
use source::*;
pub use source::{DIAGNOSTIC_V1, DIAGNOSTIC_V2, Diagnostic, source_profile};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    rc::Rc,
};
use windows::Win32::{
    Foundation::*,
    System::{Com::CoTaskMemAlloc, Variant::*},
    UI::{Input::KeyboardAndMouse::HKL, TextServices::*},
};
use windows_core::{GUID, HRESULT, IUnknown, Interface};
pub const SERVICE_ID: GUID = GUID::from_u128(0x1a746212_84db_4acf_88b3_caaa26352774);
pub const PROFILE_EN: GUID = GUID::from_u128(0x0d52a994_9eb5_44ab_b18f_603dc5f69671);
#[derive(Clone, Copy, Debug)]
pub struct Registration {
    pub client: u32,
    pub id: GUID,
    pub key: TF_PRESERVEDKEY,
}
pub struct Profile {
    pub language: Cell<u16>,
    pub enabled: Cell<windows_core::BOOL>,
    pub language_calls: Cell<u32>,
    pub activation_calls: Cell<u32>,
    pub language_effect: Cell<bool>,
    pub activation_effect: Cell<bool>,
    pub profile: Cell<TF_INPUTPROCESSORPROFILE>,
    pub substitute: Cell<HKL>,
    pub available: [Cell<TF_INPUTPROCESSORPROFILE>; 2],
    pub bindings: [Cell<Option<HKL>>; 2],
    pub results: RefCell<std::collections::BTreeMap<&'static str, HRESULT>>,
    pub nulls: RefCell<Vec<&'static str>>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            language: Cell::new(0x409),
            enabled: Cell::new(true.into()),
            language_calls: Cell::new(0),
            activation_calls: Cell::new(0),
            language_effect: Cell::new(true),
            activation_effect: Cell::new(true),
            profile: Cell::new(TF_INPUTPROCESSORPROFILE {
                dwProfileType: TF_PROFILETYPE_INPUTPROCESSOR,
                langid: 0x409,
                clsid: SERVICE_ID,
                guidProfile: PROFILE_EN,
                catid: GUID_TFCAT_TIP_KEYBOARD,
                hkl: HKL(0xe0200409usize as *mut _),
                dwFlags: TF_IPP_FLAG_ACTIVE,
                ..Default::default()
            }),
            substitute: Cell::new(HKL(0x04090409usize as *mut _)),
            available: std::array::from_fn(|index| Cell::new(source_profile(index))),
            bindings: std::array::from_fn(|_| Cell::new(None)),
            results: RefCell::new(Default::default()),
            nulls: RefCell::new(vec![]),
        }
    }
}
pub struct AdvisedKeySink {
    pub client: u32,
    pub foreground: windows_core::BOOL,
    pub sink: ITfKeyEventSink,
}
pub struct Manager {
    pub thread_sink: RefCell<Option<IUnknown>>,
    pub key_sink: RefCell<Option<Rc<AdvisedKeySink>>>,
    pub diagnostic_advises: Cell<u32>,
    pub diagnostic_unadvises: Cell<u32>,
    pub disabled: Cell<bool>,
    pub compartment_type: Cell<VARENUM>,
    pub compartment_result: Cell<HRESULT>,
    pub nulls: RefCell<Vec<&'static str>>,
    pub preserve_calls: RefCell<Vec<Registration>>,
    pub preserved_keys: RefCell<Vec<Registration>>,
    pub unpreserved_keys: RefCell<Vec<Registration>>,
    pub preserve_results: [Cell<HRESULT>; 3],
    pub unpreserve_results: [Cell<HRESULT>; 3],
    pub sink_results: [Cell<HRESULT>; 3],
    pub unsink_results: [Cell<HRESULT>; 3],
    pub advises: [Cell<u32>; 3],
    pub unadvises: [Cell<u32>; 3],

    pub top: RefCell<IUnknown>,
    pub host: Rc<HostCapsule>,
    pub diagnostic: RefCell<Option<IUnknown>>,
    pub results: RefCell<std::collections::BTreeMap<&'static str, HRESULT>>,
}
impl Manager {
    pub fn new(host: Rc<HostCapsule>, top: *mut c_void) -> Rc<Self> {
        Rc::new(Self {
            thread_sink: RefCell::new(None),
            key_sink: RefCell::new(None),
            diagnostic_advises: Cell::new(0),
            diagnostic_unadvises: Cell::new(0),
            disabled: Cell::new(false),
            compartment_type: Cell::new(VT_I4),
            compartment_result: Cell::new(S_OK),
            nulls: RefCell::new(vec![]),
            preserve_calls: RefCell::new(vec![]),
            preserved_keys: RefCell::new(vec![]),
            unpreserved_keys: RefCell::new(vec![]),
            preserve_results: std::array::from_fn(|_| Cell::new(S_OK)),
            unpreserve_results: std::array::from_fn(|_| Cell::new(S_OK)),
            sink_results: std::array::from_fn(|_| Cell::new(S_OK)),
            unsink_results: std::array::from_fn(|_| Cell::new(S_OK)),
            advises: std::array::from_fn(|_| Cell::new(0)),
            unadvises: std::array::from_fn(|_| Cell::new(0)),
            top: RefCell::new(unsafe { IUnknown::from_raw(retain(top)) }),
            host,
            diagnostic: RefCell::new(None),
            results: RefCell::new(Default::default()),
        })
    }
}
pub enum Kind {
    Document(Rc<Document>),
    Range(Rc<Document>, Rc<Cell<Span>>),
    Composition(Rc<Document>, Rc<Cell<Span>>),
    Enumeration(Rc<Document>, Cell<u32>, bool),
    Manager(Rc<Manager>),
    Profiles(Rc<Profile>),
    Diagnostic(Rc<Diagnostic>),
}
pub struct Provider {
    pub host: Rc<HostCapsule>,
    pub references: Cell<u32>,
    pub kind: Kind,
    pub context_owner: Option<IUnknown>,
    pub ITfContext: Slot<ITfContext_Vtbl>,
    pub ITfInputProcessorProfiles: Slot<ITfInputProcessorProfiles_Vtbl>,
    pub ITfFunctionProvider: Slot<ITfFunctionProvider_Vtbl>,
    pub diagnostic_v1: Slot<ReaderTable>,
    pub diagnostic_v2: Slot<ReaderTable>,
    pub ITfContextComposition: Slot<ITfContextComposition_Vtbl>,
    pub ITfContextView: Slot<ITfContextView_Vtbl>,
    pub ITfProperty: Slot<ITfProperty_Vtbl>,
    pub ITfInputScope: Slot<ITfInputScope_Vtbl>,
    pub ITfCompartmentMgr: Slot<ITfCompartmentMgr_Vtbl>,
    pub ITfCompartment: Slot<ITfCompartment_Vtbl>,
    pub ITfRangeACP: Slot<ITfRangeACP_Vtbl>,
    pub ITfComposition: Slot<ITfComposition_Vtbl>,
    pub composition_alias: Slot<ITfComposition_Vtbl>,
    pub ITfCompositionView: Slot<ITfCompositionView_Vtbl>,
    pub IEnumITfCompositionView: Slot<IEnumITfCompositionView_Vtbl>,
    pub ITfThreadMgr: Slot<ITfThreadMgr_Vtbl>,
    pub ITfDocumentMgr: Slot<ITfDocumentMgr_Vtbl>,
    pub ITfSource: Slot<ITfSource_Vtbl>,
    pub ITfSourceSingle: Slot<ITfSourceSingle_Vtbl>,
    pub ITfKeystrokeMgr: Slot<ITfKeystrokeMgr_Vtbl>,
    pub ITfInputProcessorProfileMgr: Slot<ITfInputProcessorProfileMgr_Vtbl>,
    pub ITfInputProcessorProfileSubstituteLayout:
        Slot<ITfInputProcessorProfileSubstituteLayout_Vtbl>,
}
impl Provider {
    pub fn create(host: Rc<HostCapsule>, kind: Kind, id: GUID) -> *mut c_void {
        let context_owner = match &kind {
            Kind::Range(d, _) | Kind::Composition(d, _) | Kind::Enumeration(d, _, _) => {
                Some(unsafe { IUnknown::from_raw(retain(d.context.get())) })
            }
            _ => None,
        };
        let mut p = Box::new(Self {
            host,
            references: Cell::new(1),
            kind,
            context_owner,
            ITfContext: Slot::new(&ITFCONTEXT_TABLE),
            ITfInputProcessorProfiles: Slot::new(&ITFINPUTPROCESSORPROFILES_TABLE),
            ITfFunctionProvider: Slot::new(&ITFFUNCTIONPROVIDER_TABLE),
            diagnostic_v1: Slot::new(&READER_V1),
            diagnostic_v2: Slot::new(&READER_V2),
            ITfContextComposition: Slot::new(&ITFCONTEXTCOMPOSITION_TABLE),
            ITfContextView: Slot::new(&ITFCONTEXTVIEW_TABLE),
            ITfProperty: Slot::new(&ITFPROPERTY_TABLE),
            ITfInputScope: Slot::new(&ITFINPUTSCOPE_TABLE),
            ITfCompartmentMgr: Slot::new(&ITFCOMPARTMENTMGR_TABLE),
            ITfCompartment: Slot::new(&ITFCOMPARTMENT_TABLE),
            ITfRangeACP: Slot::new(&ITFRANGEACP_TABLE),
            ITfComposition: Slot::new(&ITFCOMPOSITION_TABLE),
            composition_alias: Slot::new(&ITFCOMPOSITION_TABLE),
            ITfCompositionView: Slot::new(&ITFCOMPOSITIONVIEW_TABLE),
            IEnumITfCompositionView: Slot::new(&IENUMITFCOMPOSITIONVIEW_TABLE),
            ITfThreadMgr: Slot::new(&ITFTHREADMGR_TABLE),
            ITfDocumentMgr: Slot::new(&ITFDOCUMENTMGR_TABLE),
            ITfSource: Slot::new(&ITFSOURCE_TABLE),
            ITfSourceSingle: Slot::new(&ITFSOURCESINGLE_TABLE),
            ITfKeystrokeMgr: Slot::new(&ITFKEYSTROKEMGR_TABLE),
            ITfInputProcessorProfileMgr: Slot::new(&ITFINPUTPROCESSORPROFILEMGR_TABLE),
            ITfInputProcessorProfileSubstituteLayout: Slot::new(
                &ITFINPUTPROCESSORPROFILESUBSTITUTELAYOUT_TABLE,
            ),
        });
        let address = &*p as *const Self;
        p.ITfContext.owner = address;
        p.ITfInputProcessorProfiles.owner = address;
        p.ITfFunctionProvider.owner = address;
        p.diagnostic_v1.owner = address;
        p.diagnostic_v2.owner = address;
        p.ITfContextComposition.owner = address;
        p.ITfContextView.owner = address;
        p.ITfProperty.owner = address;
        p.ITfInputScope.owner = address;
        p.ITfCompartmentMgr.owner = address;
        p.ITfCompartment.owner = address;
        p.ITfRangeACP.owner = address;
        p.ITfComposition.owner = address;
        p.composition_alias.owner = address;
        p.ITfCompositionView.owner = address;
        p.IEnumITfCompositionView.owner = address;
        p.ITfThreadMgr.owner = address;
        p.ITfDocumentMgr.owner = address;
        p.ITfSource.owner = address;
        p.ITfSourceSingle.owner = address;
        p.ITfKeystrokeMgr.owner = address;
        p.ITfInputProcessorProfileMgr.owner = address;
        p.ITfInputProcessorProfileSubstituteLayout.owner = address;
        p.host.live_objects.set(p.host.live_objects.get() + 1);
        let out = p.interface(id);
        assert!(!out.is_null());
        let _ = Box::into_raw(p);
        out
    }
    pub fn interface(&self, id: GUID) -> *mut c_void {
        let allowed = match &self.kind {
            Kind::Document(_) => [
                ITfContext::IID,
                ITfSource::IID,
                ITfContextComposition::IID,
                ITfContextView::IID,
                ITfProperty::IID,
                ITfReadOnlyProperty::IID,
                ITfInputScope::IID,
                ITfCompartmentMgr::IID,
                ITfCompartment::IID,
            ]
            .contains(&id),
            Kind::Range(d, _) => id == ITfRange::IID || (id == ITfRangeACP::IID && d.acp.get()),
            Kind::Composition(_, _) => id == ITfComposition::IID || id == ITfCompositionView::IID,
            Kind::Enumeration(_, _, _) => id == IEnumITfCompositionView::IID,
            Kind::Manager(_) => [
                ITfThreadMgr::IID,
                ITfDocumentMgr::IID,
                ITfSource::IID,
                ITfSourceSingle::IID,
                ITfKeystrokeMgr::IID,
                ITfCompartmentMgr::IID,
                ITfCompartment::IID,
            ]
            .contains(&id),
            Kind::Diagnostic(d) => {
                id == ITfFunctionProvider::IID
                    || id == DIAGNOSTIC_V1
                    || (id == DIAGNOSTIC_V2 && !d.legacy_only.get())
            }
            Kind::Profiles(_) => {
                id == ITfInputProcessorProfiles::IID
                    || id == ITfInputProcessorProfileMgr::IID
                    || id == ITfInputProcessorProfileSubstituteLayout::IID
            }
        };
        let id = if id == IUnknown::IID {
            match self.kind {
                Kind::Document(_) => ITfContext::IID,
                Kind::Range(_, _) => ITfRangeACP::IID,
                Kind::Composition(_, _) => ITfComposition::IID,
                Kind::Enumeration(_, _, _) => IEnumITfCompositionView::IID,
                Kind::Manager(_) => ITfThreadMgr::IID,
                Kind::Profiles(_) => ITfInputProcessorProfileMgr::IID,
                Kind::Diagnostic(_) => ITfFunctionProvider::IID,
            }
        } else if allowed {
            id
        } else {
            return std::ptr::null_mut();
        };
        if id == DIAGNOSTIC_V1 {
            return (&self.diagnostic_v1 as *const Slot<ReaderTable>)
                .cast_mut()
                .cast();
        }
        if id == DIAGNOSTIC_V2 {
            return (&self.diagnostic_v2 as *const Slot<ReaderTable>)
                .cast_mut()
                .cast();
        }
        if id == ITfFunctionProvider::IID {
            return (&self.ITfFunctionProvider as *const Slot<ITfFunctionProvider_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfInputProcessorProfiles::IID {
            return (&self.ITfInputProcessorProfiles
                as *const Slot<ITfInputProcessorProfiles_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfContext::IID {
            return (&self.ITfContext as *const Slot<ITfContext_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfContextComposition::IID {
            return (&self.ITfContextComposition as *const Slot<ITfContextComposition_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfContextView::IID {
            return (&self.ITfContextView as *const Slot<ITfContextView_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfProperty::IID {
            return (&self.ITfProperty as *const Slot<ITfProperty_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfReadOnlyProperty::IID {
            return (&self.ITfProperty as *const Slot<ITfProperty_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfInputScope::IID {
            return (&self.ITfInputScope as *const Slot<ITfInputScope_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfCompartmentMgr::IID {
            return (&self.ITfCompartmentMgr as *const Slot<ITfCompartmentMgr_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfCompartment::IID {
            return (&self.ITfCompartment as *const Slot<ITfCompartment_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfRange::IID {
            return (&self.ITfRangeACP as *const Slot<ITfRangeACP_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfRangeACP::IID {
            return (&self.ITfRangeACP as *const Slot<ITfRangeACP_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfComposition::IID {
            return (&self.ITfComposition as *const Slot<ITfComposition_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfCompositionView::IID {
            return (&self.ITfCompositionView as *const Slot<ITfCompositionView_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == IEnumITfCompositionView::IID {
            return (&self.IEnumITfCompositionView as *const Slot<IEnumITfCompositionView_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfThreadMgr::IID {
            return (&self.ITfThreadMgr as *const Slot<ITfThreadMgr_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfDocumentMgr::IID {
            return (&self.ITfDocumentMgr as *const Slot<ITfDocumentMgr_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfSource::IID {
            return (&self.ITfSource as *const Slot<ITfSource_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfSourceSingle::IID {
            return (&self.ITfSourceSingle as *const Slot<ITfSourceSingle_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfKeystrokeMgr::IID {
            return (&self.ITfKeystrokeMgr as *const Slot<ITfKeystrokeMgr_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfInputProcessorProfileMgr::IID {
            return (&self.ITfInputProcessorProfileMgr
                as *const Slot<ITfInputProcessorProfileMgr_Vtbl>)
                .cast_mut()
                .cast();
        }
        if id == ITfInputProcessorProfileSubstituteLayout::IID {
            return (&self.ITfInputProcessorProfileSubstituteLayout
                as *const Slot<ITfInputProcessorProfileSubstituteLayout_Vtbl>)
                .cast_mut()
                .cast();
        }
        std::ptr::null_mut()
    }
    pub fn document(&self) -> &Rc<Document> {
        match &self.kind {
            Kind::Document(d)
            | Kind::Range(d, _)
            | Kind::Composition(d, _)
            | Kind::Enumeration(d, _, _) => d,
            _ => panic!("not a document provider"),
        }
    }
    pub fn span(&self) -> Rc<Cell<Span>> {
        match &self.kind {
            Kind::Range(_, s) | Kind::Composition(_, s) => Rc::clone(s),
            _ => panic!("not a range"),
        }
    }
    pub fn diagnostic(&self) -> &Diagnostic {
        match &self.kind {
            Kind::Diagnostic(d) => d,
            _ => panic!("not diagnostic"),
        }
    }
    pub fn profile(&self) -> &Profile {
        match &self.kind {
            Kind::Profiles(p) => p,
            _ => panic!("not profiles"),
        }
    }
    pub fn manager(&self) -> &Rc<Manager> {
        match &self.kind {
            Kind::Manager(m) => m,
            _ => panic!("not manager"),
        }
    }
    pub fn hit(&self, name: &'static str) {
        self.host.hit(name);
        if name == "QI" && matches!(self.kind, Kind::Profiles(_)) {
            self.host.hit("profiles.QI");
        }
        let qualified = match (name, &self.kind) {
            ("QI", Kind::Document(_)) => Some("context.QI"),
            ("QI", Kind::Range(_, _)) => Some("range.QI"),
            ("QI", Kind::Composition(_, _)) => Some("composition.QI"),
            ("QI", Kind::Manager(_)) => Some("manager.QI"),
            ("QI", Kind::Profiles(_)) => Some("native.identity"),
            ("AddRef.before", Kind::Profiles(_)) => Some("native.addref"),
            ("Release.before", Kind::Profiles(_)) => Some("native.release"),
            ("AddRef.before", Kind::Diagnostic(_)) => Some("addref"),
            ("Release.before", Kind::Diagnostic(_)) => Some("release"),
            ("Next", _) => Some("enumerator.Next"),
            ("GetFocus", _) => Some("manager.GetFocus"),
            ("GetCompartment", Kind::Manager(_)) => Some("thread.GetCompartment"),
            ("compartment.GetValue", Kind::Manager(_)) => Some("thread.GetValue"),
            _ => None,
        };
        if let Some(name) = qualified {
            self.host.hit(name);
        }
    }
    pub fn result(&self, name: &'static str) -> HRESULT {
        match &self.kind {
            Kind::Diagnostic(_) => S_OK,
            Kind::Profiles(state) => state.results.borrow().get(name).copied().unwrap_or(S_OK),
            Kind::Manager(m) => m.results.borrow().get(name).copied().unwrap_or(S_OK),
            _ => self.document().result(name),
        }
    }
    pub fn null(&self, name: &'static str) -> bool {
        match &self.kind {
            Kind::Diagnostic(_) => false,
            Kind::Profiles(state) => state.nulls.borrow().contains(&name),
            Kind::Manager(m) => m.nulls.borrow().contains(&name),
            _ => self.document().null(name),
        }
    }
    pub fn range(&self, span: Rc<Cell<Span>>) -> *mut c_void {
        Self::create(
            Rc::clone(&self.host),
            Kind::Range(Rc::clone(self.document()), span),
            ITfRange::IID,
        )
    }
    pub fn own(&self, id: GUID) -> *mut c_void {
        unsafe { retain(self.interface(id)) }
    }
}
impl Drop for Provider {
    fn drop(&mut self) {
        if let Kind::Composition(d, _) = &self.kind {
            let pointer = self.interface(ITfComposition::IID);
            let remove = d
                .active
                .borrow()
                .as_ref()
                .is_some_and(|a| a.pointer == pointer);
            if remove {
                d.active.borrow_mut().take();
            }
        }
        self.host.live_objects.set(self.host.live_objects.get() - 1);
    }
}
pub const BOUNDARIES: &[&str] = &[
    "AdviseAdmission",
    "UnadviseAdmission",
    "UnadviseThreadSink",
    "UnadviseProfileSink",
    "thread.GetCompartment",
    "thread.GetValue",
    "identity",
    "interface",
    "function",
    "copy",
    "addref",
    "release",
    "native.identity",
    "native.addref",
    "native.release",
    "language.read",
    "profile.read",
    "target.read",
    "substitute.en",
    "substitute.ru",
    "enabled.read",
    "language.change",
    "profile.activate",
    "GetCurrentLanguage",
    "ChangeCurrentLanguage",
    "IsEnabledLanguageProfile",
    "Layouts",
    "context.QI",
    "range.QI",
    "range.ACP",
    "composition.QI",
    "manager.QI",
    "profiles.QI",
    "enumerator.Next",
    "manager.GetFocus",
    "Activate",
    "ActivateProfile",
    "AddRef.after",
    "AddRef.before",
    "AdjustForInsert",
    "AdviseKeyEventSink",
    "AdviseSingleSink",
    "AdviseSink",
    "AssociateFocus",
    "Clear",
    "ClearCompartment",
    "Clone",
    "Clone.after",
    "Collapse",
    "CompareEnd",
    "CompareStart",
    "CreateContext",
    "CreateDocumentMgr",
    "CreateRangeBackup",
    "Deactivate",
    "DeactivateProfile",
    "EndComposition",
    "EndComposition.after",
    "EnumCompartments",
    "EnumCompositions",
    "EnumContexts",
    "EnumDocumentMgrs",
    "EnumFunctionProviders",
    "EnumProfiles",
    "EnumProperties",
    "EnumRanges",
    "EnumViews",
    "FindComposition",
    "FindRange",
    "GetActiveProfile",
    "GetActiveView",
    "GetAppProperty",
    "GetBase",
    "GetCompartment",
    "GetContext",
    "GetDocumentMgr",
    "GetEmbedded",
    "GetEnd",
    "GetExtent",
    "GetFocus",
    "GetForeground",
    "GetFormattedText",
    "GetFunctionProvider",
    "GetGlobalCompartment",
    "GetGravity",
    "GetInputScopes",
    "GetOwnerClsid",
    "GetPhrase",
    "GetPreservedKey",
    "GetPreservedKeyDescription",
    "GetProfile",
    "GetProperty",
    "GetRange",
    "GetRangeFromPoint",
    "GetRegularExpression",
    "GetSRGS",
    "GetScreenExt",
    "GetSelection",
    "GetStart",
    "GetStatus",
    "GetSubstituteKeyboardLayout",
    "GetText",
    "GetText.after",
    "GetTextExt",
    "GetTop",
    "GetType",
    "GetWnd",
    "GetXML",
    "InWriteSession",
    "InsertEmbedded",
    "IsEmpty",
    "IsEqualEnd",
    "IsEqualStart",
    "IsPreservedKey",
    "IsThreadFocus",
    "KeyDown",
    "KeyUp",
    "Next",
    "Pop",
    "PreserveKey",
    "Push",
    "capture",
    "QI",
    "QI.after",
    "RegisterProfile",
    "Release.after",
    "Release.before",
    "Release.final",
    "ReleaseInputProcessor",
    "RequestEditSession",
    "RequestEditSession.after",
    "Reset",
    "SetExtent",
    "SetFocus",
    "SetGravity",
    "SetPreservedKeyDescription",
    "SetSelection",
    "SetSelection.after",
    "SetText",
    "SetText.before",
    "SetText.after",
    "SetValue",
    "SetValueStore",
    "ShiftEnd",
    "ShiftEnd.after",
    "ShiftEndRegion",
    "ShiftEndToRange",
    "ShiftStart",
    "ShiftStart.after",
    "ShiftStartRegion",
    "ShiftStartToRange",
    "SimulatePreservedKey",
    "Skip",
    "StartComposition",
    "StartComposition.after",
    "TakeOwnership",
    "TestKeyDown",
    "TestKeyUp",
    "TrackProperties",
    "UnadviseKeyEventSink",
    "UnadviseSingleSink",
    "UnadviseSink",
    "UnpreserveKey",
    "UnregisterProfile",
    "compartment.GetValue",
    "range.GetContext",
    "scope.GetValue",
];
