use keyrill_ffi::{
    Engine as NativeEngine, NativeEvent, NativePlan, kr_abi_version, kr_adopt_dictionaries,
    kr_commit, kr_engine_free, kr_engine_new, kr_plan_event, kr_reset, kr_set_automatic_policy,
    kr_set_keyboard,
};
use std::mem::MaybeUninit;
use std::ptr::NonNull;

pub(crate) struct Engine(NonNull<NativeEngine>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Pass,
    Mark,
    Finish,
    Unavailable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disposition {
    Pass,
    Consumed,
    Replan,
}

pub(crate) struct Plan {
    pub(crate) id: u64,
    pub(crate) action: Action,
    pub(crate) disposition: Disposition,
    pub(crate) expected: Vec<u16>,
    pub(crate) mark: Vec<u16>,
}

impl Engine {
    pub(crate) fn adopt(&mut self, ready: &crate::vocabulary_contract::Ready) -> Result<(), u32> {
        let dictionary = |index: usize| {
            ready.sources[index]
                .dictionary
                .as_ref()
                .map_or(std::ptr::null(), |dictionary| dictionary.pointer())
        };
        // SAFETY: immutable snapshots remain retained through this synchronous ABI acquisition.
        accepted(unsafe { kr_adopt_dictionaries(self.0.as_ptr(), dictionary(0), dictionary(1)) })
    }

    pub(crate) fn policy(&mut self, policy: u32) -> Result<(), u32> {
        // SAFETY: the serialized core validates the requested policy against its current state.
        accepted(unsafe { kr_set_automatic_policy(self.0.as_ptr(), policy) })
    }
    pub(crate) fn new() -> Result<Self, u32> {
        let mut pointer = std::ptr::null_mut();
        // SAFETY: pointer is distinct writable storage and transfers one newly allocated handle.
        let status = unsafe { kr_engine_new(kr_abi_version(), &mut pointer) };
        if status != 0 {
            return Err(status);
        }
        NonNull::new(pointer).map(Self).ok_or(1)
    }

    pub(crate) fn configure(&mut self, policy: u32) -> Result<(), u32> {
        // SAFETY: this owner serializes one live engine, with no callbacks or retained borrows.
        unsafe {
            accepted(kr_reset(self.0.as_ptr()))?;
            accepted(kr_set_keyboard(self.0.as_ptr(), 1))?;
            accepted(kr_adopt_dictionaries(
                self.0.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            ))?;
            accepted(kr_set_automatic_policy(self.0.as_ptr(), policy))
        }
    }

    pub(crate) fn reset(&mut self) {
        // SAFETY: this owner has the unique serialized handle. Poison remains permanent in the ABI.
        unsafe { kr_reset(self.0.as_ptr()) };
    }

    pub(crate) fn plan(&mut self, event: &NativeEvent) -> Result<Plan, u32> {
        let mut output = MaybeUninit::<NativePlan>::uninit();
        // SAFETY: this owner serializes the handle, and input/output are complete distinct objects.
        accepted(unsafe { kr_plan_event(self.0.as_ptr(), event, output.as_mut_ptr()) })?;
        // SAFETY: exact ABI OK initializes the complete bounded proposal.
        let plan = unsafe { output.assume_init() };
        let view = plan.view();
        Ok(Plan {
            id: view.plan_id,
            action: match view.action {
                0 => Action::Pass,
                1 => Action::Mark,
                2 => Action::Finish,
                3 => Action::Unavailable,
                _ => return Err(5),
            },
            disposition: match view.disposition {
                0 => Disposition::Pass,
                1 => Disposition::Consumed,
                2 => Disposition::Replan,
                _ => return Err(5),
            },
            expected: view.expected.to_vec(),
            mark: view.mark.to_vec(),
        })
    }

    pub(crate) fn commit(&mut self, id: u64, outcome: u32) -> Result<(), u32> {
        // SAFETY: this owner serializes the live handle and forwards the captured latest-only token.
        accepted(unsafe { kr_commit(self.0.as_ptr(), id, outcome) })
    }
}

fn accepted(status: u32) -> Result<(), u32> {
    if status == 0 { Ok(()) } else { Err(status) }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: this is the sole handle owner and Service survives every outstanding edit frame.
        unsafe { kr_engine_free(self.0.as_ptr()) };
    }
}
