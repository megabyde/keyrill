# Reentrancy and tests

The previous chapters taught the languages. This one is about the hardest idea in the adapter, which
no language feature solves: a call that waits for an answer and lets the world change underneath it.
Then it follows one word through all the code shown so far, and ends with the two testing techniques
that keep this design honest.

## A call that runs your code

*Goal: explain how a single-threaded program can be interrupted by itself.*

A single-threaded program feels safe: nothing else runs while your function runs. In Keyrill's
adapter all input handling runs on the main thread, with only dictionary loading on a background
queue, and that path is still not safe in this sense.

A message such as `[client markedRange]` is IPC to the application, and while the main thread waits
for the answer, it may run a **nested run loop**: the event loop is entered again from inside the
waiting call, and it delivers other events. A deactivation, a mode change or another key can arrive
*inside* the call, and its callback runs on your thread, on your object, while your function is
still waiting on the stack. This is
[**reentrancy**](https://en.wikipedia.org/wiki/Reentrancy_%28computing%29): your code entered again
before its first run finished.

JavaScript and Python programmers know the shape from asynchronous code. Across an
[`await`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Operators/await), other
callbacks run and may change any state you read before it; the difference here is that the call
looks synchronous, so nothing in the code marks the place. No lock helps: a lock held by the outer
call would block the inner call on the same thread forever. Rust's borrow rules don't help either,
because the call to the application happens outside Rust altogether.

The session's header documents the complete schedule for one correction, with the place of every
check. Read it slowly; each line marked "recheck generation" guards one point where the application
could have run the adapter's code again:

[`native/macos/src/KRSession.h:48-75`](../../native/macos/src/KRSession.h#L48-L75)

```objc
/// Main thread only. A client call is synchronous IPC and can spin a nested run loop, so any of
/// them can reenter the controller with a deactivation, a mode change or another key. A
/// reentrant callback bumps the generation, resets Rust and drops local authority; the
/// interrupted operation sees the new generation at its next recheck and refuses before the
/// write or reports an unknown outcome after it, and never writes again. Automatic correction
/// of "ghbdtn" at Space, with the mark at [s, s+6) and the caret at s+6:
///
///   kr_plan_event          MARK, expected "ghbdtn", mark "привет " a proposal only
///   markedRange            == [s, s+6)                             recheck generation
///   selectedRange          == {s+6, 0}                             recheck generation
///   Secure Input           off
///   stringFromRange        == "ghbdtn", actual range == [s, s+6)   recheck generation
///   Secure Input           off
///   -- local authority dropped: from here the outcome is verified or unknown --
///   setMarkedText          "привет ", selection at its end         the plan's one write
///   markedRange            == [s, s+7)                             recheck generation
///   selectedRange          == {s+7, 0}                             recheck generation
///   Secure Input           off
///   stringFromRange        == "привет ", actual range == [s, s+7)  readback
///   kr_commit              APPLIED_VERIFIED: Rust advances, the new mark is owned
///
/// A failed check before the write acknowledges REFUSED_BEFORE_WRITE: nothing was written and
/// the key passes. A failed check after it acknowledges ATTEMPTED_UNKNOWN: correction stops,
/// and a key the write carried is consumed, never replayed. When that check failed on the
/// client's answer rather than on reentrancy, an exception or Secure Input, the session commits
/// the client's own exact readback of its marked range once, if both are still available, so
/// that the next key can't replace it; that is the one write allowed after an unknown outcome,
/// and it repairs nothing.
```

> [!TIP]
> **Remember.** A call to the application can run your own callbacks before it returns. After such a
> call, everything you knew before it may be stale.

## Generation and busy

*Goal: read the two devices that detect a reentrant call.*

The defense has two parts. The first is a counter, the **generation**. Every event that invalidates
what the session knows adds one to `_generation`, drops the session's authority over its text and
resets the Rust engine:

[`native/macos/src/KRSession.mm:120-132`](../../native/macos/src/KRSession.mm#L120-L132)

```obj-c++
- (void)invalidate {
  if (!NSThread.isMainThread)
    return;
  ++_generation;
  _owner = nil;
  _text = nil;
  _ownerMode = 0;
  _contextLocation = NSNotFound;
  _automaticSwitchMode = 0;
  _finishVerified = NO;
  if (_engine)
    [self noteStatus:kr_reset(_engine)];
}
```

The operation in progress captured the generation when it started and carries it as an argument;
after **every** call to the application it compares the two. A difference means some callback ran
inside the call, and everything the operation knew before it is void. Front-end code uses the same
pattern against stale network responses: number each request, and ignore a response whose number is
no longer the latest.

[`native/macos/src/KRSession.mm:401-408`](../../native/macos/src/KRSession.mm#L401-L408)

```obj-c++
  NSRange expected = NSMakeRange(start, text.length);
  NSRange marked = [client markedRange];
  if (generation != _generation)
    return NO;
  if (!NSEqualRanges(marked, expected)) {
    _inputStatus = KRInputMarkMismatch;
    return NO;
  }
```

The order of the two tests matters. The generation comes first: after a reentrant callback, `marked`
describes a document the session no longer has authority over, and comparing it with the expected
range would answer a question that has expired.

The second part is a flag that lets the reentrant callback recognize itself. `_busy` is set while a
plan is in flight, and a key that arrives while it is set came through a nested run loop:

[`native/macos/src/KRSession.mm:199-205`](../../native/macos/src/KRSession.mm#L199-L205)

```obj-c++
  // A key arriving while a plan is in flight came through a nested run loop inside a client
  // call. Invalidating here is what makes the outer operation's next recheck fail
  if (_busy) {
    _inputStatus = KRInputInterrupted;
    [self invalidate];
    return reserved;
  }
```

The nested call doesn't try to finish the outer one's work, or to do its own. It invalidates and
returns, and the outer operation, when the call to the application returns to it, finds a new
generation and stops. Work in flight is cancelled, never merged.

The `_generation` comparison is the same device as the slot generation in the
[dictionary handle](boundary.md#threads-and-locks). There it guards against another thread; here it
guards against the same thread arriving a second time.

> [!TIP]
> **Remember.** Capture the generation before a call, compare after it, and stop on any difference.
> A callback that finds `_busy` set invalidates and returns.

## Two kinds of failure

*Goal: tell "refused before the write" from "unknown after it", and see where the code draws the
line.*

A failed check means different things before and after the write. Before it, nothing has happened to
the document: the outcome is "refused before write", the engine is told so, and the key goes to the
application as if Keyrill weren't there. After the write begins, the honest answer can only be
"verified" or "unknown". The application may have applied the text, part of it, or nothing, and a
failed **readback**, reading the text back to compare it with what was written, doesn't say which.

The adapter draws the line in the code by dropping its authority *before* the write, so that no path
after it can reuse that authority:

[`native/macos/src/KRSession.mm:495-524`](../../native/macos/src/KRSession.mm#L495-L524)

```obj-c++
  // From here on the outcome is either verified or unknown; local authority is dropped first so
  // that no path can reuse it after the write
  _owner = nil;
  _text = nil;
  @try {
    if (plan.action == KR_MARK)
      [client setMarkedText:[[NSAttributedString alloc] initWithString:mark]
             selectionRange:NSMakeRange(plan.mark_len, 0)
           replacementRange:kCurrentSelection];
    else
      [client insertText:mark
          replacementRange:unchanged ? NSMakeRange(start, mark.length) : kCurrentSelection];
    if (generation != _generation)
      return KR_ATTEMPTED_UNKNOWN;

    if (plan.action == KR_MARK && mark.length) {
      if (![self verifyMark:mark
                         at:start
                      caret:start + plan.mark_len
                   readText:YES
                     client:client
                 generation:generation]) {
        _checkFailed = generation == _generation && _inputStatus != KRInputSecure;
        return KR_ATTEMPTED_UNKNOWN;
      }
      _owner = client;
      _start = start;
      _text = mark;
      return KR_APPLIED_VERIFIED;
    }
```

Authority comes back in exactly one place: after the readback matched, `_owner`, `_start` and
`_text` are set to what was verified. Every other path out of this block leaves them `nil`. The
method's return value plays no part; `setMarkedText:` returns nothing, and if it returned `YES`,
that would still be a claim by the application rather than evidence about the document. Only reading
the document back is evidence.

> [!TIP]
> **Remember.** Before the write, a failure is a refusal; after it, a failure is unknown. Only a
> readback that matches restores authority.

## Plan, apply, acknowledge

*Goal: read the adapter's main loop for one key.*

The core of the adapter's loop is short enough to learn by heart. It runs once per key, twice when a
finished word must be acknowledged before the same key starts the next one:

[`native/macos/src/KRSession.mm:257-288`](../../native/macos/src/KRSession.mm#L257-L288)

```obj-c++
    kr_plan plan;
    uint32_t status = kr_plan_event(_engine, &event, &plan);
    if (status != KR_OK) {
      _inputStatus = KRInputEngineUnavailable;
      [self invalidate];
      [self noteStatus:status];
      return NO;
    }

    uint32_t outcome = KR_APPLIED_VERIFIED;
    if (plan.action == KR_MARK || plan.action == KR_FINISH)
      outcome = [self apply:plan client:client generation:generation];
    else if (plan.action != KR_PASS && plan.action != KR_UNAVAILABLE)
      outcome = KR_REFUSED_BEFORE_WRITE;

    status = kr_commit(_engine, plan.plan_id, outcome);
    if (outcome == KR_ATTEMPTED_UNKNOWN) {
      // Only a check after the write that failed on the client's answer, with the generation
      // unchanged, earns the one readback commit; after reentrancy, a client exception or Secure
      // Input the client gets no further call
      BOOL commit = _checkFailed && generation == _generation;
      [self invalidate];
      _stopped = YES;
      ++_unknownOutcomes;
      _rearmable = _verifiedWrite && _unknownOutcomes < 2;
      [self noteStatus:status];
      if (commit)
        [self commitReadbackIn:client generation:_generation];
      // A write that carried this key may already be in the document: consume it, never replay
      // it. A FINISH that passes or replans carried only the previous mark, so the key passes.
      return plan.disposition == KR_KEY_CONSUMED;
    }
```

`kr_plan plan;` is a 536-byte struct on the stack, written by Rust; `&plan` passes its address, and
`apply:` receives it by reference. An action the header doesn't define is refused rather than
guessed at. An unknown outcome stops correction: the session invalidates, marks itself stopped and
doesn't retry, because a retry would be a second write on top of an effect nobody can see. The one
exception is spelled out in the comment and in the header above: if the readback failed on the
application's own answer, the session commits exactly what the application reports as its marked
text, once, so that the next key the user types can't replace it.

> [!TIP]
> **Remember.** Plan, apply, acknowledge, once per plan: one plan per key, two when a finished word
> must be acknowledged first. An unknown outcome stops correction and is never retried.

## One word, end to end

*Goal: follow `ghbdtn` and Space through every layer.*

Here are all the pieces in order, for `ghbdtn` and Space typed in TextEdit in English mode.

1. **Controller.** `-[KeyrillAlphaController handleEvent:client:]` checks Secure Input, the runtime
   and the application's tier, opens a `CallbackScope`, and builds a `kr_event` with
   `KREventForKey`.
1. **Session entry.** `-[KRSession handleEvent:client:]` retains itself, checks `_busy`, sets it,
   and for a word not yet started asks the application what precedes the caret.
1. **ABI.** `kr_plan_event` decodes the C struct into an `Event` inside `with_engine`, under
   `catch_unwind`.
1. **Decision.** `propose_using` runs `transition` on the confirmed state; at Space, inference may
   convert the word. The proposed state waits in `pending`.
1. **Check and write.** `apply:` verifies the mark, the caret and the text, comparing the generation
   after each call, drops authority, makes the one write and reads it back.
1. **Acknowledge.** `kr_commit` reaches `acknowledge`, `take()` moves the proposal out, and the
   proposed state becomes the confirmed one.

| Key | Action | Expected in mark | Written to mark | Key afterwards |
| -- | -- | -- | -- | -- |
| `g` | `MARK` | empty | `g` | consumed |
| `h` ... `n` | `MARK` | `g` ... `ghbdt` | `gh` ... `ghbdtn` | consumed |
| Space | `MARK` | `ghbdtn` | `привет ` | consumed |
| next letter | `FINISH`, then `MARK` | `привет ` | commits, then starts the new word | replanned |
| Control-Option-Z before that | `MARK` | `привет ` | `ghbdtn ` | consumed |

The word stays marked, underlined as text an input method is still composing, after Space, and that
is the whole trick behind undo: as long as the text is a composition Keyrill owns, replacing it is
an ordinary edit of its own mark. Keyrill never rewrites committed text on this path
([ADR 005](../adr/005-no-committed-text-rewrite.md)).

With layout switching on, the default, a converted word also makes the controller request the other
input mode, Russian here. `-[KeyrillAlphaController requestInputMode:client:]` sends the request
from a block queued with `dispatch_async`, after the key has returned to the application; the block
holds the controller weakly, as in the [Objective-C chapter](objective-c.md#blocks), and gives up if
the input-mode request generation (`_selectionGeneration`) changed in the meantime.

> [!TIP]
> **Remember.** The word stays Keyrill's own mark after Space, which is why undo is an ordinary edit
> and committed text is never touched.

## Property tests in Rust

*Goal: read a property test and know what makes it reproducible.*

An example test checks the cases someone thought of. A **property test** states a law that must hold
for every input and checks it against many generated inputs; when one fails, the tool **shrinks** it
to a small failing input before reporting. Python has
[Hypothesis](https://hypothesis.readthedocs.io/) for this, JavaScript has
[fast-check](https://fast-check.dev/), and Rust has
[proptest](https://proptest-rs.github.io/proptest/), all descended from Haskell's
[QuickCheck](https://en.wikipedia.org/wiki/QuickCheck). This test states that a plan id never comes
back, whatever happens:

[`crates/keyrill-core/tests/contracts.rs:199-232`](../../crates/keyrill-core/tests/contracts.rs#L199-L232)

```rust
    // Ids grow strictly across resets and invalidation, so a late acknowledgement of an expired
    // plan can never match a later one; plans stay within one word and its Space. Codes divisible
    // by 3 reset before acknowledging, code 7 reports Unknown, and a failed proposal resets
    #[test]
    fn arbitrary_event_schedules_never_resurrect_a_reset_token(events in proptest::collection::vec(0u8..12, 0..=256)) {
        let mut session = Session::default();
        session.set_fixtures(true).unwrap();
        let mut previous = 0;
        for code in events {
            let event = match code {
                0 => Event::Manual, 1 => Event::Reverse, 2 => Event::Backspace,
                3 => Event::Escape, 4 => Event::Finish, 5 => Event::Text(vec![32]),
                6 => Event::Text(vec![0xd800]), 7 => Event::Text(vec![0x301]),
                _ => Event::Text(vec![97]),
            };
            if let Ok(plan) = session.propose(&event, Mode::En, Context::Boundary) {
                prop_assert!(plan.id > previous);
                prop_assert!(plan.text.len() <= MAX_WORD + 1);
                prop_assert!(plan.expected.len() <= MAX_WORD + 1);
                if plan.action == Action::Mark {
                    prop_assert!(plan.text.iter().all(|&u| mapping::is_letter(u) || u == 32 || b"[];',./`\\{}:\"<>~|".contains(&(u as u8))), "owned units remain in the defined map");
                }
                previous = plan.id;
                if code % 3 == 0 {
                    session.reset();
                    prop_assert!(session.acknowledge(plan.id, Outcome::Verified).is_err());
                } else {
                    session.acknowledge(plan.id, if code == 7 { Outcome::Unknown } else { Outcome::Verified }).unwrap();
                }
            } else {
                session.reset();
            }
        }
    }
```

The generator, `vec(0u8..12, 0..=256)`, produces up to 256 codes; the `match` turns them into
events, with letters weighted heavily, since four codes of twelve type `a`. The schedule mixes in
the hostile cases on purpose: a lone surrogate (`0xd800`, half of a two-unit character with its
other half missing), a combining accent (`0x301`), resets in the middle of a plan, unknown outcomes.
The configuration a few lines above the test fixes the random **seed**, the starting value of the
random generator, so every run tries the same 512 schedules and a failure reproduces on any machine.

> [!TIP]
> **Remember.** A property test checks a law over generated inputs. A fixed seed makes it repeat
> exactly.

## A hostile fake client

*Goal: read a test that injects a fault at one exact step of a protocol.*

The adapter is tested against `FakeClient`, a class in `native/macos/tests/session_tests.mm` that
implements `IMKTextInput` over an `NSMutableString` and misbehaves on demand. It is a **fake**, a
working stand-in written by hand, rather than a mock that only records calls, such as Python's
`unittest.mock`. Its documentation is the best description of what such a test can and can't prove:

[`native/macos/tests/session_tests.mm:43-59`](../../native/macos/tests/session_tests.mm#L43-L59)

```obj-c++
/// A scripted IMKTextInput client: one document string, a selection and a marked range, all in
/// UTF-16 units. setMarkedText ignores its replacement range and writes over the mark, or over
/// the selection when there is none, then places the caret by the mark-relative selection.
/// insertText writes to the same place for an NSNotFound range, or to exactly the explicit range
/// it is given, and puts the caret after the text.
/// stringFromRange clamps the range to the document and reports the clamped actual range.
///
/// A test plays the hostile host through `hook`, which runs at the entry of each call Keyrill
/// makes: it can move the caret, plant a foreign mark, switch Secure Input on, reenter the
/// session or throw, at exactly the call where a real host could. The flags below script what a
/// host does with a write. `calls` and `writes` let a test assert zero client contact or zero
/// writes after a refusal.
///
/// The client does nothing on its own: no focus, layout, undo, attributes or IMK reentrancy, and
/// it never moves the caret or commits the mark unless a test tells it to. That keeps every
/// schedule deterministic. A pass shows that the session refuses or verifies the scripted
/// schedule; it shows nothing about what a real application does.
```

The hook is a block property, called at the entry of every client method and once more after each
write has taken effect:

[`native/macos/tests/session_tests.mm:138-157`](../../native/macos/tests/session_tests.mm#L138-L157)

```obj-c++
- (void)setMarkedText:(id)string
       selectionRange:(NSRange)selectionRange
     replacementRange:(NSRange)r {
  (void)r;
  // A write counts as attempted from its entry, before any effect is visible
  ++_writes;
  ++_markWrites;
  NSString *text = [string isKindOfClass:NSAttributedString.class] ? [string string] : string;
  if (!text.length)
    ++_emptyMarks;
  [self enter:@"setMarkedText"];
  if (!_ignoreWrites) {
    NSRange target = [self target];
    [_document replaceCharactersInRange:target withString:text];
    _marked = text.length ? NSMakeRange(target.location, text.length) : NSMakeRange(NSNotFound, 0);
    _selection =
        NSMakeRange(_misplaceCaretOnMark ? 0 : target.location + selectionRange.location, 0);
  }
  [self enter:@"setMarkedText+after"];
}
```

This is [fault injection](https://en.wikipedia.org/wiki/Fault_injection) at every step of a
protocol: the hook is the injection point, and the test chooses the step and the fault. Here is a
whole test that reenters the session from inside its own write:

[`native/macos/tests/session_tests.mm:1786-1810`](../../native/macos/tests/session_tests.mm#L1786-L1810)

```obj-c++
// IMK asks for a commit from inside the first mark write, after its effect. The nested finish
// makes zero client calls and reports an interruption; the outer write is then unknown, its key
// stays consumed, and the session stops with the client's document as the write left it (S3, S4)
void testLifecycleInterruptionStatus() {
  currentTest = "lifecycle_interruption_status";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  __block NSUInteger callsAtInterruption = 0;
  __weak FakeClient *weakClient = client;
  client.hook = ^(NSString *call) {
    if ([call isEqualToString:@"setMarkedText+after"]) {
      FakeClient *strong = weakClient;
      callsAtInterruption = strong.calls;
      [session finishInClient:strong];
    }
  };
  kr_event input = textEvent(@"g", KR_EN);
  CHECK([session handleEvent:&input client:client]);
  CHECK(callsAtInterruption > 0 && client.calls == callsAtInterruption);
  CHECK(client.writes == 1 && session.stopped && !session.owned);
  CHECK(session.inputStatus == KRInputInterrupted);
  NSString *expected = [kPrefix stringByAppendingFormat:@"g%@", kSuffix];
  CHECK([client.document isEqualToString:expected]);
  client.hook = nil;
}
```

Two details from the [Objective-C chapter](objective-c.md#blocks) appear here in their natural
place. `__block` lets the block assign to a local variable of the enclosing function; without it, a
captured local is a read-only copy. `__weak` breaks a cycle: the client stores the hook, and a hook
that captured the client strongly would keep both alive forever.

The checks then state the whole contract of the schedule. After the interruption the session made no
further client call (`client.calls == callsAtInterruption`), made one write and no second, stopped,
holds no authority, says why, and the document is exactly what the one write produced. `S3, S4`
refer to the safety invariants in [ARCHITECTURE.md](../../ARCHITECTURE.md#safety-invariants).

Know the limit of this evidence. The fake proves the adapter's logic, not the behavior of real
applications: TextEdit, Safari and Firefox each answer these calls in their own way, and some
answers are wrong. The project treats controlled tests and physical application tests as different
evidence and never substitutes one for the other; [Testing](../testing.md) describes both.

> [!TIP]
> **Remember.** A fake with a hook can put a fault at any single step. Its pass says nothing about
> real applications.
