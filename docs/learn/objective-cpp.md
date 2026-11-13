# Objective-C++

Objective-C++ is what clang compiles when a file ends in `.mm`: C++ and Objective-C in the same
**translation unit**, the one source file plus every header it includes, which the compiler turns
into one compiled object file. So a C++ class may hold an Objective-C object, an Objective-C method
may take a C++ reference, and a block may capture a C++ value. Apple's description of the mixture
was in a guide that is no longer published;
[Wikipedia](https://en.wikipedia.org/wiki/Objective-C#Objective-C++) summarizes the rules, and
clang's [ARC specification](https://clang.llvm.org/docs/AutomaticReferenceCounting.html) is the
reference where memory is involved.

The previous chapter showed the Objective-C in the adapter. This one shows what C++ adds and, for
each construct, why the adapter uses it. If you don't know C++, read each section for the problem it
solves; the C++ syntax is explained where it appears.

## Which language is which

*Goal: tell C, Objective-C and C++ apart in one file.*

The build line from `native/macos/build.sh` names all three:
`clang++ -std=c++20 -fobjc-arc -fobjc-exceptions`.

| Construct | Language | Where |
| -- | -- | -- |
| `[client markedRange]`, `@interface`, `@property`, `^{ }`, `@try` | Objective-C | Every adapter file |
| `kr_event`, `kr_plan`, `NSRange`, `#include "keyrill.h"` | C | The ABI and Apple's C types |
| `namespace { }`, `constexpr`, `nullptr`, `static_cast`, `enum class` | C++ | Helpers and constants |
| `struct CallbackScope` with a destructor | C++ | `KRController.mm` |
| `- (uint32_t)apply:(const kr_plan &)plan ...` | Objective-C with a C++ reference | `KRSession.mm` |
| `std::atomic<bool> _stopped;` as an instance variable | C++ inside Objective-C | `KRRuntime.mm` |
| `struct Choice { NSString *title; SEL action; ... }` | Objective-C inside C++ | `KRController.mm` |
| `std::vector`, `std::copy`, `std::memcmp`, `std::string`, `std::array` | C++ standard library | `main.mm`, `KRKeys.mm`, `KRSession.mm`, `KRRuntime.mm` |

> [!TIP]
> **Remember.** Brackets and `@` are Objective-C; `std::`, `namespace` and `&` in a parameter are
> C++; the structs from the header are C.

## C linkage

*Goal: see why the header wraps its declarations in `extern "C"`.*

C++ allows two functions with the same name and different parameters, so it encodes the parameter
types into each symbol name; this is the
[name mangling](https://en.wikipedia.org/wiki/Name_mangling) the
[C boundary](boundary.md#step-4-the-anatomy-of-an-exported-function) chapter turned off on the Rust
side. C doesn't mangle, and Rust exports `kr_commit` under exactly that name. The adapter's `.mm`
files are C++, so when they include the header, every declaration in it must be marked as having
[C language linkage](https://en.cppreference.com/w/cpp/language/language_linkage); otherwise the
compiler asks the linker for a mangled name the Rust library never defined:

[`include/keyrill.h:54-62`](../../include/keyrill.h#L54-L62)

```c
#ifndef KEYRILL_H
#define KEYRILL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
```

`__cplusplus` is defined only when the compiler is compiling C++, so a C compiler sees plain
declarations and a C++ compiler sees them inside `extern "C" { ... }`, closed near the end of the
file just before the C++ `static_assert` block shown in
[the C boundary](boundary.md#step-1-agree-on-memory-layout). The same header serves the C smoke
test, which `./tools/verify` compiles as C11, and the adapter, which compiles it as C++20. Exercise
7 removes the block and shows the linker's complaint.

The adapter's own functions don't need this. `KREventForKey` is declared in `KRKeys.h` and defined
in `KRKeys.mm`, and every file that calls it is C++, so its mangled name agrees on both sides:

[`native/macos/src/KRKeys.h:10-11`](../../native/macos/src/KRKeys.h#L10-L11)

```objc
kr_event KREventForKey(unsigned short keyCode, NSEventModifierFlags flags, NSString *characters,
                       uint32_t mode);
```

> [!TIP]
> **Remember.** C++ mangles names and C doesn't. `extern "C"` makes C++ code look for the plain
> names that Rust exports.

## Hidden helpers and compile-time constants

*Goal: read an unnamed namespace and `constexpr`.*

A helper used by one file only should be invisible to the linker, so that another file can't call it
or collide with its name; C++ calls that **internal linkage**. C gets it with `static`, and C++ adds
the [unnamed namespace](https://en.cppreference.com/w/cpp/language/namespace#Unnamed_namespaces),
which hides everything inside it, types included. A Python module gets the same effect only by the
convention of a leading underscore:

[`native/macos/src/KRSession.mm:6-24`](../../native/macos/src/KRSession.mm#L6-L24)

```obj-c++
namespace {

constexpr NSRange kCurrentSelection = {NSNotFound, 0};
// Keeps start + KR_TEXT_CAPACITY far from overflow without a check at every addition
constexpr NSUInteger kMaxStart = NSUIntegerMax - 4 * KR_TEXT_CAPACITY;
NSUInteger callbackDepth = 0;
bool callbackDepthSaturated = false;

bool hasMark(NSRange range) { return range.location != NSNotFound && range.length != 0; }

bool isWhitespace(unichar unit) {
  return unit == u' ' || unit == u'\t' || unit == u'\n' || unit == u'\r' || unit == 0x00A0;
}

NSString *string(const uint16_t *units, uint32_t length) {
  return [[NSString alloc] initWithCharacters:units length:length];
}

} // namespace
```

[`constexpr`](https://en.cppreference.com/w/cpp/language/constexpr) asks for a value computed at
compile time, so `kCurrentSelection`, an `NSRange` (a plain C struct from Foundation), is built by
the compiler. `u' '` is a
[`char16_t` literal](https://en.cppreference.com/w/cpp/language/character_literal), one UTF-16 code
unit, which compares directly with Foundation's `unichar`. The last helper shows that a C++ function
may contain Objective-C: it returns a new `NSString`, and ARC manages the result exactly as it would
in a method.

The two counters in the namespace are process-wide state that the class methods
`+beginInputCallback` and `+endInputCallback` update. Objective-C has no class variables, so
file-level variables play that part, and the namespace keeps them out of reach of every other file.

> [!TIP]
> **Remember.** An unnamed namespace hides a file's helpers from every other file. `constexpr`
> values are computed by the compiler.

## RAII across Objective-C exceptions

*Goal: see why one small C++ struct is the main reason these files are Objective-C++.*

[**RAII**](https://en.cppreference.com/w/cpp/language/raii), "resource acquisition is
initialization", ties an action to the lifetime of a local object: its **constructor** does the
action, its **destructor** undoes it, and the destructor runs on every way out of the scope, by
`return` or by exception. Python's
[`with` statement](https://docs.python.org/3/reference/compound_stmts.html#the-with-statement) does
the same job explicitly, and JavaScript code writes `try`/`finally` by hand. The controller uses
RAII to count how deep the process is inside callbacks from the application:

[`native/macos/src/KRController.mm:15-24`](../../native/macos/src/KRController.mm#L15-L24)

```obj-c++
namespace {

// Marks a host callback for the whole entry, including exception unwinding, so vocabulary
// adoption never runs inside one, even from a nested run loop serving another controller
struct CallbackScope {
  CallbackScope() { [KRSession beginInputCallback]; }
  ~CallbackScope() { [KRSession endInputCallback]; }
  CallbackScope(const CallbackScope &) = delete;
  CallbackScope &operator=(const CallbackScope &) = delete;
};
```

[`native/macos/src/KRController.mm:614-618`](../../native/macos/src/KRController.mm#L614-L618)

```obj-c++
- (BOOL)handleEvent:(NSEvent *)event client:(id)sender {
  if (!NSThread.isMainThread || !_session)
    return NO;
  CallbackScope scope;
  __attribute__((objc_precise_lifetime)) KeyrillAlphaController *retained = self;
```

A C++ constructor and destructor send Objective-C messages, and the object lives inside an
Objective-C method. The
[deleted](https://en.cppreference.com/w/cpp/language/function#Deleted_functions) copy constructor
and copy assignment (`= delete`) make it impossible to duplicate a scope by accident, which would
end the callback twice. `handleEvent:` has a dozen `return` statements; a matching
`endInputCallback` call before each of them would be a dozen chances to forget one.

The comment's phrase "including exception unwinding" is the reason this file is Objective-C++ and
not Objective-C. In the 64-bit runtime, Objective-C exceptions are implemented on the same unwinding
machinery as C++ exceptions, and Apple's documentation states that C++ destructors and `@finally`
blocks both run while an Objective-C exception unwinds
([Exceptions in 64-Bit Executables](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Exceptions/Articles/Exceptions64Bit.html)).
So an application's exception that escapes `handleEvent:` still ends the callback scope. Plain
Objective-C has `@finally` for that, but nothing that cleans up by itself.

The same holds for ARC. By default in Objective-C, ARC is *not* exception-safe: a strong local
variable in a function that an exception passes through is never released. The
[ARC specification](https://clang.llvm.org/docs/AutomaticReferenceCounting.html#exceptions) explains
that Cocoa treats exceptions as fatal, so the cleanup code would cost size for nothing, and then
adds: "In Objective-C++, `-fobjc-arc-exceptions` is enabled by default." An input method that
catches an application's exceptions and keeps running is exactly the program that needs the cleanup,
and the `.mm` extension gives it without a flag.

> [!TIP]
> **Remember.** A C++ destructor runs even when an Objective-C exception passes through, and in
> Objective-C++ ARC releases objects on that path too.

## C++ references in Objective-C methods

*Goal: read `const T &` in a method signature.*

A C++ **reference** is another name for an existing object; a `const` reference may be read and not
written through, and unlike a pointer it can't be null or be pointed somewhere else
([references](https://en.cppreference.com/w/cpp/language/reference)). Python and JavaScript pass
every object this way already, so the novelty is only that C++ makes you ask for it. The plan is a
536-byte struct, and `apply:` takes it by const reference instead of copying it:

[`native/macos/src/KRSession.mm:445-447`](../../native/macos/src/KRSession.mm#L445-L447)

```obj-c++
- (uint32_t)apply:(const kr_plan &)plan
           client:(id<IMKTextInput>)client
       generation:(uint64_t)generation {
```

A function can also return a reference, which lets a helper choose *which* field of a C struct the
caller assigns to:

[`native/macos/src/KRRuntime.mm:24-26`](../../native/macos/src/KRRuntime.mm#L24-L26)

```obj-c++
KRSourceStatus &source(KRRuntimeStatus &status, int index) {
  return index == kLearned ? status.learned : index ? status.russian : status.english;
}
```

[`native/macos/src/KRRuntime.mm:206-207`](../../native/macos/src/KRRuntime.mm#L206-L207)

```obj-c++
      source(_status, i) =
          _paths[i] == PathSetting::Unusable ? KRSourceStatusMissing : KRSourceStatusPending;
```

`KRRuntimeStatus` is a C struct declared in `KRRuntime.h` with three status fields. Without the
reference, choosing a field by number would need a pointer, or a `switch` at every assignment.

> [!TIP]
> **Remember.** A C++ reference is a name for an existing object that can't be null. Objective-C++
> methods may take and return them.

## C++ objects inside Objective-C objects

*Goal: see C++ fields in an Objective-C class, and why two of them are atomic.*

An Objective-C class may have instance variables of C++ class type. The compiler generates hidden
methods that run their constructors when the object is allocated and their destructors before its
memory is freed, so a `std::string` field starts empty and is destroyed with its object, like a
member of a C++ class. The dictionary loader's fields show several kinds:

[`native/macos/src/KRRuntime.mm:117-139`](../../native/macos/src/KRRuntime.mm#L117-L139)

```obj-c++
@implementation KRRuntime {
  KRRuntimeStatus _status;
  uint32_t _enginePolicy;
  kr_vocabulary *_vocabulary;
  PathSetting _paths[2];
  std::string _explicitPaths[2];
  NSString *_Nullable (^_homeResolver)(void);
  dispatch_queue_t _queue;
  void (^_mainExecutor)(dispatch_block_t);
  uint64_t _interval;
  dispatch_source_t _timer;
  bool _configured[kSources];
  bool _started;
  __weak id<KRVocabularyAdopting> _target;
  // Written on main; read by observation rounds to keep retrying a refused adoption
  std::atomic<bool> _needsAdoption;
  std::atomic<bool> _stopped;
  NSDictionary<NSString *, NSNumber *> *_builtIn;
  // Work-queue confined once observation begins
  bool _observing;
  Sources _sources;
  bool _active[kSources];
}
```

Five kinds of member sit side by side: C structs and a C handle; an array of a C++ `enum class`
(`PathSetting`, a
[scoped enumeration](https://en.cppreference.com/w/cpp/language/enum#Scoped_enumerations) whose
values must be written with their type's name, declared in the file's unnamed namespace); C++
library objects; blocks; and strong and weak pointers to objects. Under ARC even `dispatch_queue_t`
is an object pointer, retained and released like any other.

The two [`std::atomic<bool>`](https://en.cppreference.com/w/cpp/atomic/atomic) fields are the reason
for C++ here. The loader is used from two queues: `_stopped` is written on the main thread and read
in blocks on the dictionary queue, and an ordinary `bool` shared that way is a data race, undefined
behavior in C++ as in Rust ([the C boundary](boundary.md#threads-and-locks)). Objective-C has no
atomic type of its own; C11's `_Atomic` would do in a `.m` file, and in a `.mm` file `std::atomic`
is the idiom. The comment on `_observing` names the other discipline, confinement: that field is
touched only on the work queue, so it needs no atomic.

A block that captures a C++ object copies it with the object's copy constructor. That is how the
loader hands its configured state to the work queue: by value, so the two queues share nothing that
changes:

[`native/macos/src/KRRuntime.mm:316-326`](../../native/macos/src/KRRuntime.mm#L316-L326)

```obj-c++
// Hands the configured state to the work queue, then observes immediately and periodically
- (void)beginObserving {
  Sources sources = {{_status.english, _status.russian, _status.learned}, {}};
  std::array<bool, kSources> configured = {_configured[0], _configured[1], _configured[kLearned]};
  dispatch_async(_queue, ^{
    self->_observing = true;
    self->_sources = sources;
    for (int i = 0; i < kSources; ++i)
      self->_active[i] = configured[i];
    [self observe];
  });
```

The `std::array` is there for the capture: a block can't capture a plain C array, but it can capture
a C++ object that contains one.

> [!TIP]
> **Remember.** C++ fields in an Objective-C object are constructed and destroyed with it. Share a
> flag between queues as `std::atomic`, or confine it to one queue.

## Objective-C objects inside C++ objects

*Goal: see ARC manage object pointers stored in a C++ struct.*

The reverse also works. A C++ struct may hold pointers to Objective-C objects, and ARC retains and
releases them as the struct is created, copied and destroyed. The input menu describes its items as
a local table:

[`native/macos/src/KRController.mm:450-469`](../../native/macos/src/KRController.mm#L450-L469)

```obj-c++
  struct Choice {
    NSString *title;
    SEL action;
    NSString *name;
  };
  const Choice items[] = {
      {[NSString stringWithFormat:@"Default (%@)", tierName(fallback)], @selector(useDefaultTier:),
       nil},
      {@"Direct Editing", @selector(useCompositionTier:), @"composition"},
      {@"Compatibility, Automatic and Manual", @selector(useAutomaticTier:), @"automatic"},
      {@"Compatibility, Manual Only", @selector(useManualTier:), @"manual"},
      {@"Off", @selector(useOffTier:), @"off"},
  };
  for (const Choice &choice : items) {
    NSMenuItem *item = [choices addItemWithTitle:choice.title
                                          action:choice.action
                                   keyEquivalent:@""];
    bool selected = choice.name ? [chosen isEqualToString:choice.name] : !chosen;
    item.state = selected ? NSControlStateValueOn : NSControlStateValueOff;
  }
```

A struct declared inside a function; an array initialized from a list in braces; and a
[range-based `for`](https://en.cppreference.com/w/cpp/language/range-for), C++'s counterpart of
Python's `for choice in items:`, here by const reference. The first title is built when the code
runs and has no other owner; the array holds it, strongly, until the function returns.

> [!TIP]
> **Remember.** ARC manages object pointers inside C++ structs as it does anywhere else.

## The standard library at the edges

*Goal: read the places where C++'s standard library meets C and Objective-C.*

A C function that fills a buffer the caller provides takes the buffer of a `std::vector`, C++'s
growable array, directly. The home directory comes from the account database, with a buffer sized by
`sysconf`:

[`native/macos/src/main.mm:13-24`](../../native/macos/src/main.mm#L13-L24)

```obj-c++
// The account database, not $HOME, names the directory holding the default personal sources
static NSString *accountHome() {
  long size = sysconf(_SC_GETPW_R_SIZE_MAX);
  std::vector<char> buffer(size > 0 ? static_cast<size_t>(size) : 16384);
  passwd entry;
  passwd *result = nullptr;
  if (getpwuid_r(getuid(), &entry, buffer.data(), buffer.size(), &result) != 0 || !result ||
      !result->pw_dir)
    return nil;
  return [NSFileManager.defaultManager stringWithFileSystemRepresentation:result->pw_dir
                                                                   length:strlen(result->pw_dir)];
}
```

Note the two kinds of null side by side: `nullptr` for a C pointer, `nil` for an object. Both are
zero; the spelling says which world the value belongs to. `static_cast` replaces the C cast where
the conversion changes a value's type in an ordinary way, so a reader searching for dangerous casts
finds the one `reinterpret_cast` in the adapter, in `KRRuntime.mm`, which turns `std::string` bytes
into the `const uint8_t *` the C ABI wants.

The key mapper is the plainest mixture in the adapter, and a good place to read Objective-C++ for
the first time:

[`native/macos/src/KRKeys.mm:7-17`](../../native/macos/src/KRKeys.mm#L7-L17)

```obj-c++
kr_event KREventForKey(unsigned short keyCode, NSEventModifierFlags flags, NSString *characters,
                       uint32_t mode) {
  kr_event event = {KR_FINISH_REQUEST, mode, KR_CTX_INELIGIBLE, 0, {}};
  NSEventModifierFlags modifiers =
      flags & (NSEventModifierFlagCommand | NSEventModifierFlagControl | NSEventModifierFlagOption |
               NSEventModifierFlagShift);
  constexpr NSEventModifierFlags chord = NSEventModifierFlagControl | NSEventModifierFlagOption;
  if (modifiers == chord && keyCode == kVK_ANSI_K) {
    event.role = KR_MANUAL;
    return event;
  }
```

[`native/macos/src/KRKeys.mm:32-47`](../../native/macos/src/KRKeys.mm#L32-L47)

```obj-c++
  NSUInteger length = characters.length;
  if (length == 0 || length > KR_EVENT_CAPACITY)
    return event;
  uint16_t units[KR_EVENT_CAPACITY];
  [characters getCharacters:units range:NSMakeRange(0, length)];
  for (NSUInteger i = 0; i < length; ++i) {
    uint16_t unit = units[i];
    // Control characters (Return, Tab, Backspace with Shift) and AppKit function-key units
    if (unit < 0x20 || unit == 0x7F || (unit >= 0xF700 && unit <= 0xF8FF))
      return event;
  }
  event.role = KR_TEXT;
  event.len = static_cast<uint32_t>(length);
  std::copy(units, units + length, event.units);
  return event;
}
```

A C struct from the header, initialized from a list in braces in which `{}` zeroes the units; a
`constexpr` mask; a message to an `NSString`; `std::copy` into the struct's array. The first line is
also the function's policy: the event starts as `KR_FINISH_REQUEST`, so a key this function can't
classify ends the word and goes to the application, and every early `return` inherits that cautious
answer. The length check comes before `getCharacters:`, which writes into a fixed array on the
stack.

> [!TIP]
> **Remember.** `nullptr` is a null C pointer and `nil` a null object. Start from the cautious
> answer and let every early return inherit it.

## `BOOL` and `bool`

*Goal: know why the code never compares with `YES`.*

The adapter uses both, and whether they are the same type depends on the target. `BOOL` is
Objective-C's: a real `bool` on Apple silicon, but a `signed char` on Intel Macs, where any nonzero
byte counts as true and comparing a `BOOL` with `YES` can fail. The code follows one rule: `BOOL`
where an Objective-C API says `BOOL`, in method signatures and properties, and C++ `bool` in C++
helpers and local variables. It never compares with `YES`.

> [!TIP]
> **Remember.** Use `BOOL` at Objective-C interfaces and `bool` inside C++, and test a `BOOL` for
> truth, not for equality with `YES`.
