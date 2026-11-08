# Rust in the core

`keyrill-core` is the crate that decides. It is safe Rust with no dependencies outside the
**standard library**, the library every Rust program gets with the compiler, and it knows nothing
about macOS, pointers (raw memory addresses) or documents. That makes it the right place to learn
the language: every idea below is one the compiler checks, and the chapter shows the place where
breaking it would stop the build. The full reference is [The Rust Programming Language] (the Book);
the links go to the chapter that treats each idea at length.

## Before the first excerpt: editions and checks

*Goal: know what "Rust 2024" means and what the compiler does before a program runs.*

Rust changes in small steps, and the steps that would break old code are collected into an
**edition**: Rust 2015, 2018, 2021 and 2024. Each crate names its edition in its `Cargo.toml`, the
manifest that Rust's build tool, Cargo, reads; Keyrill's **workspace**, the set of crates built
together from the root `Cargo.toml`, sets `edition = "2024"`. Crates of different editions link
together freely, so an edition is a choice of dialect per crate, not a fork of the language
([Edition Guide](https://doc.rust-lang.org/edition-guide/editions/index.html)). When this
walkthrough says "Rust 2024 requires...", it means a rule of that dialect.

Much of what follows happens at **compile time**, while the compiler translates the program, rather
than at **run time**, while the program runs. Python and JavaScript find most mistakes at run time,
when the faulty line executes; C++ finds some at compile time. Rust moves as much as it can to
compile time, and this crate is built to take advantage of it: when a rule below is broken, the
program doesn't build.

> [!TIP]
> **Remember.** An edition is a per-crate dialect of Rust; Keyrill uses 2024. The checks in this
> chapter happen at compile time, before anything runs.

## Types that carry their meaning

*Goal: read an `enum` with fields and the `match` that takes it apart.*

A **struct** is a record of named **fields**, like a C++ `struct`, a Python dataclass or a
JavaScript object with a fixed shape. An **enum** in Rust is a choice between **variants**, and each
variant may carry its own fields, which exist only while the value is that variant. The general name
is a [**tagged union**](https://en.wikipedia.org/wiki/Tagged_union): C++ has it as
[`std::variant`](https://en.cppreference.com/w/cpp/utility/variant), TypeScript as a
[discriminated union](https://www.typescriptlang.org/docs/handbook/2/narrowing.html#discriminated-unions),
and Python and plain JavaScript only by convention, an object with a `kind` field. Here is the state
of the word being typed:

[`crates/keyrill-core/src/session.rs:164-182`](../../crates/keyrill-core/src/session.rs#L164-L182)

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    /// Composing while `boundary` is clear, BoundaryRetained once the Space has joined the mark.
    Owned {
        /// What the owned mark holds, including the Space once `boundary` is set.
        text: Vec<u16>,
        /// The receipt: the text before the word was first converted; `None` if it was never
        /// converted, or after an edit or a Reverse.
        original: Option<Vec<u16>>,
        boundary: bool,
        /// Set by Manual or Reverse; Space then keeps the word without inference.
        explicit: bool,
        /// Input mode the word was started in.
        mode: Mode,
    },
    Invalidated,
}
```

Look at what can't be written. There is no `text` while the state is `Idle`, because the field
doesn't exist there. A class with a `state` member and a separate `text` member would allow the
nonsensical pair "idle, with text" and rely on every programmer to avoid it; here the type rules it
out.

The first line is an **attribute**, an annotation for the compiler written `#[...]`.
[`derive`](https://doc.rust-lang.org/reference/attributes/derive.html) asks the compiler to generate
code for the listed **traits**. A trait is a named set of methods a type can provide, close to a C++
concept, a Python protocol and a TypeScript interface at once; `Clone` provides copying, `Debug`
printing for programmers, `PartialEq` the `==` operator, `Default` a default value.
[`#[default]`](https://doc.rust-lang.org/std/default/trait.Default.html#enums) names the variant
that `Default` produces, which is how `Session::default()` starts in `Idle` without a constructor.

**`Option<T>`** is an enum from the standard library with two variants, `Some(T)` and `None`. It
replaces the null pointer, Python's `None` and JavaScript's `null` everywhere in safe Rust, with one
difference that matters: the compiler will not let you use the value inside without first saying
what happens when there is none. `original` above is either a receipt or nothing, and code that
forgets the "nothing" case doesn't compile.

Small enums take the place of integer codes, even where a number would be shorter. The plan says
what to do with the key, and the adapter reports what happened, each as its own type:

[`crates/keyrill-core/src/session.rs:121-141`](../../crates/keyrill-core/src/session.rs#L121-L141)

```rust
/// What happens to the key event once the plan's action is verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Let the host handle the event once.
    Pass,
    /// The event is handled; the host does not see it.
    Consumed,
    /// Finish was acknowledged; plan the same event again from Idle.
    Replan,
}

/// Native effect acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Native effect and readback verified, or the plan had no native effect.
    Verified,
    /// Rejected before any native write.
    Refused,
    /// Write attempted but outcome cannot be established.
    Unknown,
}
```

Two integers could carry both, and a function could then be handed a disposition where an outcome
was meant. With two types, that mistake doesn't compile. `Copy` in the derive list says these values
are plain bits, copied on assignment like a C `int`; the next section explains what happens to
values that aren't.

### `match` keeps count

*Goal: see why adding a variant breaks the build, and why that is good.*

[`match`](https://doc.rust-lang.org/book/ch06-02-match.html) compares a value against **patterns**,
one per **arm**, and runs the first arm that fits. It must cover every variant, so adding a variant
turns every incomplete `match` into a compile error, even in another crate. Python has a
[`match` statement](https://docs.python.org/3/tutorial/controlflow.html#match-statements) with
similar syntax, but it doesn't check that the cases are complete; TypeScript can, with an
[exhaustiveness check](https://www.typescriptlang.org/docs/handbook/2/narrowing.html#exhaustiveness-checking)
you write yourself. This is the place where the C boundary turns Rust's enums back into the numbers
of `keyrill.h`:

[`crates/keyrill-ffi/src/lib.rs:575-592`](../../crates/keyrill-ffi/src/lib.rs#L575-L592)

```rust
            let mut copied = NativePlan {
                plan_id: plan.id,
                action: match plan.action {
                    Action::Pass => 0,
                    Action::Mark => 1,
                    Action::Finish => 2,
                    Action::Unavailable => 3,
                },
                disposition: match plan.disposition {
                    Disposition::Pass => 0,
                    Disposition::Consumed => 1,
                    Disposition::Replan => 2,
                },
                expected_len: plan.expected.len() as u32,
                mark_len: plan.text.len() as u32,
                expected: [0; 128],
                mark: [0; 128],
            };
```

`match` is an expression: it produces a value, so it can sit inside a struct literal. There is no
`default:` arm, and that is the point. A fifth action added to the core can't cross to the C side
unnoticed, because this file stops compiling until someone decides which number it gets. Exercise 2
shows the error.

A pattern can look inside a variant and add a condition, a **guard**, written after `if`. This one
recognizes a key that starts or continues a word:

[`crates/keyrill-core/src/session.rs:449-460`](../../crates/keyrill-core/src/session.rs#L449-L460)

```rust
        // In English mode the keys that type Russian letters can start or continue a word
        let letter = match event {
            Event::Text(units)
                if units.len() == 1
                    && (mapping::is_letter(units[0])
                        || (mode == Mode::En
                            && inference::is_provisional(units[0], self.keyboard))) =>
            {
                Some(units[0])
            }
            _ => None,
        };
```

`Event::Text(units)` matches only the text variant and names its contents `units`; the guard then
requires exactly one unit that is a letter, or, in English mode, a key such as `[` that types a
Russian letter on the Russian layout. `_` matches everything else. The result is an `Option<u16>`: a
letter, or none.

Ranges are patterns too, and [`matches!`](https://doc.rust-lang.org/std/macro.matches.html) turns a
pattern into a `bool`. The `!` marks a **macro**, code that the compiler expands into more code
before compiling it; `matches!`, `assert!` and `include_bytes!` below are all macros. The letter set
of the whole engine is one line:

[`crates/keyrill-core/src/mapping.rs:42-46`](../../crates/keyrill-core/src/mapping.rs#L42-L46)

```rust
/// Whether a unit is an EN/RU letter: `A`-`Z`, `a`-`z`, `А`-`я`, `Ё` or `ё`. [`Mapping::Manual`],
/// the word session and language context detection all use this letter set.
pub fn is_letter(unit: u16) -> bool {
    matches!(unit, 0x41..=0x5a | 0x61..=0x7a | 0x410..=0x44f | 0x401 | 0x451)
}
```

`u16` is an unsigned 16-bit integer, and the text is a sequence of them throughout: **UTF-16 code
units**, the encoding macOS and Windows use for text, in which most characters take one unit and
some take two. An offset that means the same thing on both sides of the C boundary is worth more
here than the convenience of Rust's UTF-8 `String`.

> [!TIP]
> **Remember.** An enum's fields exist only in their variant, so impossible states can't be built. A
> `match` must cover every variant, so a new variant can't be forgotten.

## Ownership and borrowing

*Goal: tell from a method's signature whether it can change its object.*

Python and JavaScript free memory with a **garbage collector**, which finds objects nobody refers to
and reclaims them later; C++ leaves it to the programmer and to destructors. Rust has neither a
garbage collector nor manual `free`. Every value has one **owner**, a variable or a field, and the
value is destroyed, **dropped**, when its owner goes out of scope.

Other code gets access by
[borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html): it takes a
**reference** to the value without owning it. There are two kinds, and the compiler's **borrow
checker** enforces one rule about them: any number of shared references `&T`, or exactly one
exclusive reference `&mut T`, never both at the same time. Python and JavaScript have nothing like
this; any function you hand a list to may change it. The rule is short, and this project builds its
central guarantee on it. Compare the first parameter of two methods in the same `impl` block, the
block that defines a type's methods:

[`crates/keyrill-core/src/session.rs:438-446`](../../crates/keyrill-core/src/session.rs#L438-L446)

```rust
    /// The transition table: the plan's action, disposition and text, and the proposed state.
    /// Reads the confirmed state and changes nothing.
    fn transition(
        &self,
        event: &Event,
        mode: Mode,
        context: Context,
        automatic: Automatic<'_>,
    ) -> (Action, Disposition, Vec<u16>, State) {
```

A method's first parameter, `self`, is the object it was called on, like Python's `self` and
JavaScript's `this`. `transition` takes `&self`, a shared borrow of the session, so it **cannot**
change the session. The doc comment says "changes nothing", and for once a comment needs no trust:
the compiler enforces it, as exercise 1 shows. The whole transition table, more than 150 lines, is
therefore a pure function of the confirmed state and the event. Its caller takes `&mut self`, and it
stores the result as a proposal, leaving the confirmed `state` alone:

[`crates/keyrill-core/src/session.rs:363`](../../crates/keyrill-core/src/session.rs#L363)

```rust
        let (action, disposition, text, next) = self.transition(event, mode, context, automatic);
```

[`crates/keyrill-core/src/session.rs:381-393`](../../crates/keyrill-core/src/session.rs#L381-L393)

```rust
        self.pending = Some(Pending {
            id,
            state: next,
            history,
            mode,
        });
        Ok(Plan {
            id,
            action,
            disposition,
            expected,
            text,
        })
```

`state: next` **moves** the proposed state into `pending`. After that line `next` can no longer be
used, and the compiler says so if you try. Moving is what assignment does for any type that isn't
`Copy`: the value changes owner, and its heap buffers go with it without being copied. The **heap**
is the memory for data whose size or lifetime isn't known in advance, such as the contents of a
growable `Vec`; the **stack** holds a function's local variables and is freed when the function
returns. In Python, `b = a` makes a second name for the same object and both stay usable; in Rust,
`let b = a;` hands the value over, and `a` is dead. C++'s `std::move` is the nearest relative,
except that the moved-from variable there is "valid but unspecified" rather than unusable.

Because `transition` only borrows the session, it can't take the current text out of it to build the
next state. It copies instead, and says so in the code:

[`crates/keyrill-core/src/session.rs:498-501`](../../crates/keyrill-core/src/session.rs#L498-L501)

```rust
                let mut text = text.clone();
                let mut original = original.clone();
                let mut boundary = *boundary;
                let mut explicit = *explicit;
```

`clone()` makes a deep copy, like Python's `copy.deepcopy` or JavaScript's `structuredClone`, and in
Rust it is always written out; nothing is copied behind your back the way a C++ copy constructor
can. `*boundary` reads the `bool` that `boundary` refers to. The copies make the next property
possible: the `Plan` returned to the adapter owns its text and borrows nothing from the session. A
borrowed plan would keep the session locked for as long as the adapter held it, and the adapter must
stay free to reset the session in the middle of a call to the application.

### Taking a value out exactly once

*Goal: see how a value that may be used only once is made impossible to use twice.*

The acknowledgement is where a proposal becomes the truth, and it must happen at most once per plan.
[`Option::take`](https://doc.rust-lang.org/std/option/enum.Option.html#method.take) moves the value
out of an `Option` and leaves `None` in its place:

[`crates/keyrill-core/src/session.rs:405-410`](../../crates/keyrill-core/src/session.rs#L405-L410)

```rust
    pub fn acknowledge(&mut self, id: u64, outcome: Outcome) -> Result<(), Error> {
        if self.pending.as_ref().map(|p| p.id) != Some(id) {
            return Err(Error::Stale);
        }
        let pending = self.pending.take().ok_or(Error::Stale)?;
        if outcome == Outcome::Verified {
```

[`crates/keyrill-core/src/session.rs:428-436`](../../crates/keyrill-core/src/session.rs#L428-L436)

```rust
            self.mode = Some(pending.mode);
            self.state = pending.state;
        } else {
            self.state = State::Invalidated;
            self.history.clear();
            self.mode = None;
        }
        Ok(())
    }
```

Read the first check twice. `self.pending.as_ref()` turns a reference to an `Option<Pending>` into
an `Option` holding a reference, so the **closure** `|p| p.id`, an anonymous function like Python's
`lambda p: p.id` or JavaScript's `p => p.id`, can read the id without taking the proposal. The
result is compared with `Some(id)`. Only then does `take()` move the proposal out. A second
acknowledgement of the same id finds `None` and returns `Stale`. Nothing in this code needs a "has
been used" flag, because the value itself is gone.

> [!TIP]
> **Remember.** `&self` can read and can't write; `&mut self` can do both, and only one may exist at
> a time. Assignment moves a value unless its type is `Copy`.

## Lifetimes

*Goal: read `'a` in a type and say what it protects against.*

A reference must never outlive the value it points to. A garbage-collected language guarantees that
by keeping the value alive as long as anything refers to it; Rust instead refuses to compile a
program in which a reference could outlive its value. Most of the time the compiler works this out
without help. When a type *stores* a reference, it needs a name for how long that reference stays
valid, and that name is a
[**lifetime**](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html), written with an
apostrophe: `'a`. A lifetime generates no code. It is a constraint the compiler solves and then
forgets.

The word model is a good first example, because nothing in it is copied at all. Every field is a
**slice**, `&'a [u8]`, a view of part of one byte array, made of a pointer and a length. Python's
[`memoryview`](https://docs.python.org/3/library/stdtypes.html#memoryview) and JavaScript's
[`subarray`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/TypedArray/subarray)
are views of the same kind, with the difference that they keep the underlying buffer alive and a
Rust slice doesn't:

[`crates/keyrill-core/src/model.rs:92-107`](../../crates/keyrill-core/src/model.rs#L92-L107)

```rust
/// One language's frequency table and character chain, borrowed from a validated asset.
#[derive(Clone, Copy)]
pub struct LanguageModel<'a> {
    alphabet: &'a [u8],
    offsets: &'a [u8],
    words: &'a [u8],
    frequencies: &'a [u8],
    chain: &'a [u8],
}

/// Validated EN and RU models.
#[derive(Clone, Copy)]
pub struct Model<'a> {
    en: LanguageModel<'a>,
    ru: LanguageModel<'a>,
}
```

`Model<'a>` reads "a model that may not outlive the bytes it was parsed from". In C++ the same
design is a struct of [`std::span`](https://en.cppreference.com/w/cpp/container/span) members and a
comment warning about dangling pointers. Here a program in which the bytes die first doesn't
compile. Copying the whole `Model` copies ten slices, twenty machine words and no data, which is why
it can derive `Copy`.

The parser shows a subtler point. A `Reader` holds the bytes not yet read, and `take` hands out the
next piece:

[`crates/keyrill-core/src/model.rs:109-122`](../../crates/keyrill-core/src/model.rs#L109-L122)

```rust
/// Cursor over the asset; a read past the end is [`ModelError::Truncated`].
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ModelError> {
        if length > self.bytes.len() {
            return Err(ModelError::Truncated);
        }
        let (head, tail) = self.bytes.split_at(length);
        self.bytes = tail;
        Ok(head)
    }
```

`take` borrows the reader exclusively (`&mut self`), yet the slice it returns has the lifetime `'a`,
tied to the asset, not to the reader. So the caller may keep the slice after the reader moves on,
and `Model::parse` keeps every section this way without copying a byte. Had the return type been
written `&[u8]`, with no name, the compiler would have tied it to the borrow of the reader, and the
reader would have stayed locked as long as any section was alive. One annotation says which of the
two is meant.

The asset itself is compiled into the program with
[`include_bytes!`](https://doc.rust-lang.org/std/macro.include_bytes.html), so it lives as long as
the program, and that is what the special lifetime `'static` says:

[`crates/keyrill-core/src/model.rs:32`](../../crates/keyrill-core/src/model.rs#L32)

```rust
static EMBEDDED: &[u8] = include_bytes!("../../../assets/model.bin");
```

[`crates/keyrill-core/src/model.rs:327-329`](../../crates/keyrill-core/src/model.rs#L327-L329)

```rust
    pub fn embedded() -> Result<Model<'static>, ModelError> {
        Model::parse(EMBEDDED)
    }
```

`Result<..., ModelError>` is the return type of a function that can fail; the section on errors
below covers it.

### A type without a lifetime can't hold a borrow

*Goal: read a missing lifetime as a promise.*

Lifetimes also let a type promise what it does *not* keep. Automatic correction needs the model and
the user's dictionaries for the length of one call, so the enum that carries them has a lifetime:

[`crates/keyrill-core/src/session.rs:184-188`](../../crates/keyrill-core/src/session.rs#L184-L188)

```rust
/// Automatic correction at Space: the fixture policy, on or off, or the model.
enum Automatic<'a> {
    Fixture(bool),
    Model(&'a Model<'a>, Dictionaries<'a>, Policy),
}
```

The proposal that waits for acknowledgement has none:

[`crates/keyrill-core/src/session.rs:197-203`](../../crates/keyrill-core/src/session.rs#L197-L203)

```rust
/// The proposed state of the latest plan, adopted only by a verified acknowledgement.
struct Pending {
    id: u64,
    state: State,
    history: HistoryEffect,
    mode: Mode,
}
```

`Pending` has no lifetime parameter, so none of its fields can be a reference with a limited
lifetime. The documentation of `propose_with_inference` says that "the pending operation retains no
model, vocabulary reference or native callback" (a **callback** is a function handed to other code
for it to call later), and the type definition is the proof. Between a plan and its acknowledgement
the adapter calls into a foreign application, which may reset the session or replace the
dictionaries; nothing in the session can point at the old ones.

> [!TIP]
> **Remember.** A lifetime names how long a borrowed reference stays valid. A type without one can't
> hold a borrowed reference, and the compiler holds it to that.

## Shared data that never changes

*Goal: share one value between readers without copying it and without locks.*

Personal dictionaries change while Keyrill runs: the user adds a word in Vim while Keyrill holds the
old list. A reader that started with the old list must keep a consistent view, and a broken file
must not replace a good one. The tool is
[`Arc`](https://doc.rust-lang.org/std/sync/struct.Arc.html), an **atomically reference-counted**
pointer: it counts how many owners share the value, frees the value when the count reaches zero, and
updates the count safely from several **threads**, the independent sequences of execution inside one
running program. It is the counterpart of C++'s `std::shared_ptr<const T>`. CPython counts
references to every object in the same way, and adds a collector for reference cycles that Rust
doesn't have. The value inside an `Arc` is never changed:

[`crates/keyrill-core/src/vocabulary.rs:374-403`](../../crates/keyrill-core/src/vocabulary.rs#L374-L403)

```rust
/// Atomically replaceable immutable vocabulary, initially missing rather than silently empty.
#[derive(Default)]
pub struct Snapshot {
    current: Option<Arc<Vocabulary>>,
}

impl From<Option<Arc<Vocabulary>>> for Snapshot {
    fn from(current: Option<Arc<Vocabulary>>) -> Self {
        Self { current }
    }
}

impl Snapshot {
    /// Clone the current immutable snapshot; absence keeps automatic policy unready.
    pub fn current(&self) -> Option<Arc<Vocabulary>> {
        self.current.clone()
    }

    /// Parse `source` as a Vim word list and publish it whole; a candidate with skipped lines is
    /// still published. Readers that cloned the previous snapshot keep it unchanged.
    ///
    /// # Errors
    ///
    /// The candidate's [`ParseError`]; the current snapshot is left unchanged.
    pub fn replace(&mut self, source: &[u8]) -> Result<(), ParseError> {
        let candidate = Vocabulary::parse(source)?;
        self.current = Some(Arc::new(candidate));
        Ok(())
    }
}
```

Cloning an `Arc` copies the pointer and adds one to the count; the vocabulary itself is shared.
Publishing replaces the pointer, never the contents, so a reader holding the old `Arc` keeps the old
list until it lets go. Note the order inside `replace`: the `?` leaves the function on a parse error
*before* the assignment, so a bad file can't disturb the good snapshot. The doc comment says
"initially missing rather than silently empty", and the `Option` makes the difference representable:
a user who has no dictionary yet is not the same as one whose dictionary is empty.

[`From`](https://doc.rust-lang.org/std/convert/trait.From.html) is the standard trait for
conversions. Implementing it lets the C boundary write `current.map(Snapshot::from)` later; in
Python it would be a constructor that takes the other type.

The engine takes the snapshots apart into plain references for the length of one proposal:

[`crates/keyrill-ffi/src/lib.rs:271-287`](../../crates/keyrill-ffi/src/lib.rs#L271-L287)

```rust
    match &engine.automatic {
        Automatic::Model(model) => {
            let en = engine.dictionaries[0].current();
            let ru = engine.dictionaries[1].current();
            engine.session.propose_with_inference(
                input,
                mode,
                context,
                model,
                Dictionaries {
                    en: en.as_deref(),
                    ru: ru.as_deref(),
                    learned: &engine.learned,
                },
                Policy::default(),
            )
        }
```

`en` is an `Option<Arc<Vocabulary>>`, a counted owner held in a local variable for the whole call.
`as_deref()` borrows through it and gives an `Option<&Vocabulary>`, which is what `Dictionaries`
stores. The borrow checker accepts this only because `en` outlives the call that uses the reference.
Build `Dictionaries` in a `let` statement straight from
`engine.dictionaries[0].current().as_deref()` instead, and the `Arc` becomes a temporary dropped at
the end of that statement; the compiler rejects the dangling reference with
`error[E0716]: temporary value dropped while borrowed`.

> [!TIP]
> **Remember.** An `Arc` shares one value that never changes. Updating means publishing a new `Arc`,
> while readers keep the old one.

## Errors as values

*Goal: read `Result`, `?` and `let ... else`, and know where a panic fits.*

Python and JavaScript report most failures with exceptions, which travel up the stack until someone
catches them, invisible in the function's signature. Rust has no exceptions for failures a program
expects. A function that can fail returns
[`Result<T, E>`](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html), an
enum of `Ok(T)` and `Err(E)`, so the failure is part of the return type and the caller must handle
it. The
[`?` operator](https://doc.rust-lang.org/reference/expressions/operator-expr.html#the-try-propagation-expression)
says "on failure, return the error to my caller", which is what an uncaught exception does, except
that it is written at each place it can happen. It works on `Option` as well, where failure is
`None`. The model parser uses both:

[`crates/keyrill-core/src/model.rs:128-145`](../../crates/keyrill-core/src/model.rs#L128-L145)

```rust
    fn u32(&mut self) -> Result<u32, ModelError> {
        u32_at(self.take(4)?, 0).ok_or(ModelError::Truncated)
    }
}

fn u16_at(bytes: &[u8], index: usize) -> Option<u16> {
    let at = index.checked_mul(2)?;
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(bytes: &[u8], index: usize) -> Option<u32> {
    let at = index.checked_mul(4)?;
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
```

`u16_at` has four ways to fail, and each is one `?`: the multiplication overflows, the addition
overflows, the range lies outside the slice (`get` returns `None` instead of crashing the way
`bytes[at]` would), or the piece has the wrong length for a two-byte array. The asset comes from the
build, yet every read is checked, because the same parser must be safe on a damaged file, and the
project's **fuzzer**, a tool that feeds a program generated and damaged inputs, does exactly that.

### `let ... else` and arithmetic that can't wrap

*Goal: handle an unlikely case that must not be skipped.*

[`let ... else`](https://doc.rust-lang.org/reference/statements.html#let-statements) binds a pattern
if it matches and otherwise runs the `else` block, which must leave the function. Read it as "take
the happy path, or get out". Here it guards the counter that numbers plans:

[`crates/keyrill-core/src/session.rs:350-357`](../../crates/keyrill-core/src/session.rs#L350-L357)

```rust
        // Exhaustion is permanent: reset keeps the counter, so every later proposal fails too
        let Some(id) = self.serial.checked_add(1) else {
            self.state = State::Invalidated;
            self.pending = None;
            self.history.clear();
            self.mode = None;
            return Err(Error::Invalidated);
        };
```

A plain `self.serial += 1` would **panic**, stop with an error, in a **debug build**, the
unoptimized build made for development, and wrap around to zero in a **release build**, the
optimized one that ships. Python integers never overflow and JavaScript numbers silently lose
precision, so neither prepares you for this.
[`checked_add`](https://doc.rust-lang.org/std/primitive.u64.html#method.checked_add) returns an
`Option` instead, which makes overflow a case the code must handle. It is reached only after 2^64
plans. The rule it protects is that an expired plan id can never match a later plan, and the type
system doesn't let the code skip the case because it is unlikely.

### An error type with nothing to leak

*Goal: read an error type and its traits.*

[`crates/keyrill-core/src/session.rs:143-162`](../../crates/keyrill-core/src/session.rs#L143-L162)

```rust
/// Session protocol errors; no raw text is included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The id is not the pending plan: a later proposal superseded it, it was already
    /// acknowledged, or a reset expired it.
    Stale,
    /// Session invalidated or token counter exhausted.
    Invalidated,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Stale => "plan is not the latest proposal",
            Self::Invalidated => "session is invalidated",
        })
    }
}

impl std::error::Error for Error {}
```

An error type is an ordinary enum. Implementing
[`Display`](https://doc.rust-lang.org/std/fmt/trait.Display.html) gives it human text, as `__str__`
does in Python and `toString` in JavaScript, and the empty `impl std::error::Error` admits it to the
standard family of errors. The comment says "no raw text is included", and the enum has no field
that could hold any: an error from this crate can be logged without the risk of writing down what
the user typed.

> [!TIP]
> **Remember.** Failure is a value of type `Result` or `Option`, and `?` passes it up. A panic is
> for bugs, not for expected failures.

## Iterators and closures

*Goal: read a chain of iterator methods and a function that takes a closure.*

An [**iterator**](https://doc.rust-lang.org/book/ch13-02-iterators.html) produces values one at a
time, and its methods (`map`, `zip`, `chain`, `filter`) build a pipeline that does nothing until
something asks for the values. Python's generators and
[`itertools`](https://docs.python.org/3/library/itertools.html) work the same way, as do
JavaScript's
[iterator helpers](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Iterator);
JavaScript's `Array.prototype.map` is different, since it builds a whole new array at once. Rust's
compiler turns such a pipeline into a plain loop. The whole layout conversion is one:

[`crates/keyrill-core/src/mapping.rs:60-88`](../../crates/keyrill-core/src/mapping.rs#L60-L88)

```rust
pub fn convert(text: &[u16], keyboard: Keyboard, mapping: Mapping) -> Vec<u16> {
    text.iter()
        .map(|&unit| convert_unit(unit, keyboard, mapping))
        .collect()
}

fn convert_unit(unit: u16, keyboard: Keyboard, mapping: Mapping) -> u16 {
    if mapping == Mapping::Manual && !is_letter(unit) {
        return unit;
    }
    let extra = match keyboard {
        Keyboard::Apple => ('\\', '|'),
        Keyboard::Pc => ('`', '~'),
    };
    for (en, ru) in EN
        .chars()
        .zip(RU.chars())
        .chain(EN_SHIFT.chars().zip(RU_SHIFT.chars()))
        .chain([(extra.0, 'ё'), (extra.1, 'Ё')])
    {
        if mapping != Mapping::RuToEn && unit == en as u16 {
            return ru as u16;
        }
        if mapping != Mapping::EnToRu && unit == ru as u16 {
            return en as u16;
        }
    }
    unit
}
```

`|&unit|` is a closure whose parameter pattern reads the `u16` behind the reference the iterator
yields. `collect()` builds whatever collection the return type names, here a `Vec<u16>`. In
`convert_unit`, `zip` walks two strings of key labels side by side and `chain` appends the shifted
row and the ё key, so the loop visits 66 pairs without an index or a table. The linear search is
deliberate: 66 comparisons per key are nothing next to a round trip to the application, and the
pairs stay readable as the rows of a keyboard.

Iterator methods replace index arithmetic in places where an off-by-one error would hurt. The word
history keeps at most 256 units and must start at a whole word:

[`crates/keyrill-core/src/session.rs:414-426`](../../crates/keyrill-core/src/session.rs#L414-L426)

```rust
                HistoryEffect::Append(text) => {
                    self.history.extend(text);
                    // Trim at a Space so the window starts at a whole word; this can drop one
                    // word more than the bound requires
                    if self.history.len() > MAX_HISTORY {
                        let minimum = self.history.len() - MAX_HISTORY;
                        let remove = self.history[minimum..]
                            .iter()
                            .position(|&unit| unit == 32)
                            .map_or(self.history.len(), |at| minimum + at + 1);
                        self.history.drain(..remove);
                    }
                }
```

`position` finds the first Space at or after the cut; `map_or` turns that into the removal point, or
the whole history when there is no Space; and `drain(..remove)` deletes the prefix in place. The
pattern `HistoryEffect::Append(text)` moves the appended text out of the enum, so `extend` consumes
it without a copy.

### A closure as a parameter

*Goal: see dependency injection with a closure, and a value computed once per process.*

A function may take a closure as a parameter, as a Python function may take a callable.
`impl FnOnce() -> ...` in a parameter list means "any function or closure that can be called once
and returns this":

[`crates/keyrill-ffi/src/lib.rs:178-189`](../../crates/keyrill-ffi/src/lib.rs#L178-L189)

```rust
// Validated once per process; every engine that selects MODEL shares the result, a failure too
fn embedded_model() -> Result<Model<'static>, ModelError> {
    static MODEL: OnceLock<Result<Model<'static>, ModelError>> = OnceLock::new();
    *MODEL.get_or_init(Model::embedded)
}

// The model loader is a parameter so a test can inject an asset failure
fn set_policy(
    engine: &mut Engine,
    policy: u32,
    model: impl FnOnce() -> Result<Model<'static>, ModelError>,
) -> u32 {
```

Production passes `embedded_model`; a test passes a closure that returns an error, and so proves
that a broken asset leaves the policy off. This is **dependency injection**, handing a function its
collaborators instead of letting it reach for them, here with no interface and no virtual call.
`impl FnOnce` is **generic**: the compiler produces a separate copy of `set_policy` for each closure
type it is called with, as a C++ template does, so the call costs nothing at run time. The
Objective-C side does the same with blocks, as the [Objective-C chapter](objective-c.md#blocks)
shows.

[`OnceLock`](https://doc.rust-lang.org/std/sync/struct.OnceLock.html) is a cell that is filled at
most once, safely even when several threads ask at the same moment. Python's
[`functools.cache`](https://docs.python.org/3/library/functools.html#functools.cache) on a function
without arguments is the nearest relative, except that it may run the function twice under
contention and `OnceLock` never does. Here it validates the 5.6 MB model once per process, however
many engines ask for it.

> [!TIP]
> **Remember.** Iterator chains are lazy and compile to loops. A closure parameter is how Rust code
> lets a test swap a dependency.

## Modules, documentation and lints

*Goal: read a crate root, and know which checks the build enforces.*

The crate root, `lib.rs`, is short, and its last lines are a promise the compiler keeps:

[`crates/keyrill-core/src/lib.rs:55-61`](../../crates/keyrill-core/src/lib.rs#L55-L61)

```rust
#![forbid(unsafe_code)]

pub mod inference;
pub mod mapping;
pub mod model;
pub mod session;
pub mod vocabulary;
```

`#![...]`, with the `!`, applies an attribute to the whole crate. A **lint** is a check for a
suspicious pattern that the compiler reports, and each has a level: `allow`, `warn`, `deny` or
`forbid` ([lint levels](https://doc.rust-lang.org/rustc/lints/levels.html)). `forbid` is the
strictest: unlike `deny`, nothing further down can relax it, the way `# noqa` relaxes a Python
linter or `eslint-disable` relaxes ESLint. So there is no `unsafe` code in this crate and no way to
add any quietly (exercises 3 and 4); the next chapter explains what `unsafe` is.

`pub mod` declares a **module**, a namespace whose source is the file of that name, and makes it
public. Items inside are private unless marked `pub`, and the compiler enforces it; Python's leading
underscore is only a convention. That is why `State` and `Pending` above are invisible to every
other crate while `Session` and `Plan` are not.

The lints for the whole workspace live in one place:

[`Cargo.toml:12-18`](../../Cargo.toml#L12-L18)

```toml
[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "deny"
missing_docs = "warn"

[workspace.lints.clippy]
missing_errors_doc = "warn"
missing_panics_doc = "warn"
```

`./tools/verify` runs [Clippy](https://doc.rust-lang.org/clippy/), Rust's extra linter, with
warnings treated as errors, so these warnings fail the build too. `missing_errors_doc` is the reason
every public function that returns a `Result` has an `# Errors` section, as `Snapshot::replace`
above does. Documentation comments (`///`) may contain examples, and `cargo test` compiles and runs
them as tests, like Python's [`doctest`](https://docs.python.org/3/library/doctest.html). The
example at the top of `lib.rs` types `ghbdtn` and Space through a whole session; it is the shortest
complete use of the engine in the repository.

> [!TIP]
> **Remember.** `forbid` can't be relaxed below it. Private is the default, and examples in
> documentation run as tests.

[the rust programming language]: https://doc.rust-lang.org/book/
