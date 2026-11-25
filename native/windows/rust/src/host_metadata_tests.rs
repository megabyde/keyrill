use super::*;
use crate::com;
use crate::service::Service;

fn frame(current: Rc<dyn Fn() -> bool>) -> Rc<Frame> {
    Rc::new(Frame {
        inputs: RefCell::new(None),
        current,
        entry: Cell::new(Entry::Ready),
        value: Cell::new(Metadata::default()),
        panicked: Cell::new(false),
        _owner: com::create_owned(Service::new()),
    })
}

#[test]
fn callback_is_single_entry() {
    let frame = frame(Rc::new(|| true));
    assert_eq!(frame.run(1), S_OK);
    let observed = frame.value.get();
    assert_eq!(observed.entered, 1);
    assert_eq!(observed.completed, 1);
    assert_eq!(frame.run(2), E_FAIL);
    assert_eq!(frame.value.get(), observed);
}

#[test]
fn expired_callback_never_reads_borrowed_current_flag() {
    let frame = frame(Rc::new(|| {
        panic!("expired callback touched its current flag")
    }));
    frame.entry.set(Entry::Expired);
    let observed = frame.value.get();
    assert_eq!(frame.run(1), E_FAIL);
    assert_eq!(frame.value.get(), observed);
}

#[test]
fn current_loss_takes_precedence_over_native_status() {
    let frame = frame(Rc::new(|| false));
    assert!(!frame.good(Call { result: E_FAIL }));
    assert_eq!(frame.value.get().reason, Reason::Stale);
    assert_eq!(frame.value.get().result, 0);
}

#[test]
fn native_status_requires_exact_success() {
    let frame = frame(Rc::new(|| true));
    assert!(!frame.good(Call { result: S_FALSE }));
    assert_eq!(frame.value.get().reason, Reason::Native);
    assert_eq!(frame.value.get().result, S_FALSE.0 as u32);
}

#[implement(ITfEditSession, Agile = false)]
struct ReturnedObject(Rc<Cell<u32>>);

impl ITfEditSession_Impl for ReturnedObject_Impl {
    fn DoEditSession(&self, _: u32) -> Result<()> {
        Ok(())
    }
}

impl Drop for ReturnedObject {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn normally_returned_interface_is_retired_after_nonexact_status() {
    let frame = frame(Rc::new(|| true));
    let drops = Rc::new(Cell::new(0));
    let object: ITfEditSession = ReturnedObject(Rc::clone(&drops)).into();
    // SAFETY: the generated object transfers exactly its one live COM reference to this lease.
    let lease = unsafe { foreign::adopt::<ITfEditSession>(object.into_raw()) }.unwrap();
    let output = frame.required(|| Returned {
        call: Call { result: S_FALSE },
        value: Some(lease),
    });
    assert!(output.is_none());
    assert_eq!(drops.get(), 1);
    assert_eq!(frame.value.get().reason, Reason::Native);
}

#[test]
fn generated_callback_rejects_wrong_thread_before_touching_frame() {
    let frame = frame(Rc::new(|| {
        panic!("wrong-thread callback touched its frame")
    }));
    let session: ITfEditSession = MetadataSession {
        // SAFETY: GetCurrentThreadId has no memory preconditions.
        thread: unsafe { GetCurrentThreadId() },
        frame: Rc::clone(&frame),
    }
    .into();
    let address = session.as_raw() as usize;
    let result = std::thread::spawn(move || {
        let pointer = address as *mut std::ffi::c_void;
        // SAFETY: the originating thread retains the generated COM object until join. This
        // deliberately invalid thread schedule may only read the callback's immutable thread ID.
        unsafe {
            let vtable = *(pointer as *const *const ITfEditSession_Vtbl);
            ((*vtable).DoEditSession)(pointer, 1)
        }
    })
    .join()
    .unwrap();
    assert_eq!(result, RPC_E_WRONG_THREAD);
    assert_eq!(frame.value.get(), Metadata::default());
}

#[test]
fn callback_panic_is_contained_and_has_a_distinct_receipt() {
    let frame = frame(Rc::new(|| panic!("metadata current callback panic")));
    let session: ITfEditSession = MetadataSession {
        // SAFETY: GetCurrentThreadId has no memory preconditions.
        thread: unsafe { GetCurrentThreadId() },
        frame: Rc::clone(&frame),
    }
    .into();
    // SAFETY: this is a live generated object on its owning thread with a scalar edit cookie.
    let result = unsafe { session.DoEditSession(1) };
    assert_eq!(result.unwrap_err().code(), E_UNEXPECTED);
    assert!(frame.panicked.get());
    assert_eq!(frame.value.get().reason, Reason::Exception);
    assert_eq!(frame.value.get().completed, 1);
}
