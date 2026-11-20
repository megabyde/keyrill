use crate::foreign::{Lease, document, tsf};
use crate::service::ObjectLease;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use windows::Win32::UI::TextServices::*;
use windows_core::{GUID, IUnknown, Interface, Ref, Result, implement};

#[implement(ITfStatusSink, ITfCompartmentEventSink, Agile = false)]
struct Sink {
    revoked: Arc<AtomicBool>,
    _object: ObjectLease,
}

impl ITfStatusSink_Impl for Sink_Impl {
    fn OnStatusChange(&self, _: Ref<'_, ITfContext>, _: u32) -> Result<()> {
        self.revoked.store(true, Ordering::Release);
        Ok(())
    }
}

impl ITfCompartmentEventSink_Impl for Sink_Impl {
    fn OnChange(&self, _: *const GUID) -> Result<()> {
        self.revoked.store(true, Ordering::Release);
        Ok(())
    }
}

pub(crate) struct Watch {
    subscriptions: Vec<(Lease<ITfSource>, u32)>,
    sink: IUnknown,
    revoked: Arc<AtomicBool>,
    pub(crate) uncertain: bool,
}

impl Watch {
    pub(crate) fn new(revoked: Arc<AtomicBool>) -> Self {
        Self {
            subscriptions: Vec::new(),
            sink: Sink {
                revoked: Arc::clone(&revoked),
                _object: ObjectLease::new(),
            }
            .into(),
            revoked,
            uncertain: false,
        }
    }

    fn subscribe<T: Interface>(&mut self, target: &Lease<T>, id: &GUID) -> bool {
        let Ok(source) = target.query::<ITfSource>() else {
            return false;
        };
        let (call, cookie) = tsf::advise(&source, id, self.sink.as_raw());
        self.uncertain |= call.result.is_ok() && (!call.exact() || cookie == TF_INVALID_COOKIE);
        if cookie != TF_INVALID_COOKIE {
            self.subscriptions.push((source, cookie));
        }
        call.exact() && cookie != TF_INVALID_COOKIE && !self.revoked.load(Ordering::Acquire)
    }

    fn compartment<T: Interface>(&mut self, target: &Lease<T>, id: &GUID) -> bool {
        let Ok(manager) = target.query::<ITfCompartmentMgr>() else {
            return false;
        };
        let returned = document::compartment(&manager, id);
        if !returned.call.exact() {
            return false;
        }
        let Some(compartment) = returned.value else {
            return false;
        };
        self.subscribe(&compartment, &ITfCompartmentEventSink::IID)
    }

    pub(crate) fn install(
        &mut self,
        context: &Lease<ITfContext>,
        manager: &Lease<ITfThreadMgr>,
    ) -> bool {
        self.subscribe(context, &ITfStatusSink::IID)
            && self.compartment(context, &GUID_COMPARTMENT_KEYBOARD_DISABLED)
            && self.compartment(context, &GUID_COMPARTMENT_EMPTYCONTEXT)
            && self.compartment(manager, &GUID_COMPARTMENT_KEYBOARD_DISABLED)
    }

    pub(crate) fn drain(&mut self) -> bool {
        let mut exact = true;
        while let Some((source, cookie)) = self.subscriptions.pop() {
            exact &= tsf::unadvise(&source, cookie).exact();
        }
        if !exact {
            self.uncertain = true;
            self.revoked.store(true, Ordering::Release);
        }
        exact
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.drain();
    }
}
