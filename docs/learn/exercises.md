# Exercises

*Goal: break each guarantee from the earlier chapters on purpose, and predict how it fails.*

Each exercise is one small change that breaks a guarantee the earlier chapters described. Write down
your prediction first: which tool complains, with which error, and why. Then run the command and
compare. Predicting an error teaches more than reading about one, and a wrong prediction is the most
useful result you can get, because it shows exactly which idea to reread. Every answer below was
reproduced on this code, and the error texts are quoted from the compiler or the test runner.

A **worktree** is a second checkout of the same Git repository in another directory. Work in one, so
that nothing touches your own checkout. Only exercises 7 and 9 need macOS, and they use the fake
client; none of the exercises installs or selects the input method.

```sh
git worktree add --detach ../keyrill-lesson HEAD
cd ../keyrill-lesson
git checkout .                     # after each exercise
cd - && git worktree remove ../keyrill-lesson   # when finished
```

Each exercise starts from a clean checkout. Run `git checkout .` after each one and confirm that
`git status --short` prints nothing before you start the next; the one exception is exercise 4,
which keeps the change from exercise 3. When you are done, remove the worktree with the last line
above.

| # | Change | Run | Question |
| -- | -- | -- | -- |
| 1 | Add `self.serial += 1;` as the first line of `transition` in `crates/keyrill-core/src/session.rs` | `cargo check -p keyrill-core` | Can a method that takes `&self` be made to write? |
| 2 | Add a variant `Teleport` to `enum Action` in `session.rs` | `cargo check -p keyrill-ffi` | Which crate complains, and where? |
| 3 | Append `fn sneak() { unsafe {} }` to `crates/keyrill-core/src/mapping.rs` | `cargo check -p keyrill-core` | Does an empty `unsafe` block count? |
| 4 | Put `#[allow(unsafe_code)]` on the line above `sneak` | `cargo check -p keyrill-core` | Can one function opt out of `forbid`? |
| 5 | In `NativeEvent` in `crates/keyrill-ffi/src/lib.rs`, change `units: [u16; 8]` to `[u16; 9]` | `cargo check -p keyrill-ffi` | Who notices the ABI change? |
| 6 | Delete `#[repr(C)]` above `NativePlan` | `cargo check -p keyrill-ffi` | Does anything notice, given that the size may stay the same? |
| 7 | In `include/keyrill.h`, delete the three lines that open `extern "C" {` and the `}` that closes it | `native/macos/build.sh check` | The header still compiles as C; what does the adapter see? |
| 8 | In `acknowledge`, change `outcome == Outcome::Verified` to `outcome != Outcome::Unknown` | `cargo test -p keyrill-core` | What goes wrong if a refusal advances the state? |
| 9 | In `apply:` in `native/macos/src/KRSession.mm`, delete the generation check right after the write (lines 507 and 508) | `native/macos/build.sh check` | Is one missing recheck observable? |

## Answers

Read these only after you have written down a prediction for every exercise.

1. `` error[E0594]: cannot assign to `self.serial`, which is behind a `&` reference ``. It can't,
   without changing the signature to `&mut self`, and then every caller that relies on the
   transition being pure would have to change too. See
   [ownership and borrowing](rust.md#ownership-and-borrowing).
1. `` error[E0004]: non-exhaustive patterns: `keyrill_core::session::Action::Teleport` not covered ``,
   in `keyrill-ffi`, at the `match` that turns an action into its C number; the core itself still
   compiles. The new case can't cross the ABI silently. See
   [`match` keeps count](rust.md#match-keeps-count).
1. `` error: usage of an `unsafe` block ``. The lint looks at the block, not at what is inside it,
   and the compiler also warns that the block is unnecessary.
1. `error[E0453]: allow(unsafe_code) incompatible with previous forbid`, followed by the same error
   as in exercise 3. That is the difference between `forbid` and `deny`: a `deny` could be relaxed
   for one item, a `forbid` can't.
1. `error[E0080]`, with `assertion failed: size_of::<NativeEvent>() == 32`. The check runs inside
   the compiler, before anything links. See [Step 1](boundary.md#step-1-agree-on-memory-layout).
1. `error[E0080]`, with `assertion failed: std::mem::offset_of!(NativePlan, expected) == 24`.
   Without `repr(C)` the compiler reorders the fields; the size assertion alone would not catch it,
   which is why the offset is pinned as well.
1. The adapter compiles, then fails to link: `Undefined symbols for architecture arm64:` followed by
   names such as `"kr_engine_new(unsigned int, kr_engine**)"`. Those are C++ signatures, demangled
   by the linker; the Rust library exports `_kr_engine_new`, without parameter types. See
   [C linkage](objective-cpp.md#c-linkage).
1. Two tests fail: `unknown_refused_and_reset_expire_pending_and_receipt` and
   `history_is_acknowledged_bounded_and_expires_with_authority`. A refused plan then installs its
   proposed state although nothing was written, and Rust believes in a mark the document doesn't
   have.
1. `session_tests: 1 failure(s)`, in the case `lifecycle_interruption_status`, the test shown in
   [Reentrancy and tests](reentrancy-and-tests.md#a-hostile-fake-client). It reenters from inside
   the write and then requires that the adapter make no further client call. Without the check the
   adapter goes on to read the mark back from a client it no longer has authority over.

## Reading order for the source

| Read | For |
| -- | -- |
| `crates/keyrill-core/src/mapping.rs` | 125 lines of idiomatic Rust with tests. Start here. |
| `crates/keyrill-core/src/lib.rs` | A complete word in one doc example, which `cargo test --doc` runs. |
| `crates/keyrill-core/src/session.rs` | The state machine, with its diagrams in the module header. |
| `include/keyrill.h` | The contract as C sees it. The opening comment is the best summary in the repository. |
| `crates/keyrill-ffi/src/lib.rs` | `with_engine`, then `kr_plan_event` and `kr_commit`. |
| `native/macos/src/KRKeys.mm` | 47 lines of Objective-C++ in its gentlest form. |
| `native/macos/src/KRSession.h`, `KRSession.mm` | The header's schedule first, then `runEvent:` and `apply:`. |
| `native/macos/tests/session_tests.mm` | `FakeClient`, then one reentrancy test. |
