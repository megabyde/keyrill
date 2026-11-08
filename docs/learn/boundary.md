# The C boundary

Rust and Objective-C++ can't call each other directly. They meet at the one interface both
understand: C. This chapter is about that meeting point, what the compiler stops proving there, and
what the code does in its place.

## Why C, and what an ABI is

*Goal: know what an ABI and an FFI are, and which files form Keyrill's.*

When a compiled program calls a function in another compiled library, both sides must agree on
details no source code shows: which machine registers carry the arguments (the **calling
convention**), the exact name the function has in the compiled file (its **symbol**, which the
**linker**, the tool that joins compiled files into one program, matches between them), and the byte
layout of every struct passed across. That agreement is an
[application binary interface](https://en.wikipedia.org/wiki/Application_binary_interface)
(**ABI**). Every language can speak C's ABI, which is why it serves as the common language here.

A **foreign function interface** (**FFI**) is the part of a language that calls, or is called by,
code written in another one. Python has [`ctypes`](https://docs.python.org/3/library/ctypes.html)
and cffi for it, Node.js has [Node-API](https://nodejs.org/api/n-api.html), and Rust has
`extern "C"` functions, described in the [Rustonomicon](https://doc.rust-lang.org/nomicon/ffi.html).
Keyrill's FFI has two halves:

- the crate `keyrill-ffi`, compiled into a **static library**, a file of compiled code that the
  linker copies into the macOS program;
- the C **header** `include/keyrill.h`, a file of declarations that C, C++ and Objective-C++ code
  includes so the compiler knows the names, types and struct layouts on the other side.

> [!TIP]
> **Remember.** An ABI is the machine-level agreement between compiled code. Keyrill's is C's:
> `keyrill-ffi` on the Rust side, `include/keyrill.h` on the C side.

## Where `unsafe` lives

*Goal: read an `unsafe` block and the comment that must justify it.*

The core crate forbids `unsafe` outright
([Rust in the core](rust.md#modules-documentation-and-lints)), so every `unsafe` block in the
project is in `keyrill-ffi`. The word is easy to misread. `unsafe` switches no check off: the borrow
checker and the type checker run inside an `unsafe` block as everywhere else. It permits five extra
operations, of which this crate uses two: reading through a **raw pointer** (`*mut T` or `*const T`,
a plain C address with no lifetime and no guarantee that it points anywhere valid) and calling an
`unsafe fn`.

Getting either wrong is **undefined behavior**: the program may crash, corrupt memory or seem to
work, and nothing promises which. Python reaches the same territory when `ctypes` is handed a bad
address and the interpreter crashes. So an `unsafe` block records that a *person*, not the compiler,
now carries a proof, and every block here has a `// SAFETY:` comment that states the argument, as
the [Rust reference](https://doc.rust-lang.org/reference/unsafety.html) and the
[Book's chapter on unsafe Rust](https://doc.rust-lang.org/book/ch20-01-unsafe-rust.html) describe.

An `unsafe fn` is a function with a **precondition**, a condition the caller must make true, that
the compiler can't check; calling it requires an `unsafe` block. In the 2024 edition the body of an
`unsafe fn` is ordinary safe code, so each unsafe operation inside needs its own block with its own
justification
([`unsafe_op_in_unsafe_fn`](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-op-in-unsafe-fn.html)),
and the workspace raises that lint from a warning to an error.

> [!TIP]
> **Remember.** `unsafe` turns off no check; it lets a person vouch for what the compiler can't
> prove, and the `SAFETY` comment is the written proof.

## Step 1: agree on memory layout

*Goal: see how two declarations of one struct, in two languages, are kept identical.*

Rust may reorder the fields of a struct to save **padding**, the unused bytes the compiler inserts
so that each field starts at a suitable address.
[`#[repr(C)]`](https://doc.rust-lang.org/reference/type-layout.html#the-c-representation) forbids
that and lays the fields out by C's rules, in declaration order. Python's
[`ctypes.Structure`](https://docs.python.org/3/library/ctypes.html#structures-and-unions), with its
`_fields_` list, does the same job from the Python side:

[`crates/keyrill-ffi/src/lib.rs:132-153`](../../crates/keyrill-ffi/src/lib.rs#L132-L153)

```rust
/// `kr_event`: one key or command, read by value; `len` counts the UTF-16 units in use.
#[repr(C)]
pub struct NativeEvent {
    role: u32,
    mode: u32,
    context: u32,
    len: u32,
    units: [u16; 8],
}

/// `kr_plan`: a proposal copied into caller-owned arrays; no Rust allocation crosses the
/// interface.
#[repr(C)]
pub struct NativePlan {
    plan_id: u64,
    action: u32,
    disposition: u32,
    expected_len: u32,
    mark_len: u32,
    expected: [u16; 128],
    mark: [u16; 128],
}
```

The same structs are declared a second time in C, and nothing ties the two declarations together
except agreement. So the layout is pinned from both sides. In Rust, a `const` item is evaluated by
the compiler, and an `assert!` in it that fails is a compile error:

[`crates/keyrill-ffi/src/lib.rs:167-176`](../../crates/keyrill-ffi/src/lib.rs#L167-L176)

```rust
// The same layouts the header's static assertions pin
const _: () = assert!(size_of::<CompatibilityPlan>() == 280);

const _: () = assert!(size_of::<NativeEvent>() == 32);
const _: () = assert!(size_of::<NativePlan>() == 536);
const _: () = assert!(std::mem::offset_of!(NativePlan, expected) == 24);
// A word plus its Space fits the 128-unit plan arrays
const _: () = assert!(keyrill_core::session::MAX_WORD < 128);
// KR_MAX_DICTIONARY_BYTES
const _: () = assert!(MAX_SOURCE_BYTES == 1_048_576);
```

In C, the header documents each offset and asserts the same numbers:

[`include/keyrill.h:173-192`](../../include/keyrill.h#L173-L192)

```c
/* A copied plan. expected is the exact text the owned composition must hold before the write,
 * empty when no word is owned; mark is the text to write. Units past each length are zero.
 *
 *   offset   0  plan_id       token for kr_commit; only the latest plan is accepted
 *            8  action        KR_PASS, KR_MARK, KR_FINISH or KR_UNAVAILABLE
 *           12  disposition   KR_KEY_PASS, KR_KEY_CONSUMED or KR_KEY_REPLAN
 *           16  expected_len
 *           20  mark_len
 *           24  expected      uint16_t[128]
 *          280  mark          uint16_t[128]                          536 bytes in all
 */
typedef struct {
  uint64_t plan_id;
  uint32_t action;
  uint32_t disposition;
  uint32_t expected_len;
  uint32_t mark_len;
  uint16_t expected[KR_TEXT_CAPACITY];
  uint16_t mark[KR_TEXT_CAPACITY];
} kr_plan;
```

[`include/keyrill.h:435-446`](../../include/keyrill.h#L435-L446)

```c
#ifdef __cplusplus
}
static_assert(sizeof(kr_event) == 32);
static_assert(sizeof(kr_plan) == 536);
static_assert(offsetof(kr_plan, expected) == 24);
static_assert(sizeof(kr_compatibility_plan) == 280);
#else
_Static_assert(sizeof(kr_event) == 32, "kr_event ABI");
_Static_assert(sizeof(kr_plan) == 536, "kr_plan ABI");
_Static_assert(offsetof(kr_plan, expected) == 24, "kr_plan text offset");
_Static_assert(sizeof(kr_compatibility_plan) == 280, "kr_compatibility_plan ABI");
#endif
```

A changed size or a moved text array is now a compile error on whichever side changed, never a plan
read at the wrong offset at run time. Why pin the offset of `expected` and not only the size?
Without `#[repr(C)]` Rust may put the arrays first, and the struct is still 536 bytes; only the
offset shows the move. Exercises 5 and 6 show which assertion fires in each case.

The assertions don't pin every field, though: swapping `action` and `disposition` in one declaration
passes them both. That remaining risk is what the C smoke test
`crates/keyrill-ffi/tests/abi_smoke.c` covers, by driving real plans through the header's struct.
The `#ifdef` picks the spelling each language understands: C11's `_Static_assert` or C++'s
`static_assert`. The `}` at the top closes an `extern "C"` block, which the
[Objective-C++ chapter](objective-cpp.md#c-linkage) explains.

> [!TIP]
> **Remember.** `#[repr(C)]` gives a struct C's layout, and compile-time assertions on both sides
> pin its size and the offset that would move.

## Step 2: hand ownership across, and take it back

*Goal: follow one engine from its creation in Rust to its destruction, requested from C.*

C needs a **handle** to an engine, a value that identifies it, but must not see inside it. Python
programmers know the idea from file descriptors: `os.open` returns an integer that means a file, and
the program must close it exactly once. The header declares an **opaque type**, a struct with a name
and no definition:

[`include/keyrill.h:74`](../../include/keyrill.h#L74)

```c
typedef struct kr_engine kr_engine;
```

C code can hold a `kr_engine *` and pass it back; it can't allocate one, copy one or read a field,
because the compiler doesn't know the size. On the Rust side the pointer is a
[`Box<Engine>`](https://doc.rust-lang.org/std/boxed/struct.Box.html), Rust's owning pointer to a
value on the heap, the counterpart of C++'s `std::unique_ptr`:

[`crates/keyrill-ffi/src/lib.rs:330-366`](../../crates/keyrill-ffi/src/lib.rs#L330-L366)

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_engine_new(abi: u32, out: *mut *mut Engine) -> u32 {
    if out.is_null() {
        return NULL;
    }
    // SAFETY: `out` is non-null (checked above), and the caller guarantees it is aligned and
    // writable for one pointer
    unsafe {
        out.write(std::ptr::null_mut());
    }
    if !compatible_abi(abi) {
        return VERSION;
    }
    match catch_unwind(|| {
        Box::new(Engine {
            session: Session::default(),
            poisoned: false,
            automatic: Automatic::Off,
            dictionaries: [Snapshot::default(), Snapshot::default()],
            learned: Vec::new(),
            backend: Backend::Unselected,
            compatibility_boundary: false,
            compatibility_word_boundary: false,
            compatibility_collect: true,
            compatibility_pending: None,
        })
    }) {
        Ok(engine) => {
            // SAFETY: same output contract; allocation is transferred exactly once
            unsafe {
                out.write(Box::into_raw(engine));
            }
            OK
        }
        Err(_) => POISONED,
    }
}
```

Read the order of operations: the output pointer is checked, then set to null, then the ABI version
is compared, and only then is anything allocated. A caller that ignores the status code still finds
a null handle, never leftover garbage from its own stack.
[`Box::into_raw`](https://doc.rust-lang.org/std/boxed/struct.Box.html#method.into_raw) gives up
ownership and returns a raw pointer, so Rust will not drop the engine; from this moment C holds the
only reference. The way back is
[`Box::from_raw`](https://doc.rust-lang.org/std/boxed/struct.Box.html#method.from_raw), which takes
ownership back, after which `drop` destroys the engine and frees its memory:

[`crates/keyrill-ffi/src/lib.rs:368-382`](../../crates/keyrill-ffi/src/lib.rs#L368-L382)

```rust
/// Free once, after every call on the engine has returned and no callback can reach it. Null is
/// a no-op.
///
/// # Safety
/// A non-null pointer must be the live allocation returned by `kr_engine_new`, with no aliases.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_engine_free(pointer: *mut Engine) {
    if !pointer.is_null() {
        // An unwind must not cross into C; a panic while dropping is discarded
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: caller transfers the original unique allocation exactly once
            drop(unsafe { Box::from_raw(pointer) });
        }));
    }
}
```

The `# Safety` section of the doc comment is the precondition of this `unsafe fn`, written for the
caller: a pointer freed twice, or freed while another call on it is running, is the caller's bug.

Is that enforced? Not by the compiler, and not by this function, which can't tell a live pointer
from a freed one. Two tools catch it in testing, on the paths a test exercises:
`./tools/verify sanitize` runs the native tests under
[AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer.html), which reports a double free
or a use after free at the moment it happens, and `./tools/verify miri` runs the Rust pointer tests
under [Miri](https://github.com/rust-lang/miri), an interpreter that detects undefined behavior. In
the shipped program the guarantee is ownership: each engine belongs to exactly one Objective-C
object, a `KRSession` or, for the compatibility backend, a `KRCompatibilitySession`, which frees it
once in its destructor, as the next chapter shows. The only other frees are of engines that never
left the function that created them: a failed configuration in `KRController.mm` and a validation
probe in `KRRuntime.mm`.

> [!TIP]
> **Remember.** `Box::into_raw` hands an allocation to C, and `Box::from_raw` takes it back for
> exactly one free. Freeing twice is the caller's bug; tests catch it, the compiler can't.

## Step 3: stop panics at the boundary

*Goal: see why a Rust panic must never reach C, and how one function prevents it.*

A panic **unwinds** the stack: it leaves each function in turn, running the destructors of its local
values, until something catches it, much as an exception does in Python, JavaScript and C++. It must
not unwind into C or Objective-C frames, which know nothing about Rust's unwinding. Since
[Rust 1.81](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/), a panic that reaches an
`extern "C"` function **aborts** the **process**, the running program, ending it at once with no
cleanup; before 1.81 it was undefined behavior. An aborted input method takes the user's typing with
it. So every exported function that touches an engine goes through one function, the most
instructive twenty lines of the crate:

[`crates/keyrill-ffi/src/lib.rs:292-316`](../../crates/keyrill-ffi/src/lib.rs#L292-L316)

```rust
// Callers provide live, uniquely borrowed handles. No callback is made while this borrow
// exists; every export releases it before returning to native code. A panic in `f` poisons the
// handle, which also drops the word, history and pending plans it held. AssertUnwindSafe is
// acceptable because nothing relies on state a panic may have left inconsistent: the handle is
// only reset, and every later call returns POISONED before touching it.
unsafe fn with_engine(pointer: *mut Engine, f: impl FnOnce(&mut Engine) -> u32) -> u32 {
    if pointer.is_null() {
        return NULL;
    }
    // SAFETY: non-null was checked above; the caller contract requires a live aligned handle from
    // kr_engine_new, not yet freed, with no other reference to it alive during this call
    let engine = unsafe { &mut *pointer };
    if engine.poisoned {
        return POISONED;
    }
    match catch_unwind(AssertUnwindSafe(|| f(engine))) {
        Ok(result) => result,
        Err(_) => {
            engine.poisoned = true;
            engine.session.reset();
            engine.compatibility_pending = None;
            POISONED
        }
    }
}
```

Three things happen in order. A null check, because C may pass anything. The conversion of a raw
pointer into `&mut Engine`, the one step that needs `unsafe`: from here on the borrow checker treats
the engine as exclusively borrowed, and the `SAFETY` comment states why that is true. Then
[`catch_unwind`](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html) around the work, which
turns a panic into an `Err`. In Python terms, every exported function is wrapped in `try`/`except`
and turns any exception into an error code.

A caught panic **poisons** the handle permanently. The panic may have left the session half-updated,
and no later call should reason from that state, so the handle is reset and every later call returns
`KR_POISONED` before touching anything.
[`AssertUnwindSafe`](https://doc.rust-lang.org/std/panic/struct.AssertUnwindSafe.html) is the
explicit statement that this is acceptable: by default `catch_unwind` refuses a closure that holds a
`&mut` across the panic, precisely because of the half-updated state, and the comment explains why
nothing here relies on it.

Look again at the closure parameter `f: impl FnOnce(&mut Engine) -> u32`. `with_engine` lends the
engine to `f` and gets it back; the borrow ends when `f` returns, and the function returns to native
code right after. No Rust reference to the engine can be alive while native code runs, and since
Rust never calls back into the adapter, there is no other way for one to be. That is how the rule
"never hold Rust borrows across native callbacks" from `AGENTS.md` is kept by construction.

Catching depends on one build setting:

[`Cargo.toml:20-22`](../../Cargo.toml#L20-L22)

```toml
[profile.release]
# The C ABI catches a panic and poisons the handle; with abort the panic would end the input server
panic = "unwind"
```

With [`panic = "abort"`](https://doc.rust-lang.org/cargo/reference/profiles.html#panic) there is no
unwinding to catch, and `catch_unwind` would never get a chance.

> [!TIP]
> **Remember.** A panic must not cross into C. `with_engine` catches it, poisons the handle, and
> returns an error code instead.

## Step 4: the anatomy of an exported function

*Goal: read an `extern "C"` function line by line.*

[`crates/keyrill-ffi/src/lib.rs:602-627`](../../crates/keyrill-ffi/src/lib.rs#L602-L627)

```rust
/// Acknowledge the latest plan; refused and unknown outcomes invalidate the session until
/// [`kr_reset`]. A stale token changes nothing.
///
/// # Safety
/// `pointer` must meet the live unique serialized handle contract, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_commit(pointer: *mut Engine, token: u64, outcome: u32) -> u32 {
    // SAFETY: forwarded handle contract
    unsafe {
        with_engine(pointer, |engine| {
            let outcome = match outcome {
                0 => Outcome::Verified,
                1 => Outcome::Refused,
                2 => Outcome::Unknown,
                _ => return BAD_TAG,
            };
            if engine.backend == Backend::Compatibility {
                return STATE;
            }
            engine
                .session
                .acknowledge(token, outcome)
                .map_or_else(status, |()| OK)
        })
    }
}
```

- [`extern "C"`](https://doc.rust-lang.org/reference/items/functions.html#extern-function-qualifier)
  selects C's calling convention.
- `#[unsafe(no_mangle)]` keeps the symbol name exactly `kr_commit`. Without it the compiler would
  **mangle** the name, encoding the crate, module and a hash into the symbol so that equal names in
  different places stay distinct ([name mangling](https://en.wikipedia.org/wiki/Name_mangling)), and
  the linker would never match it to the header's declaration. The 2024 edition marks the attribute
  itself [`unsafe`](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-attributes.html),
  because an unmangled name can collide with another symbol in the final program.
- An integer from C becomes an enum only through a `match` with a catch-all arm that returns
  `BAD_TAG`. Rust has no cast from an integer to an enum, and a `transmute`, a reinterpretation of
  the bits, of a stray `3` would create an `Outcome` with no variant, which is undefined behavior;
  the value must be rejected before it becomes an enum.
- `return BAD_TAG` inside the closure returns from the closure, which is `with_engine`'s result.
- `map_or_else(status, |()| OK)` turns the session's `Result<(), Error>` into a status code: the
  function `status`, passed by name, converts the error, and the closure converts success.

Plans travel the other way by value. `kr_plan_event` decodes the C event into an owned Rust `Event`
and writes a complete `NativePlan` into memory the caller owns:

[`crates/keyrill-ffi/src/lib.rs:228-255`](../../crates/keyrill-ffi/src/lib.rs#L228-L255)

```rust
fn decode_event(event: &NativeEvent) -> Result<(Event, Mode, Context), u32> {
    if event.len > 8 {
        return Err(BOUNDS);
    }
    let mode = match event.mode {
        1 => Mode::En,
        2 => Mode::Ru,
        _ => return Err(BAD_TAG),
    };
    let context = match event.context {
        0 => Context::Boundary,
        1 => Context::Ineligible,
        _ => return Err(BAD_TAG),
    };
    if event.role != 1 && event.len != 0 {
        return Err(BAD_TAG);
    }
    let input = match event.role {
        1 => Event::Text(event.units[..event.len as usize].to_vec()),
        2 => Event::Backspace,
        3 => Event::Escape,
        4 => Event::Manual,
        5 => Event::Reverse,
        6 => Event::Finish,
        _ => return Err(BAD_TAG),
    };
    Ok((input, mode, context))
}
```

[`crates/keyrill-ffi/src/lib.rs:593-597`](../../crates/keyrill-ffi/src/lib.rs#L593-L597)

```rust
            // Conversion maps unit for unit, so both texts are at most a MAX_WORD word plus Space
            copied.expected[..plan.expected.len()].copy_from_slice(&plan.expected);
            copied.mark[..plan.text.len()].copy_from_slice(&plan.text);
            out.write(copied);
            OK
```

The length is checked before the slice `event.units[..event.len as usize]` is taken, although an
out-of-range slice would only panic and be caught: a status code is a better answer to bad input
than a poisoned handle. `to_vec()` copies the units into a vector the session owns, so nothing in
Rust keeps a pointer into the caller's struct. On the way out, the plan's vectors are copied into
the fixed arrays of `NativePlan`, the rest stays zero, and `out.write` stores the whole struct at
once. No Rust allocation crosses the interface in either direction, and the adapter frees nothing
but handles.

> [!TIP]
> **Remember.** An export uses C's calling convention and an unmangled name, validates every integer
> before it becomes an enum, and copies data across instead of sharing it.

## Threads and locks

*Goal: read a `Mutex`, an atomic flag and a lock order, and know what each prevents.*

The engine is used from one thread: the header requires calls on one engine to come one at a time
from the thread that owns its input context, and `&mut` in `with_engine` encodes exactly that.
Dictionary files are a different matter. Reading a file is slow and must not happen on the thread
that handles keys, so a second handle, `kr_vocabulary`, is shared between a background queue that
reads files and the main thread, the one on which macOS delivers input, that hands dictionaries to
engines.

Two threads that touch the same memory, at least one of them writing, without coordination, form a
**data race**: undefined behavior in Rust and C++. Python's global interpreter lock hides some of
this, though not all, and JavaScript avoids it by running one thread per event loop. The module
documentation states the design:

[`crates/keyrill-ffi/src/vocabulary_host.rs:1-19`](../../crates/keyrill-ffi/src/vocabulary_host.rs#L1-L19)

````rust
//! `kr_vocabulary`: dictionary sources that engines adopt, without scheduling or native editing
//! authority.
//!
//! A vocabulary handle holds six vocabulary sources, the English and Russian Vim spell files and
//! four learned-word slots, each with a configured path, flags, a generation and a current
//! snapshot. Two locks keep the slow path away from the controller thread:
//!
//! ```text
//!   acquisition  one Source per slot; held across file I/O by kr_vocabulary_observe
//!   published    path, flags, generation and snapshot per slot; held briefly by configure,
//!                adopt, and the start and end of observe
//! ```
//!
//! Observe takes `acquisition` before `published` and nothing takes them in the other order, so
//! configure and adopt never wait for a file read. A configuration change advances the slot's
//! generation; an observation that began under the old generation finds the change when it
//! locks `published` again and publishes nothing (`STALE_CONFIGURATION`). Adoption clones every
//! snapshot under one lock, so an engine never pairs a language from before a change with one
//! from after it.
````

A **lock** lets one thread at a time into the code it protects. Taking two locks in different orders
on two threads can leave each thread waiting for the lock the other holds, forever: a
[deadlock](https://en.wikipedia.org/wiki/Deadlock_%28computer_science%29). A fixed order, here
`acquisition` before `published`, rules that out. The data structure puts each lock around exactly
what it protects:

[`crates/keyrill-ffi/src/vocabulary_host.rs:72-79`](../../crates/keyrill-ffi/src/vocabulary_host.rs#L72-L79)

```rust
/// `kr_vocabulary`: six vocabulary sources behind two locks, shared by the acquisition queue and
/// the controller thread. Free only after every operation has completed.
#[derive(Default)]
pub struct VocabularyHandle {
    poisoned: AtomicBool,
    acquisition: Mutex<Acquisition>,
    published: Mutex<[Language; SOURCES]>,
}
```

A Rust [`Mutex<T>`](https://doc.rust-lang.org/std/sync/struct.Mutex.html) *contains* the data it
guards. The only way to reach the `Acquisition` is `lock()`, which returns a **guard** that gives
access to the data and unlocks when it is dropped, so forgetting to lock is not an error you can
make. Python's [`threading.Lock`](https://docs.python.org/3/library/threading.html#lock-objects) in
a `with` block unlocks the same way, but it protects nothing by itself: any code can touch the data
without taking it.

Rust tracks which types may be used from several threads with two traits the compiler derives on its
own, [`Send` and `Sync`](https://doc.rust-lang.org/nomicon/send-and-sync.html); `Mutex` and
`AtomicBool` are what make a shared `&VocabularyHandle` safe to use from two threads at once. A raw
pointer from C bypasses that check, so the function that turns it into a reference states the
argument itself:

[`crates/keyrill-ffi/src/vocabulary_host.rs:212-234`](../../crates/keyrill-ffi/src/vocabulary_host.rs#L212-L234)

```rust
// Shared references are synchronized internally; no native calls occur under either lock.
// Any POISONED result, a caught panic or a lock poisoned by one, poisons the handle for good.
unsafe fn with_vocabulary(
    pointer: *const VocabularyHandle,
    operation: impl FnOnce(&VocabularyHandle) -> u32,
) -> u32 {
    if pointer.is_null() {
        return NULL;
    }
    // SAFETY: non-null was checked above; the caller contract requires the live aligned
    // allocation from kr_vocabulary_new, not yet freed and kept alive for this call. Only shared
    // references are made, and every mutation goes through a Mutex or the atomic flag, so
    // concurrent calls on the handle are allowed.
    let vocabulary = unsafe { &*pointer };
    if vocabulary.poisoned.load(Ordering::Acquire) {
        return POISONED;
    }
    let status = catch_unwind(AssertUnwindSafe(|| operation(vocabulary))).unwrap_or(POISONED);
    if status == POISONED {
        vocabulary.poisoned.store(true, Ordering::Release);
    }
    status
}
```

Compare it with `with_engine`. This one makes `&*pointer`, a shared reference, where `with_engine`
made `&mut *pointer`; two threads may hold shared references at once, and the type allows nothing
through them but locking and atomic operations. An **atomic** variable is one whose reads and writes
are indivisible even across threads, and the poison flag is one, with
[`Acquire` and `Release`](https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html) ordering:
the rules that decide which other writes a thread is guaranteed to see. They are the same as C++'s
`std::memory_order`, and JavaScript exposes them through
[`Atomics`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Atomics)
for shared memory between workers.

A Rust **mutex**, short for *mutual exclusion* and the name of its lock type, has poisoning of its
own: if a thread panics while it holds the guard, later calls to `lock()` return an error rather
than hand out data that may be inconsistent
([poisoning](https://doc.rust-lang.org/std/sync/struct.Mutex.html#poisoning)). The code turns that
error into `KR_POISONED` as well:

[`crates/keyrill-ffi/src/vocabulary_host.rs:133-160`](../../crates/keyrill-ffi/src/vocabulary_host.rs#L133-L160)

```rust
        let Ok(mut languages) = self.published.lock() else {
            return POISONED;
        };
        let language = &mut languages[index];
        if language.path == path
            && language.missing_is_empty == missing_is_empty
            && language.excluded == excluded
        {
            return OK;
        }
        let Some(generation) = language.generation.checked_add(1) else {
            return STATE;
        };
        let retired = std::mem::replace(
            language,
            Language {
                generation,
                path,
                missing_is_empty,
                excluded,
                current: None,
            },
        );
        // Free the old snapshot after unlocking, so a large vocabulary never holds the lock
        drop(languages);
        drop(retired);
        OK
    }
```

[`std::mem::replace`](https://doc.rust-lang.org/std/mem/fn.replace.html) puts a new value in place
and hands back the old one, which keeps the old snapshot alive past the assignment. Then the order
of the two `drop` calls does the work the comment describes: the guard first, which unlocks, and the
retired value second, so freeing a large word list happens outside the lock. In C++ the same intent
needs a carefully placed scope; here destruction is a function call you can put where you want it.

The generation is the same device the Objective-C++ side uses against reentrancy, applied to
threads: an observation remembers the generation it started under and publishes nothing if the
configuration changed while it read the file. You will meet it again, as `_generation`, in
[Reentrancy and tests](reentrancy-and-tests.md).

> [!TIP]
> **Remember.** A Rust `Mutex` owns its data, so it can't be bypassed. Take locks in one fixed
> order, and free large values after unlocking.
