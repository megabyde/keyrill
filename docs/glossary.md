# Glossary

The documentation uses abbreviations, language terms and a set of terms with exact meanings in this
project. An abbreviation links to the reference that defines it, and a language term to the section
of the walkthrough that introduces it. A project term has no outside reference, so it is defined
here, with a pointer to the page that treats it in full.

## Abbreviations

| Abbreviation | Meaning |
| -- | -- |
| ABI | [Application binary interface](https://en.wikipedia.org/wiki/Application_binary_interface): names, calling convention and struct layouts at the machine level; Keyrill's is `include/keyrill.h` |
| ADR | [Architecture decision record](https://adr.github.io/); Keyrill's are in [`docs/adr`](adr/README.md) |
| API | [Application programming interface](https://en.wikipedia.org/wiki/API) |
| ARC | [Automatic Reference Counting](https://clang.llvm.org/docs/AutomaticReferenceCounting.html): retain and release inserted by the Objective-C compiler |
| ASan | [AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer.html), clang's memory-error detector |
| BOM | [Byte order mark](https://www.unicode.org/faq/utf_bom.html#BOM), U+FEFF at the start of a text file |
| CC BY-SA | [Creative Commons Attribution-ShareAlike 4.0](https://creativecommons.org/licenses/by-sa/4.0/), the license of the model's word data |
| CI | [Continuous integration](https://en.wikipedia.org/wiki/Continuous_integration); here, the GitHub Actions workflows in `.github/workflows` |
| COM | [Component Object Model](https://learn.microsoft.com/en-us/windows/win32/com/component-object-model--com--portal), the Windows object system TSF is built on |
| DLL | [Dynamic-link library](https://learn.microsoft.com/en-us/troubleshoot/windows-client/setup-upgrade-and-drivers/dynamic-link-library), a Windows shared library |
| EWT | [English Web Treebank](https://universaldependencies.org/treebanks/en_ewt/index.html), one of the evaluation corpora |
| FFI | [Foreign function interface](https://doc.rust-lang.org/nomicon/ffi.html): calling between Rust and another language; here, the crate `keyrill-ffi` |
| GCD | [Grand Central Dispatch](https://developer.apple.com/documentation/dispatch), Apple's queue library |
| GTK | [GTK](https://www.gtk.org/), a toolkit for Linux desktop applications |
| HRESULT | [A Windows status code](https://learn.microsoft.com/en-us/windows/win32/com/structure-of-com-error-codes) returned by COM methods |
| IMK | [InputMethodKit](https://developer.apple.com/documentation/inputmethodkit), the macOS framework for input methods |
| IPC | [Inter-process communication](https://en.wikipedia.org/wiki/Inter-process_communication); every IMK client call is one |
| MSVC | [Microsoft's C and C++ toolchain](https://learn.microsoft.com/en-us/cpp/), and the Rust Windows target that uses it |
| RAII | [Resource acquisition is initialization](https://en.cppreference.com/w/cpp/language/raii): a C++ object whose destructor undoes what its constructor did |
| SHA-256 | [A cryptographic hash](https://en.wikipedia.org/wiki/SHA-2); used to pin the model source and the corpora, for consistency rather than authenticity |
| TIS | Text Input Sources, the Carbon API that lists and selects keyboard layouts and input methods; see [Terms](#terms) |
| TSF | [Text Services Framework](https://learn.microsoft.com/en-us/windows/win32/tsf/text-services-framework), the Windows counterpart of IMK |
| TUI | [Text-based user interface](https://en.wikipedia.org/wiki/Text-based_user_interface), such as a program running in a terminal |
| UBSan | [UndefinedBehaviorSanitizer](https://clang.llvm.org/docs/UndefinedBehaviorSanitizer.html), clang's undefined-behavior detector |
| UD | [Universal Dependencies](https://universaldependencies.org/), the treebank project the evaluation corpora come from |
| UTF-8, UTF-16 | [UTF-8](https://en.wikipedia.org/wiki/UTF-8) and [UTF-16](https://en.wikipedia.org/wiki/UTF-16), Unicode encodings; every length in the ABI counts UTF-16 code units |
| X11 | [The X Window System](https://en.wikipedia.org/wiki/X_Window_System), the older Linux display protocol |

## Language terms

The terms the [walkthrough](learn/README.md) sets in bold, with the section that introduces each.

| Term | Meaning |
| -- | -- |
| [Accessor](learn/objective-c.md#properties) | A getter or setter method; an Objective-C property declares a pair |
| [AppKit](learn/objective-c.md#selectors-and-target-action) | The part of Cocoa for windows, menus and events |
| [ARC](learn/objective-c.md#memory-arc) | Automatic Reference Counting: the Objective-C compiler inserts the retain and release calls |
| [Atomic](learn/boundary.md#threads-and-locks) | A variable whose reads and writes are indivisible even across threads |
| [Attribute](learn/rust.md#types-that-carry-their-meaning) | An annotation for the Rust compiler, written `#[...]`, or `#![...]` for a whole crate |
| [Block](learn/objective-c.md#blocks) | Objective-C's closure, written `^{ ... }` |
| [Borrow checker](learn/rust.md#ownership-and-borrowing) | The part of the Rust compiler that enforces the rules for references |
| [Callback](learn/rust.md#a-type-without-a-lifetime-cant-hold-a-short-lived-borrow) | A function handed to other code for it to call later |
| [Calling convention](learn/boundary.md#why-c-and-what-an-abi-is) | Which registers and stack slots carry a function's arguments and result |
| [Capture](learn/objective-c.md#blocks) | A closure or block keeping a variable from the code around it |
| [Class method](learn/objective-c.md#messages) | An Objective-C method called on the class itself, declared with `+` |
| [Closure](learn/rust.md#taking-a-value-out-exactly-once) | An anonymous function that can use variables from where it was written |
| [Cocoa](learn/objective-c.md#properties) | Apple's Objective-C frameworks for macOS applications |
| [Compile time, run time](learn/rust.md#before-the-first-excerpt-editions-and-checks) | While the compiler translates the program, and while the program runs |
| [Constructor, destructor](learn/objective-cpp.md#raii-across-objective-c-exceptions) | The C++ functions that run when an object is created and when it is destroyed |
| [Crate](learn/README.md#one-rule-before-any-syntax) | Rust's unit of compilation: one library or one program, compiled as a whole |
| [Data race](learn/boundary.md#threads-and-locks) | Two threads touching the same memory, at least one writing, without coordination; undefined behavior |
| [Debug build, release build](learn/rust.md#let--else-and-arithmetic-that-cant-wrap) | The unoptimized build for development, and the optimized one that ships |
| [Dependency injection](learn/rust.md#a-closure-as-a-parameter) | Handing a function its collaborators instead of letting it reach for them |
| [Drop](learn/rust.md#ownership-and-borrowing) | Destroying a Rust value when its owner goes out of scope |
| [Edition](learn/rust.md#before-the-first-excerpt-editions-and-checks) | A per-crate dialect of Rust: 2015, 2018, 2021 or 2024 |
| [Enum, variant](learn/rust.md#types-that-carry-their-meaning) | A Rust type that is one of several variants, each with its own fields |
| [Fake](learn/reentrancy-and-tests.md#a-hostile-fake-client) | A working stand-in for a real dependency, written by hand for tests |
| [Fuzzer](learn/rust.md#errors-as-values) | A tool that feeds a program generated and damaged inputs |
| [Garbage collector](learn/rust.md#ownership-and-borrowing) | The part of Python's or JavaScript's runtime that frees objects nobody refers to |
| [Generic](learn/rust.md#a-closure-as-a-parameter) | Code written once for many types; the Rust compiler makes a copy per type |
| [Guard](learn/boundary.md#threads-and-locks) | In a `match`, a condition after `if`; for a mutex, the value that holds the lock and releases it when dropped |
| [Handle](learn/boundary.md#step-2-hand-ownership-across-and-take-it-back) | A value that identifies an object to code that can't see inside it |
| [Header](learn/boundary.md#why-c-and-what-an-abi-is) | A C file of declarations that other files include |
| [Heap, stack](learn/rust.md#ownership-and-borrowing) | Memory for data of unknown size or lifetime, and memory for a function's local variables |
| [Initializer](learn/objective-c.md#creating-and-destroying-an-object) | An Objective-C method that sets up a new object, the counterpart of a constructor |
| [Input method](learn/README.md) | A program between the keyboard and applications that decides what text a key produces |
| [Input source](learn/README.md#chapters) | The macOS name for a keyboard layout or input method chosen from the menu bar |
| [Instance variable](learn/objective-c.md#a-class-in-two-files) | A private field of an Objective-C object, declared in `@implementation` |
| [Internal linkage](learn/objective-cpp.md#hidden-helpers-and-compile-time-constants) | A name visible only inside its own source file |
| [Iterator](learn/rust.md#iterators-and-closures) | A source of values produced one at a time, on demand |
| [Lifetime](learn/rust.md#lifetimes) | The name, written `'a`, for how long a borrowed reference stays valid |
| [Linker](learn/boundary.md#why-c-and-what-an-abi-is) | The tool that joins compiled files into one program by matching symbols |
| [Lint](learn/rust.md#modules-documentation-and-lints) | A check for a suspicious pattern, at level `allow`, `warn`, `deny` or `forbid` |
| [Lock, mutex](learn/boundary.md#threads-and-locks) | Lets one thread at a time into the code or data it protects; a Rust `Mutex` contains its data |
| [Macro](learn/rust.md#match-keeps-count) | Code the compiler expands into more code; in Rust its name ends in `!` |
| [Main thread, main queue](learn/objective-c.md#queues-instead-of-threads) | The thread on which macOS delivers every input event, and the queue that runs blocks on it |
| [Mangling](learn/boundary.md#step-4-the-anatomy-of-an-exported-function) | Encoding a function's context or parameter types into its symbol name |
| [Message, selector](learn/objective-c.md#messages) | A method call looked up by name at run time, and that name, such as `handleEvent:client:` |
| [Module](learn/rust.md#modules-documentation-and-lints) | A Rust namespace, usually one file |
| [Move](learn/rust.md#ownership-and-borrowing) | Handing a Rust value to a new owner; the old variable can't be used afterwards |
| [Nested run loop](learn/reentrancy-and-tests.md#a-call-that-runs-your-code) | The event loop entered again from inside a call that is waiting |
| [Opaque type](learn/boundary.md#step-2-hand-ownership-across-and-take-it-back) | A struct with a name and no definition, which code can point to but not look inside |
| [Owner](learn/rust.md#ownership-and-borrowing) | The variable or field that a Rust value belongs to |
| [Panic](learn/rust.md#let--else-and-arithmetic-that-cant-wrap) | Rust stopping on a bug; it unwinds the stack unless the build aborts instead |
| [Pattern, arm](learn/rust.md#match-keeps-count) | A shape a value is compared against, and one branch of a `match` |
| [Process](learn/boundary.md#step-3-stop-panics-at-the-boundary) | A running program |
| [Property](learn/objective-c.md#properties) | An Objective-C declaration of a getter and setter, written by the compiler |
| [Property test](learn/reentrancy-and-tests.md#property-tests-in-rust) | A test that checks a law over many generated inputs |
| [Protocol](learn/objective-c.md#protocols-and-forward-declarations) | A named list of methods any Objective-C class may adopt |
| [Queue, serial queue](learn/objective-c.md#queues-instead-of-threads) | A GCD list of blocks to run; a serial queue runs them one at a time, in order |
| [RAII](learn/objective-cpp.md#raii-across-objective-c-exceptions) | Tying an action to a C++ object's lifetime, so its destructor undoes it on every exit |
| [Raw pointer](learn/boundary.md#where-unsafe-lives) | A plain memory address with no lifetime and no guarantee it is valid |
| [Reentrancy](learn/reentrancy-and-tests.md#a-call-that-runs-your-code) | Code entered again before its first run has finished |
| [Reference](learn/rust.md#ownership-and-borrowing) | Access to a value without owning it: `&T` shared, `&mut T` exclusive; in C++, another name for an object |
| [Retain count, retain cycle](learn/objective-c.md#memory-arc) | The number of owners of an Objective-C object, and two objects that own each other and so are never freed |
| [Run loop](learn/objective-c.md#where-the-process-starts) | The main thread's event loop in a macOS program |
| [Seed](learn/reentrancy-and-tests.md#property-tests-in-rust) | The starting value of a random generator; fixing it makes a test repeat exactly |
| [Shrinking](learn/reentrancy-and-tests.md#property-tests-in-rust) | A property-test tool reducing a failing input to a small one |
| [Slice](learn/rust.md#lifetimes) | A view of part of an array: a pointer and a length |
| [Standard library](learn/rust.md) | The library every Rust program gets with the compiler |
| [Static library](learn/boundary.md#why-c-and-what-an-abi-is) | A file of compiled code the linker copies into a program |
| [Struct, field](learn/rust.md#types-that-carry-their-meaning) | A record of named fields |
| [Symbol](learn/boundary.md#why-c-and-what-an-abi-is) | The name a function or variable has in compiled code |
| [Tagged union](learn/rust.md#types-that-carry-their-meaning) | A value that is one of several variants and records which |
| [Thread](learn/rust.md#shared-data-that-never-changes) | An independent sequence of execution inside one process |
| [Trait](learn/rust.md#types-that-carry-their-meaning) | A named set of methods a Rust type can provide |
| [Translation unit](learn/objective-cpp.md) | One source file with every header it includes, compiled as one unit |
| [Undefined behavior](learn/boundary.md#where-unsafe-lives) | An operation the language gives no meaning to; the program may crash, corrupt memory or seem to work |
| [Unsafe](learn/boundary.md#where-unsafe-lives) | Rust code in which a person vouches for what the compiler can't prove |
| [Unwinding](learn/boundary.md#step-3-stop-panics-at-the-boundary) | Leaving each function in turn and cleaning up, as an exception or panic propagates |
| [UTF-16 code unit](learn/rust.md#match-keeps-count) | One 16-bit value of UTF-16 text; most characters take one, some take two |
| [Workspace](learn/rust.md#before-the-first-excerpt-editions-and-checks) | The set of Rust crates built together from one root `Cargo.toml` |
| [Worktree](learn/exercises.md) | A second checkout of the same Git repository in another directory |

## Terms

**Acknowledgement.** The adapter's report of what happened to a plan, through `kr_commit`: verified,
refused before the write, or unknown. Only a verified acknowledgement advances the Rust session. See
[Rust in the core](learn/rust.md#taking-a-value-out-exactly-once).

**Adapter.** The platform code that owns the native objects and the editing: on macOS, the
Objective-C++ under `native/macos`. It calls the engine through the C ABI and never the other way
round. See [macOS adapter](macos.md).

**Boundary.** The context in which a letter may start a word: the caret at the start of the document
or after whitespace, read back from the client. Anything else, including a prefix that couldn't be
read, is *ineligible*, and the letter goes to the application untouched.

**Boundary mark.** A word and its Space, still marked after Space so that a correction can be undone
by replacing Keyrill's own mark. It ends with the next key. ARCHITECTURE.md calls the state
BoundaryRetained.

**Compatibility backend.** The macOS backend for applications that can't hold an owned composition,
such as terminals: it posts synthetic key events and treats them as dispatched, never as verified
edits. It is the one exception to owned-composition-only editing; see
[ADR 006](adr/006-compatibility-backend.md).

**Controlled-native evidence.** A result from the native tests, which run the real adapter and the
real Rust engine against a scripted fake client. It shows what the adapter does on a schedule; it
shows nothing about a real application, which only a physical test can. See [Testing](testing.md).

**Explicit intent.** A manual conversion or a reversal by the user. It stops automatic inference for
the rest of that word, so inference never undoes a choice the user made.

**Fixture policy.** A test policy that converts exactly `ghbdtn` and `руддщ` and nothing else, so
that session tests don't depend on the model.

**Generation.** A counter that grows on every event invalidating what the adapter knows. An
operation remembers the generation it started under and compares after each native call; a
difference means a callback ran in between, and the operation stops. See
[Reentrancy and tests](learn/reentrancy-and-tests.md#generation-and-busy).

**Guard.** The compatibility backend's listen-only event tap. It observes mouse, navigation,
shortcut and source changes without reading characters; each one ends tracking and advances the
process-wide guard generation, which every rewrite rechecks before it posts. See
[macOS](macos.md#compatibility).

**Inference.** The engine's decision, at a word boundary, whether a finished word was typed in the
wrong layout. It uses the frozen model, the user's dictionaries and recent context, and leaves the
word alone whenever it is unsure. See [Engine](engine.md#inference).

**Input mode.** Keyrill EN or Keyrill RU, the two input sources Keyrill installs; the mode decides
which layout the keys type in.

**Learned words.** Words the user taught the OS spell checker. They protect and forbid words like
the Vim dictionaries, but automatic correction never waits for them.

**Local authority.** The adapter's record of the mark it verified last: the client, the start offset
and the exact text. A write needs it, and the adapter drops it before every write and regains it
only when the write's checks pass, which for a mark update means a matching readback.

**Mark, owned composition.** Marked text, the underlined composition of an input method, that
Keyrill itself placed with `setMarkedText` and still owns. It is the only text the composition path
may change; the compatibility backend is the one exception. See
[How Keyrill works](how-it-works.md#why-an-input-method).

**Plan.** Rust's answer to one key: an id, one permitted action (pass, mark, finish or unavailable),
the text the mark must hold before the write, the text to write, and what to do with the key
afterwards. A plan is a proposal, never editing authority.

**Poisoned handle.** An engine or vocabulary handle in which a Rust panic was caught. Every later
call on it returns `KR_POISONED`; only freeing it remains. See
[The C boundary](learn/boundary.md#step-3-stop-panics-at-the-boundary).

**Provisional key.** A key such as `[` or `;` that types punctuation in English but a Russian letter
in Russian. In English mode it may start or continue a word, because the word may turn out to be
Russian typed in the wrong layout.

**Readback.** Reading the composition back from the client after a write, over exactly the range
written. It is the only evidence of a write's effect that the adapter accepts; an API's success
return is not. The one write verified without it is an unchanged commit that IMK or the controller
requests, which writes back the text last verified in the mark; see the
[edit contract](../ARCHITECTURE.md#edit-contract).

**Receipt.** The text a word had before Keyrill first converted it, kept while the same word is
owned so that Control-Option-Z or Escape can restore it. An edit or finishing the word expires it.

**Secure Input.** The macOS mode in which keyboard input goes only to the focused application, used
for password fields
([TN2150](https://developer.apple.com/library/archive/technotes/tn2150/_index.html)). While it is
on, Keyrill passes every key and inspects nothing.

**Snapshot.** An immutable, fully parsed copy of a dictionary, published whole as an `Arc` and never
edited in place. A failed parse leaves the last good snapshot in force.

**Tier.** How Keyrill handles one application, chosen by its bundle identifier: composition,
compatibility (automatic and manual, or manual only) or off. See
[macOS adapter](macos.md#application-tiers).

**TIS.** Text Input Sources, the Carbon interface in `HIToolbox/TextInputSources.h` (functions named
`TIS...`) that lists, enables and selects input sources. Apple documents it only in that header.
Keyrill uses it to list input sources and read their properties and to observe source changes, and
avoids it where its answers are unreliable inside the input method's own process
([Input-source switching](macos.md#input-source-switching)).

**Unknown outcome.** The acknowledgement for a write that was attempted but whose effect couldn't be
verified. It invalidates the session and stops correction; nothing is repaired or replayed. The
[edit contract](../ARCHITECTURE.md#edit-contract) lists the one write still allowed afterwards and
when correction resumes.
