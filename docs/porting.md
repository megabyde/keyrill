# Porting to Windows and Linux

A new platform needs a native input-method adapter and nothing else: the engine, model, vocabulary
parser, word session and C ABI are shared. Read [architecture](../ARCHITECTURE.md) first; its state
table, safety invariants and rejected mechanisms apply to every adapter. Don't write a second
inference engine, and don't build a cross-platform editing abstraction before two adapters have
shown the same real contract ([ADR 001](adr/001-rust-core-native-adapters.md)).

The Windows C++ backend passed controlled-editor and bounded ordinary Notepad acceptance before the
windows-rs migration. The full Rust runtime and proof tools passed complete native and experimental
sanitizer suites. Fresh automated Rust editor checks passed correction, reversal, continuation,
native Undo, focus preservation, exclusions, native lock denial, and bounded owned shutdown. Current
Rust Notepad 11.2609.11.0 checks passed fixture correction, manual commands, continuation, native
Undo, Find exclusion, a focus round trip, both model directions, and vocabulary reload. Physical
boundary correction, reversal, and a rapid-typing and native Undo sequence also passed. The broader
physical acceptance matrix remains pending. Historical packaged x64 Notepad 11.2607.14.0 acceptance
covered the owned-composition executor, shared model, and vocabulary integration. Exact UIA checks
covered both model directions, retained reversal, rapid continuation, native Undo, and vocabulary
reloads. Physical command checks covered conversion and held reversal within one focus interval.
Other versions now pass input under bounded-only containment, as do generic TSF hosts. Editing
requires the compiled proof-editor or exact-version Notepad main-field contract, fresh native
identity, writable state, scope, owned-range verification and revocation subscriptions. Browser and
other generic application correction remains unimplemented pending authoritative per-field semantic
evidence. Full Windows qualification remains in progress. No Linux adapter exists. An explicit-PID
inspector supplies independent field evidence and named public-document checks. The evidence limits
are recorded with [Windows admission](windows.md#bounded-notepad-admission-and-observation). Each
platform starts with the same first slice: one real editor, physical keys, an owned composition, a
Rust decision, a verified update, commit, then rapid continuation and a focus change, all checked
against the whole document. Use the fixture policy (`kr_set_automatic_policy` with
`KR_POLICY_FIXTURE`, which corrects only `ghbdtn` and `руддщ`) before the model and real
dictionaries. Tray icons, settings UI and packaging come after that works.

## Building the library

The Rust side builds with the pinned toolchain on any host. On Windows use an MSVC developer shell;
on Linux any C toolchain.

```console
$ cargo test --locked --workspace
$ cargo build --locked --release -p keyrill-ffi
$ cargo rustc --locked --release -p keyrill-ffi -- --print native-static-libs
```

Link `keyrill_ffi.lib` or `libkeyrill_ffi.a` together with the system libraries the last command
prints. Include `include/keyrill.h` rather than copying its declarations; its `static_assert`s pin
the struct layouts. CI runs the portable checks on `windows-2025` and `ubuntu-24.04` as well as
`macos-15`.

## Starting an adapter

Put the code in `native/windows/` or `native/linux/`, next to `native/macos/`, with its own build
script, tests and packaging. The Rust crates change only when the ABI does.

[`crates/keyrill-ffi/tests/abi_smoke.c`](../crates/keyrill-ffi/tests/abi_smoke.c) is the smallest
working caller and the place to start. It runs the whole loop against an in-memory document: plan a
key, check the composition against `expected`, write `mark`, commit the outcome, and honor the key
disposition, including `KR_FINISH` with `KR_KEY_REPLAN`. Its scenarios type `ghbdtn` and `руддщ`
with Space under the fixture policy, restore with Escape, leave an unlisted word alone, and convert
`ёлка` and `[jhjij` manually, including under the PC keyboard. `./tools/verify` links and runs the C
caller on macOS and Linux, with the system libraries `--print native-static-libs` reports. Windows
has an independent Rust caller in `crates/keyrill-ffi/tests/windows_abi.rs`. It declares the
published C structs and signatures, checks their literal layouts, and runs all nine original smoke
scenarios against the ABI.

Wire the adapter into `tools/verify` through the host branch in the default mode. Windows dispatches
to `cargo xtask windows check`, while its separate `sanitize` mode dispatches to
`cargo xtask windows sanitize`. Desktop acceptance remains a separate gate. Platform setup goes in
the Toolchain step of `.github/workflows/verify.yml`. Clippy and rustdoc run on every lane, so
shared docs can't link to an item that exists on one OS only ([development](development.md#style)).

## The C ABI

`include/keyrill.h` is the authoritative contract; this is the map. Every key an adapter handles
goes through one loop, the one `abi_smoke.c` implements and
[how Keyrill works](how-it-works.md#typing-the-word) traces on a real word:

```mermaid
flowchart TD
    K[Key or lifecycle event] --> P["kr_plan_event → kr_plan"]
    P --> A{action}
    A -->|"PASS, UNAVAILABLE"| C0["kr_commit APPLIED_VERIFIED"]
    A -->|"MARK, FINISH"| V{"Composition holds<br/>expected exactly?"}
    V -->|no| R["kr_commit REFUSED_BEFORE_WRITE;<br/>the key passes"]
    V -->|yes| W[One native write]
    W --> RB{"Checks after the write pass?<br/>Readback, except an unchanged lifecycle commit"}
    RB -->|yes| OK["kr_commit APPLIED_VERIFIED"]
    RB -->|"no, or can't tell"| U["kr_commit ATTEMPTED_UNKNOWN;<br/>stop, never replay"]
    OK --> D{disposition}
    C0 --> D
    D -->|KEY_PASS| HP[Pass the key to the host]
    D -->|KEY_CONSUMED| HC[Consume the key]
    D -->|"KEY_REPLAN, after FINISH only, once"| P
```

- **Versioning.** Pass `KR_ABI_VERSION` from the header to `kr_engine_new(abi, &engine)`. Before 1.0
  there is no compatibility promise: any other version is refused with `KR_VERSION`, and a change
  that breaks callers bumps the minor version.
- **Threading.** One engine per input context, called serially from the thread that owns that
  context. Buffers are copied in both directions and borrowed only for the call; no pointer into
  Rust storage crosses the boundary. Lengths are UTF-16 units.
- **Composition path.** Send each key as a `kr_event` (`KR_TEXT`, `KR_BACKSPACE`, `KR_ESCAPE`,
  `KR_MANUAL`, `KR_REVERSE`, or `KR_FINISH_REQUEST` for any other key or lifecycle request) with the
  mode and a context tag to `kr_plan_event`: `KR_CTX_BOUNDARY` when the caret is at the document
  start or follows whitespace, `KR_CTX_INELIGIBLE` otherwise, including when you didn't read the
  prefix. The `kr_plan` says what to do (`KR_PASS`, `KR_MARK`, `KR_FINISH`, `KR_UNAVAILABLE`), what
  the composition must contain beforehand, the new mark and what to do with the key (`KR_KEY_PASS`,
  `KR_KEY_CONSUMED`, `KR_KEY_REPLAN`). The caret goes at the end of the mark, `mark_len`; `KR_MARK`
  with `mark_len` 0 empties the composition. Perform at most that one write, verify it, then
  `kr_commit` with `KR_APPLIED_VERIFIED`, `KR_REFUSED_BEFORE_WRITE` or `KR_ATTEMPTED_UNKNOWN`.
- **Policy and dictionaries.** `kr_set_automatic_policy` selects off, fixture or model while idle.
  `kr_prepare_model` warms the existing process-wide model cache on a worker without an engine.
  `kr_dictionary_parse_utf8` creates an immutable dictionary handle off the input thread;
  `kr_adopt_dictionaries` borrows a pair on the engine owner and retains independent references.
  Free handles with `kr_dictionary_free` after their borrows drain, even after adoption. A null
  language handle means missing; a parsed empty source means ready. Pair adoption rejects any
  pending proposal and preserves policy, word history and reversal receipts.
  `kr_set_dictionary_utf8` publishes a whole dictionary (at most 1 MiB, NULL+0 is a valid empty
  source); `kr_clear_dictionary` makes a language missing. The `kr_vocabulary_*` handle reads and
  settles the files for you, as [macOS](macos.md#dictionaries) describes; the adapter owns the
  serial queue it observes on and adopts snapshots outside input callbacks. Paths are UTF-8 on
  Windows. `kr_vocabulary_configure_default` takes the home directory and names Vim's own files
  under it, `~/vimfiles/spell/{en,ru}.utf-8.add` on Windows and `~/.vim/spell` elsewhere. std has no
  stable file identity on Windows, so a read there is checked against size and the modified and
  creation times; the byte comparison between two observations still catches a same-size
  replacement. Learned slots `KR_LEARNED` through `KR_LEARNED + 3` take the OS spell checker's own
  lists as plain word lists. On Windows these are `%AppData%\Microsoft\Spelling\<tag>\default.dic`
  for added words and `default.exc`, configured with `KR_SOURCE_EXCLUDED`, for excluded ones; the
  adapter picks the language tags.
- **Failures.** Status codes are content-free. A Rust panic is caught at the boundary and
  permanently poisons the handle (`KR_POISONED`).
- **Compatibility path.** `kr_plan_compatibility_event` and `kr_commit_compatibility` are the macOS
  synthetic-rewrite backend. An adapter doesn't need them, and must not fall back to them when a
  native edit fails.

Dictionaries cross threads the one way that keeps file I/O out of key handling:

```mermaid
flowchart TD
    subgraph Worker["Serial worker queue"]
        O["kr_vocabulary_observe:<br/>read, settle, parse"]
    end
    O -->|immutable snapshot| V[("kr_vocabulary")]
    subgraph Input["Input thread, outside callbacks, no pending plan"]
        A["kr_adopt_vocabulary"] --> E["kr_engine,<br/>one per input context"]
    end
    V --> A
```

The session uses the Apple Russian map unless `kr_set_keyboard` selects `KR_KEYBOARD_PC`; they
differ only on the ё key (`\` on Apple, `` ` `` on PC).

## Adapter obligations

These follow from the architecture and apply on every platform:

- Track a generation for each input context; invalidate it on focus, context, composition and source
  changes, and recheck it after every native call that can reenter.
- Before a write, check that the composition you own still contains exactly the plan's expected
  text, at the expected position, with the caret where you left it. A commit that leaves the text
  unchanged can replace the exact composition range without the caret check. After the write, read
  the result back. If you can't read it back, the client can't be admitted.
- Report `KR_ATTEMPTED_UNKNOWN` when you can't tell what happened, and never replay the key or try
  to repair the document.
- Set key disposition from what actually happened, not from whether a request was accepted.
- Exclude password, secure and read-only fields before reading anything or starting a composition.
  Recheck current scope, writability, exact field identity, and generation after callback-capable
  preparation before every character read or native effect. Metadata checks cannot make document
  authority atomic. Exclusion after an attempted character write forbids further character reads and
  yields Unknown without replan, replay, or repair.
- Never hold a Rust borrow, a dictionary lock or the engine across a callback into the host. Contain
  native exceptions on the native side.
- Keep raw input, personal vocabulary and paths out of logs and status.

## Windows: Text Services Framework

The windows-rs implementation is in `native/windows/rust/`. Its build, maintainer-only registration
commands, evidence limits, and physical acceptance matrix are in [Windows TSF proof](windows.md).
The bounded first-editor proof includes observed owned termination on focus loss in the historical
C++ backend. The Rust Notepad physical results are recorded separately from the pending
completion-candidate matrix. Bounded field admission requires the
[native contract](windows-admission-contract.md), including coherent metadata and revocation
notifications. Generic TSF scope admission is disabled. Granted edit sessions, explicit contexts,
and owned compositions provide the native editing mechanism. [TSF design](adr/007-windows-tsf.md).

Exact version recognition includes 11.2607.14.0 and 11.2609.11.0. The updated version has matching
live native-field and unavailable-scope evidence. The current Rust DLL passed the
[fixture-policy Notepad checks](windows.md#rust-notepad-verification).

The [`windows-rs`](https://github.com/microsoft/windows-rs) adapter uses `windows` and
`windows-core` 0.62.2 for generated COM identity and SDK types. Typed leases in `foreign.rs` own
host COM references. Raw SDK vtables preserve exact HRESULTs. The service contains Rust panics at
callbacks and requires foreign COM implementations to obey the standard HRESULT and no-throw
contract. The C++ exception-recovery layer is retired. The proof editor, independent raw COM
providers, and UIA diagnostics are Rust in `native/windows/proof`. `native/windows/build-support`
owns source and artifact identities, and `tools/xtask` owns the Windows Cargo build and verification
commands. Public COM diagnostic and wire versions retain their contracts. The private test API is
version 3. The scenario inventory maps all 25 former CTest entries and 111 families to Rust tests or
explicit retirement reasons. Full native execution and the complete experimental Rust ASan suite
have passed. The ASan memory-error canary and byte-for-byte Rust model regeneration have passed. UIA
supplies test and diagnostic observations. Unsupported, secure, excluded, and uncertain controls
pass input. A hook or SendInput backend requires a separate decision.

The Windows executor replaces the exact owned range once for each MARK or changed FINISH. Ordinary
unchanged FINISH ends the exact verified composition without `SetText`. Retirement retains its
unchanged replacement protocol. Unchanged FINISH has no expected end-caret requirement and does not
force selection collapse. It captures immutable selection coordinates and style, verifies range
extent and UTF-16 text before and after `EndComposition`, and checks that no composition remains.
After those checks succeed, it preserves the complete selection using a metadata-only range clone
and at most one `SetSelection`. Failed checks stop without retry or another character write. A
FINISH followed by a replanned MARK writes once on the new composition. Controlled providers cover
selection movement, clone gravity, exact selection styles, fresh policy refusal, and
attempted-unknown termination without replay or recovery. Historical C++ Notepad checks cover rapid
continuation, native Undo, caret movement, and whole-document selection. The complete
selection-style matrix has controlled coverage.

The Windows branch uses bounded `PreserveKey` delivery for manual conversion and reversal. Two exact
left Ctrl + left Alt commands and one declined Alt-press observer belong to one activation through a
shared binding table. A provisional stack owner accounts for late successful registrations before
rollback, and reactivation waits for the whole activation call and all captured releases. Unknown
mandatory acquisition or cleanup effects fault the instance. Ordinary and preserved callbacks share
native admission and the existing executor. A command GUID supplies no modifier, repeat, or
document-authority facts. Historical C++ physical Notepad runs observed command delivery and
retained ownership through modifier release, as described in
[the command contract](windows.md#manual-conversion-and-reversal). The Windows host probe uses a
separate protocol-4 command and modifier-release observation window. Callback-local tickets preserve
command identity through reentry. Lifecycle anchors order those content-free observations against
owned termination. Dispatch records executor entry. Separate completion records distinguish verified
manual MARK, no-write reservation, refusal and unknown effects. The observer witnesses an Alt cycle
before a verified command can grant one release permission on its owned composition. Release
validation performs metadata queries only. Missing original release plus missing later observer is
an excluded indistinguishable host schedule. Historical C++ Notepad checks observed delivery and
retention, with separate generated bare-Alt checks and the physical coverage limits recorded in
[Notepad acceptance](windows.md#notepad-behavior-acceptance).

| Interface | Documented behavior | Obligation |
| -- | -- | -- |
| [`ITfContext::RequestEditSession`](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcontext-requesteditsession) | Separate request HRESULT and session result; sync and async grants differ | Check both; handle denial or deferral without a speculative edit or a wrongly eaten key |
| [`ITfEditSession::DoEditSession`](https://learn.microsoft.com/en-us/windows/win32/tsf/edit-sessions) | Supplies an edit cookie for that session | Use it for that session only; never cache a cookie |
| [`StartComposition`](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcontextcomposition-startcomposition) | Needs a write cookie; an owner can reject with success and a null composition | Require a non-null composition and the exact context before updating |
| [`ITfRange::SetText`](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfrange-settext) | Mutates under edit access | Use the owned range; the HRESULT is not a document check |
| [`OnKeyDown` / `pfEaten`](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfkeyeventsink-onkeydown) | Key consumption is explicit | Set it from actual handling, not from an accepted lock request |
| [`OnCompositionTerminated`](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcompositionsink-oncompositionterminated) | Identifies the terminated composition | Compare with the captured composition; don't end a newer one |

Start with synchronous read/write edit sessions on the key path. If a path needs asynchronous
grants, capture the exact context, composition and generation, revalidate on grant and prove key
order; don't queue and replay input to make async easier. Edit-session locks regulate access, not
rollback, Undo grouping or focus after a later callback. An in-process TSF DLL shares the editor's
process: a crash takes down the editor, and process-global Rust state such as a panic hook affects
the host, so don't install one.

> [!WARNING]
> A text service runs inside every application that uses it. A crash, a leak or a blocked callback
> in the adapter is the editor's crash, leak or hang.

Determine COM registration, profile registration, enablement and activation separately, and look for
a per-user installation before anything that needs elevation. The Microsoft
[`textservice-step06-3`](https://github.com/microsoft/Windows-classic-samples/tree/main/Samples/Win7Samples/winui/input/tsf/textservice/textservice-step06-3)
sample is a reference for structure, not error handling: it reports keys eaten too early, uses a
composition without checking it started, ends the current rather than the captured composition and
translates keys inadequately. A sample that turns virtual keys into uppercase letters is not an
EN/RU input method; verify Shift, Caps Lock, dead keys and the EN/RU layouts against real input.
Intel/AMD x64 and x86 clients have separate artifacts and execution results. ARM64 is outside the
current Windows scope.

Acceptance for the first slice includes edit-session denial, a rejected composition, focus A→B→A,
rapid continuation, read-only and password fields, a source change and service teardown, each with
the whole document, caret, composition identity and key disposition recorded.

## Linux

There is no Linux design yet; this is the starting point, not a decision. The likely target is an
[IBus](https://ibus.github.io/docs/ibus-1.5/IBusEngine.html) engine or a
[Fcitx 5](https://fcitx-im.org/wiki/Develop_an_simple_input_method) addon, both of which have
preedit text (the counterpart of a marked composition), commit, focus and reset callbacks, and a
surrounding-text API. Questions to answer with real clients before choosing:

- Does the preedit stay owned and readable across the GTK, Qt, Electron, Firefox and Chromium
  frontends, under X11 and under Wayland's `text-input-v3`? Surrounding text is optional for
  clients, so readback may be unavailable exactly where it's needed.
- Can the word plus Space stay in preedit until the next key, as on macOS, without breaking
  completion or Undo?
- Do content-type hints (IBus input purpose, Fcitx capability flags such as password and sensitive
  data) give a per-field secure-input signal? macOS only has a global one.
- Is IBus or Fcitx 5 the right first target for the desktops that matter?

The acknowledgement contract is the open decision: a Linux adapter has to give every row of the
[edit contract](../ARCHITECTURE.md#edit-contract) a check that proves its outcome. On macOS Keyrill
reads the client's marked text back after every write a key causes; only an unchanged commit that
IMK requests is written without that read. On Linux the client draws the preedit the engine sends,
and GTK's surrounding text explicitly leaves the preedit out
([`set_surrounding_with_selection`](https://docs.gtk.org/gtk4/method.IMContext.set_surrounding_with_selection.html)),
so there's nothing to read back.

> [!IMPORTANT]
> Echoing the engine's own preedit as `KR_APPLIED_VERIFIED` would verify bookkeeping, not the
> document.

Before writing the adapter, decide whether engine-owned preedit gets its own, weaker acknowledgement
in the ABI, or whether those clients stay pass-through. Until then a client without readback of the
owned preedit gets pass-through, not correction.
