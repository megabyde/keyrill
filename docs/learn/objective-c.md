# Objective-C

[Objective-C](https://en.wikipedia.org/wiki/Objective-C) is C with one new expression, the message
send, borrowed from [Smalltalk](https://en.wikipedia.org/wiki/Smalltalk), and an object model built
on it. Everything C allows is still allowed, which is why the adapter can keep a raw `kr_engine *`
inside an object and pass a `kr_event` by pointer. This chapter covers the Objective-C half of the
adapter: the syntax, the objects, how their memory is managed and how work moves between threads.
The C++ half is the [next chapter](objective-cpp.md).

Apple's
[Programming with Objective-C](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/Introduction/Introduction.html)
is the reference for the language. It is archived, and the language hasn't changed in the parts this
code uses.

## Messages

*Goal: read a bracketed message send and name the method it calls.*

The brackets look strange for about an hour. `[receiver message]` sends a **message** to an object,
and the method that runs is looked up *by name at run time*
([dynamic dispatch](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ObjCRuntimeGuide/Articles/ocrtHowMessagingWorks.html)).
If you come from Python or JavaScript, this is the familiar part: `obj.method()` there is also a
lookup by name when the line runs. C++ is the odd one out, with a table of virtual functions fixed
at compile time.

Arguments are written between the parts of the method's name.
`[session handleEvent:&input client:client]` calls one method whose full name, its **selector**, is
`handleEvent:client:`, with one colon per argument. A method declared with `-` is an **instance
method**, called on an object; a method declared with `+` is a **class method**, called on the class
itself, like a Python `@classmethod` or a C++ `static` member function.

| Objective-C | C++ | Python | JavaScript |
| -- | -- | -- | -- |
| `[client markedRange]` | `client->markedRange()` | `client.markedRange()` | `client.markedRange()` |
| `[s handleEvent:e client:c]` | `s->handleEvent(e, c)` | `s.handleEvent(e, client=c)` | `s.handleEvent(e, c)` |
| `@interface A : NSObject <P>` | `class A : public NSObject, public P` | `class A(NSObject, P)` | `class A extends NSObject`, plus an interface in TypeScript |
| `@property(nonatomic) uint32_t mode;` | getter, setter and a field | `@property` with a setter | `get mode()` and `set mode(v)` |
| `^BOOL { return YES; }` | a lambda | a `lambda` or nested `def` | an arrow function |
| `@"text"`, `@[a, b]`, `@{k : v}`, `@(n)` | library types | `"text"`, `[a, b]`, `{k: v}`, `n` | `"text"`, `[a, b]`, `{k: v}`, `n` |
| `nil`, and `[nil anything]` does nothing and gives zero for objects and numbers | `nullptr`, but a call on it crashes | `None`, and a call on it raises | `null`, closest to `obj?.method()` |
| `YES`, `NO`, `BOOL` | `true`, `false`, `bool` | `True`, `False` | `true`, `false` |

> [!TIP]
> **Remember.** `[object part1:a part2:b]` calls the method `part1:part2:`, looked up by name at run
> time.

## A class in two files

*Goal: read a class's public declaration and its private implementation.*

A class has a public face, the `@interface` in a header, and a body, the `@implementation` in the
source file. Here is the session's interface, with its class methods and its one initializer:

[`native/macos/src/KRSession.h:76-92`](../../native/macos/src/KRSession.h#L76-L92)

```objc
@interface KRSession : NSObject <KRVocabularyAdopting>

/// Process-wide depth of host input callbacks on the main thread. While it is nonzero,
/// vocabulary adoption waits and runtime replacement and deferred mode requests are refused,
/// because a nested run loop inside one controller's callback can run work queued for another.
/// Begin and end off main are no-ops, and the query returns YES off main. End at zero is a no-op.
/// Overflow saturates and defers adoption until process exit.
+ (void)beginInputCallback;
+ (void)endInputCallback;
+ (BOOL)inputCallbackActive;

/// Takes ownership of `engine` and frees it in dealloc. `allowedClients` lists the bundle
/// identifiers in which a composition may start; a nonempty client bundle identifier and every
/// secure-input, ownership and readback check remain required.
- (instancetype)initWithEngine:(kr_engine *)engine
                allowedClients:(NSSet<NSString *> *)allowedClients NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;
```

`NSObject` is the root class, the ancestor of every object here. `instancetype` means "an instance
of the class this was called on", so a subclass's initializer returns the subclass. The two macros
at the end are checked by the compiler: `NS_DESIGNATED_INITIALIZER` marks the one initializer every
other must call, and `NS_UNAVAILABLE` removes the inherited `init`, so `[[KRSession alloc] init]`, a
session with no engine, is a compile error rather than a crash later. `NSSet<NSString *> *` is a
*lightweight generic*: a hint the compiler checks where the method is called, and forgets at run
time, like a Python type annotation checked by mypy.

The private fields, **instance variables**, go in braces after `@implementation`, in the source
file, so they never appear in the header at all. A C++ header shows every private member to whoever
includes it; here the header says nothing about the representation:

[`native/macos/src/KRSession.mm:26-40`](../../native/macos/src/KRSession.mm#L26-L40)

```obj-c++
@implementation KRSession {
  kr_engine *_engine;
  NSSet<NSString *> *_allowedClients;
  // Bumped by every invalidation; each client call is followed by a comparison with the value
  // taken before it
  uint64_t _generation;
  // A plan is in flight; a callback that finds it set came through a nested run loop
  BOOL _busy;
  BOOL _poisoned;
  BOOL _finishVerified;
  // Local authority: the client, document start and exact text of the verified mark; the caret
  // is at its end
  id<IMKTextInput> _owner;
  NSUInteger _start;
  NSString *_text;
```

These comments are design notes, and the rest of the walkthrough leans on them; `_generation` and
`_busy` are the subject of [Reentrancy and tests](reentrancy-and-tests.md). Note that the handle
from Rust is an ordinary C pointer field, side by side with pointers to objects.

### Creating and destroying an object

*Goal: read the standard initializer and know what `dealloc` is for.*

Creation is two messages, `[[KRSession alloc] initWithEngine:... allowedClients:...]`: `alloc`
reserves memory filled with zeros, and the **initializer**, the counterpart of a constructor or
Python's `__init__`, fills it. Initialization is a convention rather than a language feature, and
this is its standard shape:

[`native/macos/src/KRSession.mm:75-91`](../../native/macos/src/KRSession.mm#L75-L91)

```obj-c++
- (instancetype)initWithEngine:(kr_engine *)engine
                allowedClients:(NSSet<NSString *> *)allowedClients {
  if ((self = [super init])) {
    _engine = engine;
    _allowedClients = [allowedClients copy];
    _mode = KR_EN;
    _contextLocation = NSNotFound;
    _secureInputEnabled = ^BOOL {
      return IsSecureEventInputEnabled();
    };
  }
  return self;
}

- (void)dealloc {
  kr_engine_free(_engine);
}
```

`self = [super init]` lets the parent class initialize its part. The parent may return `nil`, or
even a different object, so the result is assigned and checked. The double parentheses tell the
compiler that the assignment inside `if` is intended. `[allowedClients copy]` keeps a private copy
that can't change, because the caller might have passed a mutable set and changed it later. The
block assigned at the end is dependency injection, which the [section on blocks](#blocks) returns
to.

`dealloc` is the destructor, run when the object's last owner lets go. Under ARC, described
[below](#memory-arc), you don't release fields and don't call `[super dealloc]`; the compiler does
both. You write `dealloc` only for what ARC can't see, and a C handle is exactly that. This line is
the other end of `Box::from_raw` from
[the C boundary](boundary.md#step-2-hand-ownership-across-and-take-it-back): when the last reference
to the session goes, the Rust engine goes with it.

> [!TIP]
> **Remember.** The header shows the interface and hides the fields. `dealloc` exists here only to
> free what ARC doesn't manage, the Rust engine.

## Properties

*Goal: read property attributes and know what they generate.*

A
[**property**](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/EncapsulatingData/EncapsulatingData.html)
declares a pair of **accessors**, a getter and a setter; the compiler writes the methods and a field
named with a leading underscore, as Python's
[`@property`](https://docs.python.org/3/library/functions.html#property) and JavaScript's
[getters](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Functions/get) do by
hand. The attributes in parentheses say how:

[`native/macos/src/KRSession.h:94-104`](../../native/macos/src/KRSession.h#L94-L104)

```objc
/// Input mode (KR_EN or KR_RU) of a finish the session plans on its own; the controller keeps
/// it equal to its current mode. A key carries its mode in its event.
@property(nonatomic) uint32_t mode;
/// True after an unknown write outcome, a contract violation or a poisoned engine. Ordinary
/// keys then pass with zero client calls until `activate` (never again once poisoned). After a
/// verified write, the first unknown outcome in an activation stops only that word: the next
/// text key re-arms the session and starts from fresh client checks.
@property(nonatomic, readonly) BOOL stopped;
/// YES while the session holds local authority over a verified mark.
@property(nonatomic, readonly) BOOL owned;
@property(nonatomic, readonly) KRInputStatus inputStatus;
```

[`native/macos/src/KRSession.h:112-114`](../../native/macos/src/KRSession.h#L112-L114)

```objc
/// Replaceable for controlled tests; defaults to `IsSecureEventInputEnabled`. Checked at entry
/// and again after each client query that precedes a text read or a write.
@property(nonatomic, copy) BOOL (^secureInputEnabled)(void);
```

`nonatomic` drops the default guarantee that a getter racing a setter on another thread still sees a
whole value; everything here runs on the **main thread**, the one thread on which macOS delivers
every input event, so the guarantee would buy nothing. `readonly` publishes only the getter: the
class writes `_stopped` directly, and nobody outside can. `copy` makes the setter copy its argument,
which a block requires, since a block written in a function may live on the stack until it is copied
to the heap. `self.mode = KR_RU` and `[self setMode:KR_RU]` are the same message; the dot is
notation, not direct field access.

A property may also be computed. `owned` has no field behind it; the implementation supplies the
getter:

[`native/macos/src/KRSession.mm:93-99`](../../native/macos/src/KRSession.mm#L93-L99)

```obj-c++
- (BOOL)owned {
  return _owner != nil;
}

- (BOOL)boundaryRetained {
  return _owner && [_text hasSuffix:@" "];
}
```

When a class writes its own setter, it must say which field the property uses, with `@synthesize`;
the controller does that to pass a new Secure Input block on to the sessions it owns:

[`native/macos/src/KRController.mm:168`](../../native/macos/src/KRController.mm#L168)

```obj-c++
@synthesize secureInputEnabled = _secureInputEnabled;
```

[`native/macos/src/KRController.mm:197-201`](../../native/macos/src/KRController.mm#L197-L201)

```obj-c++
- (void)setSecureInputEnabled:(BOOL (^)(void))block {
  _secureInputEnabled = [block copy];
  _session.secureInputEnabled = _secureInputEnabled;
  _compatibility.secureInputEnabled = _secureInputEnabled;
}
```

The last line may run while `_compatibility` is `nil`, and that is fine: a message to `nil` does
nothing, and a method returning an object, a number or a pointer gives `nil` or zero (a structure
result is not always zeroed, so don't rely on it there). The language guarantees it, and **Cocoa**
code, written against Apple's Objective-C frameworks for macOS applications, relies on it
constantly, much as JavaScript code relies on
[optional chaining](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Operators/Optional_chaining),
`obj?.method()`, except that Objective-C does it for every message without being asked. It cuts both
ways: a wrong assumption that an object exists produces no crash and no error, only zeros, which is
one reason the adapter checks the objects an application hands it before using them, as
[below](#values-from-outside-are-checked).

The type of the block property needs a reading aid. `BOOL (^secureInputEnabled)(void)` declares a
block named `secureInputEnabled` that takes nothing and returns `BOOL`. It is built like the C
function pointer `BOOL (*f)(void)`, with `^` in place of `*`.

> [!TIP]
> **Remember.** A property is a getter and a setter, written by the compiler. A message to `nil` is
> legal and does nothing; an object or number result is `nil` or zero.

## Protocols and forward declarations

*Goal: read a protocol and a type that requires it.*

A
[**protocol**](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/WorkingwithProtocols/WorkingwithProtocols.html)
is a named list of methods that any class may adopt: a C++ class with only pure virtual functions, a
Python [`typing.Protocol`](https://docs.python.org/3/library/typing.html#typing.Protocol), a
TypeScript interface, a Rust trait. Both editing backends adopt this one, so the dictionary loader
can hand dictionaries to whichever is active without knowing its class:

[`native/macos/src/KRSession.h:8-14`](../../native/macos/src/KRSession.h#L8-L14)

```objc
/// A session that can take the runtime's published vocabulary snapshots into its engine. Both
/// backends implement it, so `KRRuntime` adopts into whichever session the active controller
/// routed to. Returns the `kr_adopt_vocabulary` status; STATE means "not now", and the runtime
/// retries after a later observation.
@protocol KRVocabularyAdopting <NSObject>
- (uint32_t)adoptVocabulary:(kr_vocabulary *)vocabulary;
@end
```

`id<KRVocabularyAdopting>` then means "any object that adopts it". The most important protocol in
the adapter is not Keyrill's own. `IMKTextInput`, from Apple's
[InputMethodKit](https://developer.apple.com/documentation/inputmethodkit) (**IMK**), is how an
input method talks to the application it types into: `markedRange`, `selectedRange`,
`stringFromRange:actualRange:`, `setMarkedText:selectionRange:replacementRange:` and
`insertText:replacementRange:` are messages of that protocol. Each one is
[inter-process communication](https://en.wikipedia.org/wiki/Inter-process_communication) (**IPC**):
a request to another running program, the application, which answers when it is ready.

A header that only names a class or protocol, without calling it, declares it in advance, as C++
does with `class X;`:

[`native/macos/src/KRRuntime.h:9-10`](../../native/macos/src/KRRuntime.h#L9-L10)

```objc
@class KRSession;
@protocol KRVocabularyAdopting;
```

> [!TIP]
> **Remember.** A protocol is an interface; `id<P>` is any object that adopts it. Every message to
> the application is IPC to another process.

## Nullability and enums

*Goal: read the annotations that open Keyrill's headers.*

Headers here open with `NS_ASSUME_NONNULL_BEGIN`, which makes every object pointer in the region
non-null unless it is marked `nullable`. The compiler checks the annotations where it can, and they
document the contract where it can't
([nullability](https://developer.apple.com/documentation/swift/designating-nullability-in-objective-c-apis)).

`NS_ENUM` declares a C enum with a fixed underlying type, which makes its size part of the
declaration rather than a choice the compiler makes:

[`native/macos/src/KRSession.h:16-30`](../../native/macos/src/KRSession.h#L16-L30)

```objc
/// Most recent input-boundary observation, shown in the input menu as the reason the last word
/// was left alone. It contains no text, offsets or client identity.
typedef NS_ENUM(uint8_t, KRInputStatus) {
  KRInputNotObserved,
  KRInputReady,
  /// The client's bundle identifier is not in the composition set.
  KRInputExcluded,
  KRInputSecure,
  /// The selection was not a collapsed caret, the client already had a mark, or the unit before
  /// the caret came back with another length or range.
  KRInputContextUnavailable,
  /// The UTF-16 unit before the caret is not whitespace: the word continues other text.
  KRInputNotAtBoundary,
  KRInputMarkMismatch,
  KRInputCaretMismatch,
```

The same privacy rule as on the Rust side shows in the doc comment: a status says *why* a word was
left alone and carries nothing of what was typed.

> [!TIP]
> **Remember.** Pointers are non-null unless marked `nullable`. A status names a reason, never the
> text.

## Values from outside are checked

*Goal: see how the adapter treats every answer from the application as untrusted.*

The declared type of an application's answer is a promise made by a header, and the application is
someone else's code in someone else's process. The adapter checks the answer's real type before it
uses it, and it is ready for the call itself to fail:

[`native/macos/src/KRSession.mm:340-352`](../../native/macos/src/KRSession.mm#L340-L352)

```obj-c++
- (BOOL)clientAllowed:(id<IMKTextInput>)client generation:(uint64_t)generation {
  @try {
    NSString *bundle = [client bundleIdentifier];
    BOOL allowed = [bundle isKindOfClass:NSString.class] && bundle.length != 0 &&
                   [_allowedClients containsObject:bundle];
    if (generation == _generation)
      _inputStatus = allowed ? KRInputReady : KRInputExcluded;
    return generation == _generation && allowed && ![self secureAfterQuery];
  } @catch (NSException *) {
    _inputStatus = KRInputClientException;
    return NO;
  }
}
```

`isKindOfClass:` asks the object for its class at run time, as Python's `isinstance` and
JavaScript's `instanceof` do. `NSString.class` is the class object itself, sent the `class` message
through dot syntax. If the application returned `nil`, the first test is a message to `nil` and
yields `NO`, so the same expression handles a missing answer and an answer of the wrong kind. Why
the method compares `generation` with `_generation` after the call is the subject of
[Reentrancy and tests](reentrancy-and-tests.md).

> [!TIP]
> **Remember.** Check the class of anything an application returns, and expect any call to it to
> raise.

## Literals and collections

*Goal: read Foundation's collection literals and their mutable variants.*

Foundation, Apple's base library, has collections that hold objects only, and the
[literal syntax](https://clang.llvm.org/docs/ObjectiveCLiterals.html) makes them short to write. A
per-application setting is updated like this:

[`native/macos/src/KRController.mm:522-527`](../../native/macos/src/KRController.mm#L522-L527)

```obj-c++
  NSMutableDictionary *tiers = [runtime.tierOverrides mutableCopy];
  if (name)
    tiers[bundle] = name;
  else
    [tiers removeObjectForKey:bundle];
  NSDictionary *changes = @{@"ApplicationTiers" : tiers.count ? [tiers copy] : NSNull.null};
```

Each collection comes in two classes, mutable and immutable, and `mutableCopy` and `copy` convert
between them; Python's `list` and `tuple` are the nearest pair. Subscripting,
`tiers[bundle] = name`, is notation for the message `setObject:forKey:`. A dictionary can't store
`nil`, so "remove this key" travels as the singleton `NSNull.null`, and the code that applies the
change tests for it. Iterating a collection uses fast enumeration, `for (NSString *key in changes)`,
the counterpart of Python's `for key in changes:`.

> [!TIP]
> **Remember.** `@[]` and `@{}` make immutable collections; `mutableCopy` makes a version you can
> change. `nil` can't go into a collection, so `NSNull` stands for it.

## Memory: ARC

*Goal: know what ARC does for you, and the one thing it doesn't.*

Every object carries a **retain count**, the number of owners it has; it is freed when the count
reaches zero. Under
[Automatic Reference Counting](https://clang.llvm.org/docs/AutomaticReferenceCounting.html)
(**ARC**), the compiler inserts the calls that raise and lower the count, called *retain* and
*release*, so you never write them. A pointer is `__strong` by default and keeps its object alive; a
`__weak` pointer doesn't, and becomes `nil` when the object goes away, like Python's
[`weakref`](https://docs.python.org/3/library/weakref.html) and JavaScript's
[`WeakRef`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WeakRef).

CPython also counts references, and C++'s `std::shared_ptr` works the same way. The difference from
CPython and JavaScript is that ARC has no garbage collector behind it: two objects that hold strong
pointers to each other keep each other alive forever, a **retain cycle**. The controller keeps a
weak pointer to the most recently activated controller, so this global never keeps alive a
controller that InputMethodKit has discarded:

[`native/macos/src/KRController.mm:10-13`](../../native/macos/src/KRController.mm#L10-L13)

```obj-c++
// Process-wide, main thread only
static __weak KeyrillAlphaController *activeController;
static KRRuntime *runtime;
static BOOL (^preferencesUpdater)(NSDictionary *);
```

ARC has no borrow checker, and one consequence matters here. A method is a function with a hidden
`self` parameter, and ARC does not retain `self` for the duration of a call: the
[ARC specification](https://clang.llvm.org/docs/AutomaticReferenceCounting.html#self) treats it as
kept alive by the caller, and calls deallocating an object during a message to it "undefined
behavior, or at least dangerous". In most programs nothing can release an object while one of its
methods runs. In an input method something can: a call to the application can let other events run,
including one that makes InputMethodKit drop its last reference to this session. Hence a line that
looks peculiar until you know why it is there:

[`native/macos/src/KRSession.mm:191-195`](../../native/macos/src/KRSession.mm#L191-L195)

```obj-c++
- (BOOL)handleEvent:(const kr_event *)input client:(id<IMKTextInput>)client {
  __attribute__((objc_precise_lifetime)) KRSession *lifetime = self;
  (void)lifetime;
  if (!NSThread.isMainThread || !input)
    return NO;
```

The local strong reference retains `self` for the call. Without the attribute, ARC may release a
local at its last use, and a local that is never read has no use at all.
[`objc_precise_lifetime`](https://clang.llvm.org/docs/AutomaticReferenceCounting.html#precise-lifetime-semantics)
tells ARC to keep it until the end of its scope, and `(void)lifetime;` silences the warning about an
unused variable. The other session and controller entry points that can reach the application start
the same way.

> [!TIP]
> **Remember.** ARC counts references for you but collects no cycles. It doesn't keep `self` alive
> during a call, so methods that call the application retain `self` themselves.

## Blocks

*Goal: read a block, its captures, and the weak-strong pattern.*

A
[**block**](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/WorkingwithBlocks/WorkingwithBlocks.html)
is Objective-C's closure: a literal `^{ ... }` that **captures** the variables it uses and can be
stored, copied and called later
([language specification](https://clang.llvm.org/docs/BlockLanguageSpec.html)). One difference from
Python and JavaScript closures matters: a block captures the *value* a local variable had when the
block was created, and can't assign to it, unless the variable is declared `__block`; a Python or
JavaScript closure always sees the variable itself
([closures in JavaScript](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Guide/Closures)).

The session's `secureInputEnabled` property, set in the initializer above, holds a block. Production
code leaves the default, which asks macOS whether
[Secure Input](https://developer.apple.com/library/archive/technotes/tn2150/_index.html), the mode
in which keys go only to a password field, is on; a test replaces the block and turns Secure Input
on at a chosen moment. This is the same dependency injection the Rust side does with a closure
parameter in [`set_policy`](rust.md#a-closure-as-a-parameter).

A block keeps the objects it captures alive, which makes retain cycles easy: an object that stores a
block that captures the object keeps itself alive. Work put off until later is where this matters
most, and the adapter answers it with the *weak-strong* pattern:

[`native/macos/src/KRController.mm:304-310`](../../native/macos/src/KRController.mm#L304-L310)

```obj-c++
  // Permissions granted after launch take effect here; tap creation can't run in a callback
  __weak KeyrillAlphaController *weakController = self;
  dispatch_async(dispatch_get_main_queue(), ^{
    KeyrillAlphaController *controller = weakController;
    if (controller && activeController == controller && usesCompatibility(controller->_tier))
      controller->_startGuard();
  });
```

The block captures only a weak pointer, so a queued block doesn't keep a closed controller alive.
When it runs, it takes a strong local copy, which is either `nil` (the controller is gone, and the
block does nothing) or a controller that now stays alive until the block ends. Inside the block,
`controller->_tier` reads a field through a pointer, exactly as in C; that is how a block reaches
the private fields of an object it holds in a local variable.

The opposite choice is also made on purpose. Work the dictionary loader puts on its own queue
captures `self` strongly, and writes `self->` explicitly, so the capture is visible on the page:

[`native/macos/src/KRRuntime.mm:296-303`](../../native/macos/src/KRRuntime.mm#L296-L303)

```obj-c++
  dispatch_async(_queue, ^{
    if (self->_stopped)
      return;
    NSString *home = self->_homeResolver();
    self->_mainExecutor(^{
      [self configureDefaultsInHome:home];
    });
  });
```

A one-shot block that keeps the loader alive until it has run is harmless: the cycle ends when the
queue releases the block. A repeating timer is different, because the timer lives as long as the
loader and would keep it alive forever, so its handler captures `self` weakly:

[`native/macos/src/KRRuntime.mm:331-338`](../../native/macos/src/KRRuntime.mm#L331-L338)

```obj-c++
  _timer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, _queue);
  __weak KRRuntime *weakSelf = self;
  dispatch_source_set_event_handler(_timer, ^{
    [weakSelf observe];
  });
  dispatch_source_set_timer(_timer, dispatch_time(DISPATCH_TIME_NOW, (int64_t)_interval), _interval,
                            _interval / 2);
  dispatch_resume(_timer);
```

`[weakSelf observe]` is a message to `nil` once the loader is gone, which does nothing, so no
explicit check is needed.

> [!TIP]
> **Remember.** A block captures values and keeps captured objects alive. Code that runs later
> captures `self` weakly and takes a strong copy when it starts.

## Queues instead of threads

*Goal: read `dispatch_async` and know which queue runs what.*

The adapter never creates a thread. It uses
[Grand Central Dispatch](https://developer.apple.com/documentation/dispatch) (**GCD**), Apple's
library of work queues: `dispatch_async(queue, block)` puts a block on a **queue** and returns at
once. The **main queue** runs its blocks on the main thread, where every input callback arrives, one
after another, as JavaScript's event loop runs callbacks
([execution model](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Execution_model)).
A **serial queue** runs its blocks one at a time, in order, on some thread from a shared pool, much
as a Python `ThreadPoolExecutor(max_workers=1)` would. The process creates one serial queue for
dictionary files, at low priority:

[`native/macos/src/main.mm:26-38`](../../native/macos/src/main.mm#L26-L38)

```obj-c++
static KRRuntime *runtimeForPreferences(NSDictionary *preferences) {
  return [[KRRuntime alloc] initWithPreferences:preferences
      homeResolver:^NSString * {
        return accountHome();
      }
      workQueue:dispatch_queue_create("org.keyrill.vocabulary",
                                      dispatch_queue_attr_make_with_qos_class(DISPATCH_QUEUE_SERIAL,
                                                                              QOS_CLASS_UTILITY, 0))
      mainExecutor:^(dispatch_block_t block) {
        dispatch_async(dispatch_get_main_queue(), block);
      }
      observationInterval:2 * NSEC_PER_SEC];
}
```

This is the "serialized acquisition queue" the header's dictionary contract asks for
([the C boundary](boundary.md#threads-and-locks)): `kr_vocabulary_observe` runs only on this queue,
and adoption only on the main thread. `mainExecutor` is the step back: "run this on the main queue",
as Python's
[`call_soon_threadsafe`](https://docs.python.org/3/library/asyncio-eventloop.html#asyncio.loop.call_soon_threadsafe)
hands work from a worker thread to the event loop. Even that is injected:
`native/macos/tests/runtime_tests.mm` passes an executor that queues blocks until the test drains
them, which turns the interleaving of the two queues into a fixed schedule instead of a race.

> [!TIP]
> **Remember.** Work moves between threads as blocks on queues. Input happens on the main queue,
> file reading on one serial queue.

## Exceptions

*Goal: know why an input method must catch exceptions that Cocoa programs usually don't.*

In Python and JavaScript, exceptions report ordinary failures. In Cocoa,
[exceptions](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Exceptions/Exceptions.html)
signal programmer errors and are rarely caught; expected failures return `nil`, `NO` or an `NSError`
object. An application, however, is foreign code that may raise anything from inside a protocol
method, and an exception that escapes an input method's callback can take the input method down. So
the adapter is compiled with `-fobjc-exceptions` and wraps every call to the application:

[`native/macos/build.sh:31-33`](../../native/macos/build.sh#L31-L33)

```sh
  "$cxx" -std=c++20 -fobjc-arc -fobjc-exceptions -isysroot "$sdk" \
    -mmacosx-version-min=13.0 -g -Wall -Wextra -Wpedantic -Werror \
    -Wno-gnu-zero-variadic-macro-arguments -DKR_BUNDLE_IDENTIFIER="\"$bundle_id\"" \
```

`@catch` appears in `clientAllowed:` above. `@finally` runs however the block ends, by `return` or
by exception, as `finally` does in Python and JavaScript; that is how the session makes sure its
in-flight flag clears:

[`native/macos/src/KRSession.mm:221-226`](../../native/macos/src/KRSession.mm#L221-L226)

```obj-c++
  _busy = YES;
  @try {
    return [self runEvent:*input client:client generation:_generation];
  } @finally {
    _busy = NO;
  }
```

Whether the objects held by local variables are released when an exception passes through a function
depends on a compiler setting, and the default differs between Objective-C and Objective-C++; the
[next chapter](objective-cpp.md#raii-across-objective-c-exceptions) explains why that matters for
this file.

> [!TIP]
> **Remember.** Cocoa reserves exceptions for bugs, but an application's bug must not crash the
> input method, so every call to the application is wrapped.

## Selectors and target-action

*Goal: read `@selector` and know what connects a menu item to a method.*

A selector can be a value: `@selector(name:)` has the type `SEL`, like a method name stored in a
string for Python's `getattr(obj, name)`. Menus in **AppKit**, the part of Cocoa for windows, menus
and events, use selectors for
[target-action](https://developer.apple.com/library/archive/documentation/General/Conceptual/Devpedia-CocoaApp/TargetAction.html):
a menu item stores a selector and, when chosen, sends that message to its target. The input menu's
"Correction in This App" items each name a one-line method:

[`native/macos/src/KRController.mm:542-545`](../../native/macos/src/KRController.mm#L542-L545)

```obj-c++
- (void)useDefaultTier:(id)sender {
  (void)sender;
  [self overrideTier:nil];
}
```

The colon in `useDefaultTier:` is part of the name: an action method takes the sender as its one
argument. Since dispatch is by name, nothing but the selector connects the menu item to the method,
and a typo in `@selector(...)` compiles without a word under `-Wall -Wextra`; clang warns about a
selector no visible class declares only with `-Wundeclared-selector`, which this build doesn't
enable.

> [!TIP]
> **Remember.** A selector is a method name as a value, and only the name connects a menu item to
> its method.

## Where the process starts

*Goal: read `main` of a program that runs until the user logs out.*

An input method is an ordinary application without a document window; Keyrill opens a window only
for its settings, on request. `main` sets up AppKit, registers the controller class, creates an
[`IMKServer`](https://developer.apple.com/documentation/inputmethodkit/imkserver) and runs the **run
loop**, the main thread's event loop, forever:

[`native/macos/src/main.mm:100-128`](../../native/macos/src/main.mm#L100-L128)

```obj-c++
int main() {
  @autoreleasepool {
    [NSApplication sharedApplication];
    // Registers the controller class before IMK resolves it by name from Info.plist
    (void)KeyrillAlphaController.class;
    __attribute__((objc_precise_lifetime)) KRApplicationDelegate *delegate =
        [KRApplicationDelegate new];
    NSApp.delegate = delegate;
    NSBundle *bundle = NSBundle.mainBundle;
    NSString *connection = [bundle objectForInfoDictionaryKey:@"InputMethodConnectionName"];
    if (!connection || ![bundle.bundleIdentifier isEqualToString:@KR_BUNDLE_IDENTIFIER])
      return 2;
    KRRuntime *runtime = runtimeForPreferences(
        [NSUserDefaults.standardUserDefaults persistentDomainForName:bundle.bundleIdentifier]);
    if (!runtime || ![KeyrillAlphaController setRuntime:runtime])
      return 2;
    KeyrillAlphaController.preferencesUpdater = ^BOOL(NSDictionary *changes) {
      return applyPreferences(changes);
    };
    [runtime start];
    // The server must outlive the run loop
    __attribute__((objc_precise_lifetime)) IMKServer *server =
        [[IMKServer alloc] initWithName:connection bundleIdentifier:bundle.bundleIdentifier];
    if (!server)
      return 2;
    [NSApp run];
  }
  return 0;
}
```

[`@autoreleasepool`](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/MemoryMgmt/Articles/mmAutoreleasePools.html)
is a scope for objects whose release has been postponed, a leftover of manual reference counting
that ARC still uses at the edges of Apple's APIs. InputMethodKit finds the controller class by its
name in `Info.plist` and then creates one
[`IMKInputController`](https://developer.apple.com/documentation/inputmethodkit/imkinputcontroller)
per client input session; nothing in the program refers to the class directly, so `main` sends it
the `class` message to make sure the runtime has registered it before InputMethodKit looks.

The two `objc_precise_lifetime` locals are the same device as in `handleEvent:`. `NSApp` doesn't
retain its delegate, and nothing else refers to the server, so each local is the only strong
reference to its object; neither is read again, and without the attribute ARC could release both
before `[NSApp run]`, which returns only when the process ends.

> [!TIP]
> **Remember.** `main` builds the server and enters the run loop for good. Objects only `main` holds
> are kept alive with `objc_precise_lifetime`.
