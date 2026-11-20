# Windows TSF proof

## Client architecture builds

The Windows runner accepts `--arch x64`, `--arch x86` and `--arch all` for `build`,
`controlled` and `check`. Existing commands default to x64. Intel and AMD x64 use
`x86_64-pc-windows-msvc`; 32-bit processes use `i686-pc-windows-msvc`. ARM64 is unsupported.
Both architectures run from an x64 Windows host. Target installation requires maintainer
approval. Commit `db42fa1` passed local full verification of actual x64 and x86 compilation,
controlled COM execution, independent ABI checks, registration-free export loading, and
activation. Final completion-candidate verification and installed mixed-bitness application
acceptance remain separate gates.

```console
cargo xtask windows build --arch all
cargo xtask windows controlled --arch x86 --case scenarios_boundary::boundary_requests_complete_once_and_expire
cargo xtask windows check --arch all
cargo xtask windows fingerprint production --arch x86
```

Use an exact compiled test name with `--case`. The `--case` and `--arch` options can occur in
either order. The x64 build stays in `target/windows-rust-check`, and x86 uses
`target/windows-rust-check-x86`. Each contains separate production, test-API and proof Cargo
directories and an architecture-specific artifact manifest. Artifact fingerprints include the
target triple and feature domain. PE verification checks the selected machine and optional
header pair before execution. Proof activation manifests use the selected client architecture.
The experimental sanitizer supports x64 only and rejects x86 and all explicitly.

This is an x64 and x86 Text Services Framework adapter with manual conversion and configurable automatic
correction. The `keyrill-windows` package in `native/windows/rust/` implements the service and
registrar with `windows` and `windows-core` 0.62.2. The proof editor, UIA inspectors,
controlled COM providers, and build tools are also Rust. The service links the portable engine
through [the C ABI](../include/keyrill.h). It has no replacement backend,
hooks, input injection, clipboard transport, or runtime network access. Editing requires a
compiled bounded field contract under the [native admission contract](windows-admission-contract.md).
Generic TSF hosts remain unadmitted even with ordinary scope. The tested version-bounded Notepad
unavailable-scope exception retains its separate requirements.
The [completion plan](windows-completion-plan.md) defines the pending application acceptance matrix.

Complete persisted preferences default automatic correction off. Automatic, Manual, and Off
application policies restrict native admission. The on-demand Win32 Settings editor uses complete
revision conflict detection. The [package guide](windows-package.md) describes production profiles,
dual-architecture artifacts, explicit installation, and recovery. Actual installed application and
package-lifecycle results must identify the candidate used.

## Application qualification

Experimental package `940ee5df` passed local x64 and x86 native checks, the x64 sanitizer
gate, and a clean reproducibility check. Installed COM activation loaded the expected DLL in
an x64 process and an x86 process. The subsequent installed candidate `f3831bbe` restricts
editing to fields for which the adapter can verify ownership and field eligibility. The table
identifies which application checks used that candidate.

| Application | Evidence and remaining checks |
|---|---|
| Notepad | On 11.2609.11.0 x64 with `f3831bbe`, UI Automation confirmed physical manual conversion and reversal, with both commands on the same owned TSF composition. In separate empty-document tests, typing the physical C G B C J R keys under Keyrill English produced `список`, and typing W O R L D under Keyrill Russian produced `world`. Each sequence ended with Space. UI Automation confirmed both words, the trailing space and the collapsed caret. Per-edit TSF evidence was unavailable for both automatic corrections. |
| Chrome | On 157.0.8103.79 x64 with `f3831bbe`, the maintainer saw the public test character in both the textarea and the synthetic payment field. The English service received two key callbacks. Both sessions completed at the `ProofWindow` refusal with zero consumed keys, attempted edits or owned compositions. Field identity and visible text were reported by the maintainer. No independent field or caret observation was available. An earlier build restored text with part of the suffix selected on native Undo in rich `contenteditable`. [Undo investigation](windows-chrome-undo.md). |
| Edge | Earlier candidate `940ee5df` admitted the payment fixture. On 157.0.4341.60 x64 with `f3831bbe`, both the ordinary textarea and the reported payment-field sequence passed input through with zero attempted writes. Editing and native Undo selection remain open. [Edge qualification](#edge-qualification), [payment counterexample](windows-admission-contract.md#browser-payment-field-counterexample). |
| Firefox | On ESR 153.7.0 x86, the maintainer saw the public test character in the ordinary textarea and the synthetic payment field. In separate windows, the matching English service recorded two and one key callbacks respectively. All stopped at `ProofWindow` with zero consumed keys, attempted edits or owned compositions. Independent field and caret observations and browser correction remain open. |
| VS Code Insiders | On 1.147.0 x64 with `f3831bbe`, UI Automation verified the full public document and caret after physical input of `ghbdtn`. All six key callbacks stopped at `ProofWindow` with zero attempted edits. This test used an isolated profile with accessibility support enabled and extensions disabled. Default-profile behavior and correction remain open. |
| Word | On 16.0.20527.20196 x64 with `f3831bbe`, the public Compatibility Mode RTF body retained physical `ghbdtn`. UI Automation verified the full document and caret. All six key callbacks stopped at `ProofWindow` with zero attempted edits. The earlier new-document and table-cell results used an older DLL. [Word editing limits](#word-editing-limits). |
| Windows Terminal | Physical ordinary and no-echo input reached the child unchanged on 1.25.2733.0 x64. Keyrill activation and manual commands remain unverified. [Terminal checks](#input-scope-in-windows-terminal-12527330). |

Use the [maintainer checklist](windows-maintainer-checklist.md) for physical input and package
lifecycle cases. Record the exact candidate and application architecture with each result.

### Edge qualification

The public input passed the initial text and caret check. The following results cover the
public `textarea` and rich `contenteditable` in Edge 157.0.4341.60 x64.

The service observer matched candidate `940ee5df` in Edge 157.0.4341.60 x64 with Keyrill
English active. The preserved left Ctrl+Alt+K and left Ctrl+Alt+Z commands both reached
the executor and recorded a verified `ManualMark` on the same owned composition. The
service reported zero unknown effects, and focus retirement ended the composition. The
maintainer reported `ghbdtn` at the end. The independent UIA observer missed the initial
`textarea` reset and the intermediate conversion state during that physical run.

A separate run used automated keystrokes on the same candidate and Edge version. The UIA
observer matched the whole public `textarea` and the exact caret after the reset, after typing
`ghbdtn`, after K converted it to `привет`, and after Z restored `ghbdtn`. Each command added
one verified write in the same owned composition. Right then Left ended the composition and
returned the caret to the end of the word. A second Z reached the executor, which refused it
with zero additional writes. The final text and caret matched, and the service reported zero
unknown effects throughout. Exact intermediate document states during physical input remain
pending.

With automatic correction enabled, generated `cgbcjr` followed by Space became `список`
followed by one Space. Ctrl+Alt+Z restored `cgbcjr` with the Space in the same owned
composition. Independent UIA checks matched the full text and caret after conversion,
reversal, and composition end.

In the rich field, Ctrl+Z after generated `x` restored the original text and selected UTF-16
offsets 19 through 20. After generated `xy` and Escape, Ctrl+Z restored the original text and
selected offsets 19 through 21. Keyrill owned no composition before the second Ctrl+Z and its
write count stayed fixed during that Undo. The selection matches the discrepancy observed in
[Chrome](windows-chrome-undo.md). Ctrl+Y restored `xy` and collapsed the caret after it at
offset 21. The browser report and accessible field value matched, and Keyrill's write count
stayed fixed. Both Undo tests used Keyrill English. The ordinary English baseline and physical
Undo and Redo comparisons in Edge remain pending.

## Automatic source handoff

When automatic correction and source switching are enabled, a verified converted word plus
Space retains its original source while its owned reversal mark exists. Z, Escape, and manual
conversion cancel pending source selection. The next qualifying letter must first obtain a
verified natural FINISH. That FINISH expires the reversal receipt and releases the first edit
session before native source selection.

The source-only operation checks the actual target profile, exact standard substitute layout,
loaded-layout list, enablement, current source, focus, field, keyboard state, and complete
configuration identity. It requests one language change and at most one flags-zero profile
activation. Fresh native readback precedes a new admission and edit session. The same synchronous
input callback may continue once using the confirmed target layout. This changes the source
indicator on the qualifying next letter.

Every observed source change invalidates old document authority. Matching callback facts cannot
establish who caused them. The source-only interval uses the scope last verified at FINISH and
the supported host's coherent metadata and revocation notifications. A silent scope change can
leave the OS source changed. Fresh second-session exclusion then prevents character inspection,
composition start, and character writes. Failures stop further source mutations and expire
continuation authority. Native host callback ordering and physical first-key behavior still
require qualification against the final candidate.

## Recorded runtime proof

The historical desktop acceptance results below describe the C++ backend. Automated Rust
editor and current Notepad checks have separate records below. Full Rust native execution and the
complete experimental Rust sanitizer suite have passed. Current Rust Notepad checks passed
fixture behavior, both model directions, and vocabulary reload. Physical Notepad boundary
correction and reversal passed the isolated sequence below. The broader physical matrix
remains pending. The Rust model checker has passed byte-for-byte
regeneration of the unchanged model, including gzip grammar, micro-frequency data, provenance,
and notices.

The historical first controlled-editor proof passed on Windows x64 with the fixture policy. The
maintainer registered and enabled both proof profiles. Physical runs passed correction, rapid
continuation, native Undo, focus preservation, excluded-control checks, native lock denial,
and bounded owned-shutdown and empty-close assertions. Automated real-editor runs separately
passed correction, Escape reversal, continuation, and lifecycle assertions. Automated keys do
not count as physical keyboard evidence.

The earlier unchanged-text correction attempt was superseded by later verified correction.
An initial caret mismatch and a later automated Escape end-caret defect remain part of the
history. The changing-finish fix passed its controlled regression and the same automated
correction and reversal document/caret oracles without a caret repair.

The final lifecycle-journal build has case-level Tab and A-to-B-to-A evidence under automated
keys and clicks. Tab recorded EndRequested and FinishVerified for the owned serial before
document-focus invalidation. Clicking B and then A each recorded ExactTermination for the mark
owned in the departing field before focus invalidation. The exact correction and final focus
document/caret oracles passed, with zero Unknown effects. These runs establish the observed
fixture sequence, not physical keyboard evidence or general application support. The native
EN-to-RU experiment below also passed with fresh owned evidence and an independent no-Space
document/caret oracle. This completes the controlled-editor proof, not general Windows support.
Manual commands, shared model correction in both directions, and vocabulary reloads also passed
automated real-editor acceptance as described below. Ordinary Notepad acceptance is recorded in
the bounded admission section. Those historical runs preceded the completion work for broader
application coverage, settings, and packaging. Unretained final destruction and complete callback drainage are not established
by the bounded shutdown assertion.

## Automated Rust editor verification

Commit `f16fc87` passed fresh generated-key checks in the Rust RichEdit proof editor with its
adjacent Rust production DLL. The frozen production identity was
`07358901ea4ec515bc4f5534e20ebb26b5c629ee3449ad635b5231e10dc5383b`.
EN mode and the fixture policy were confirmed before input. The independent Rust UIA inspector
checked complete public A and B documents and their required caret positions.

| Check | Observed result |
|---|---|
| Boundary correction | `ghbdtn` followed by Space became `привет ` in one owned composition |
| Escape reversal | The exact document returned to `ghbdtn ` with its end caret |
| Continuation and native Undo | Typing `x` produced `привет x`; Ctrl+Z restored `привет ` and ended ownership |
| A-to-B-to-A focus | Splitting `ghb` and `dtn ` across a focus change left A as `привет ghbdtn ` and B as `x` |
| Excluded controls | The read-only field stayed empty; the password fixture accepted masked public test input; service edit counters stayed unchanged |
| Native lock denial | All seven keys passed through as `ghbdtn `; the session result was `0x80040208`, with zero session entries and write attempts |
| Owned shutdown | A verified owned word passed `shutdown-ready`, then native Close returned exit code 0; the empty-window negative control returned `NoOwnedMark` (16) |

Password contents were not queried. The observed service snapshots contained zero unknown
effects. These runs used generated keys and clicks. Renewed physical proof-editor checks and
the live EN-to-RU transition remain pending. Current Rust Notepad results appear below.

## Rust Notepad verification

The Rust service passed generated-key checks in packaged x64 Notepad 11.2609.11.0 after the
maintainer registered the candidate and selected the EN profile. The loaded production identity
was `22221d8da8de645c17251ff7be941b1c60eba0160f53082dd6b1a365aa0aa9bf`.
The independent Rust UIA inspector checked complete public documents and collapsed end carets.
The service observer required that same build identity and observed actual key callbacks.

| Check | Observed result |
|---|---|
| Boundary correction | `ghbdtn` and Space became `привет ` in an owned composition |
| Manual reversal | Left Ctrl + left Alt + Z restored `ghbdtn ` |
| Manual conversion | Left Ctrl + left Alt + K converted owned `ghbdtn` to `привет` |
| Continuation and native Undo | `x` produced `привет x`, then Ctrl+Z restored `привет ` and ended ownership |
| Find exclusion | Find accepted public `x` while service write counters stayed unchanged and ownership stayed empty |
| Focus round trip | `ghb`, Find, then document `dtn` and Space left the exact document `ghbdtn ` |

The observer reported zero unknown effects. Find's native field failed binding admission before
character access. After returning to the document, the main RichEdit field again passed the
native and independent UIA binding checks. These runs used generated keys and clicks.

An isolated physical run then passed boundary correction and left Ctrl + left Alt + Z reversal
within one focus interval. The maintainer typed `ghbdtn` and Space, observed `привет `, and
pressed the command before leaving Notepad. UIA matched the complete final `ghbdtn ` document
and end caret. The journal recorded Reverse dispatch and verified `ManualMark` completion on
the same owned serial, followed by a matched consumed Alt release. Unknown effects stayed zero.
The earlier combined native Undo and reversal procedure had an incorrect expected result.
Native Undo ended ownership, and its later command was reserved without a write. Earlier
reports of unchanged text add no passing physical coverage.

A fresh Notepad process with the same DLL also passed generated-key EN model checks. The
service reported requested and effective model policy, a prepared model, both synthetic
dictionary sources ready, and zero source errors. The complete-document and end-caret oracle
matched `список ` after typing `cgbcjr` and Space.

Reloading the public synthetic EN vocabulary preserved the mark and reversal receipt.
Reversal returned the complete document to `cgbcjr ` on the same owned serial with one
verified write. A newly typed `cgbcjr` and Space then stayed literal under that vocabulary.
Unknown effects stayed zero. The synthetic EN file was restored to empty afterward.

After maintainer selection of the RU profile, generated keys produced `цщкдв` and Space.
UIA matched the complete corrected `world ` document and end caret. Escape then restored
`цщкдв ` and ended ownership. The service confirmed RU mode, effective model policy, both
sources ready, and zero unknown effects.

The maintainer also completed a physical RU quick burst using W, O, R, L, D, Space, and X,
followed immediately by native Ctrl+Z within one focus interval. Before the maintainer returned
to chat, UIA matched the final `world ` document and end caret. The observer reported ended
ownership and zero unknown effects. UIA coverage for this physical sequence is its final
document and caret. The generated-key continuation check above covers the intermediate result.

Held commands and an owned EN-to-RU transition still require renewed desktop checks for this
Rust build. The historical C++ results below retain their original scope.

## Build and check without installation

Use Windows x64 with Visual Studio 2022 Build Tools, its MSVC linker and Windows SDK, and
the repository's pinned Rust 1.99.1 toolchain. The sanitizer mode also needs the installed x64
AddressSanitizer runtime. Populate the locked Cargo dependency cache before building.
Artifact builds use `--locked --offline`.

```powershell
cargo xtask windows build
cargo xtask windows check
cargo xtask windows sanitize
```

From Git Bash, `./tools/verify` dispatches to `cargo xtask windows check`.
The check rejects superseded executable sources in `native/windows`, regenerates and compares
the model with the Rust checker, and runs rustfmt, Clippy, workspace tests, rustdoc, and release
builds. It builds the independent proof tools and both service variants, runs controlled COM,
ABI, worker, diagnostic, and registration-free smoke tests, and checks scenario coverage.
`cargo xtask windows controlled` builds and runs the independent controlled DLL suite alone.
For an implementation spot check, `cargo xtask windows controlled --case
correction_preserves_complete_documents` runs one exact compiled test with the same DLL identity
checks. An unknown or partial name fails. The full verification commands require every selected
test to appear in successful execution output before recording scenario coverage.
`cargo xtask windows model-check` runs the model comparison alone.

`./tools/verify sanitize` dispatches to `cargo xtask windows sanitize`. This is a separate,
experimental Rust AddressSanitizer mode using the pinned compiler with child-local
`RUSTC_BOOTSTRAP=1` and `-Zsanitizer=address`. Before building the instrumented artifacts, the
runner requires a real heap-use-after-free canary to fail with the expected ASan diagnostic.
The canary and complete instrumented suite have passed. The prebuilt
standard library and Windows code remain uninstrumented. Sanitizer checks and physical
application acceptance are separate gates.

The build stages these artifacts directly in `target/windows-rust-check`.
Sanitizer artifacts use the separate `target/windows-rust-sanitize` directory.

| Artifact | Purpose |
|---|---|
| `KeyrillTsf.dll` | Production Rust in-process STA COM text service |
| `KeyrillTsfTest.dll` | Rust service with the controlled `test-api` feature |
| `KeyrillRegister.exe` | Rust registrar with explicit maintainer registration, status, and enablement commands |
| `KeyrillProofEditor.exe` | Stock `RICHEDIT50W` editor with writable A and B, excluded controls, and separate host and service status |
| `KeyrillProofInspect.exe` | Read-only UIA fixture assertions and bounded host and service snapshots |
| `KeyrillHostInspect.exe` | Explicit-PID field metadata, optional service observation, and named public-document UIA checks |
| `KeyrillDllSmoke.exe` | Direct DLL factory, registration-free activation, and missing-adjacent-DLL checks |
| `KeyrillWorkerSmoke.exe` | Actual DLL worker and vocabulary lifetime checks |
| `KeyrillSettings.exe` | Native editor for complete per-user preferences |
| `KeyrillDiagnosticDecoder.exe` | Independent diagnostic decoding for production and test identities |

Four packages own the Windows implementation. `keyrill-windows` in `native/windows/rust`
owns the runtime and registrar. `keyrill-windows-proof` in `native/windows/proof` owns the editor,
inspectors, and independent raw COM providers. `keyrill-windows-build` in
`native/windows/build-support` computes source and artifact identities. `keyrill-xtask` in
`tools/xtask` builds and verifies the port. These packages use Cargo and Rust build scripts.
The shared macOS and portable model-packing workflows retain their Python tools.

Production and `test-api` artifacts use separate `cargo-production` and `cargo-test-api`
directories. Proof artifacts use `cargo-proof`. The Rust build-support package hashes normalized
source inputs and configuration, including the feature set and instrumentation. The runner
rejects source changes during a build, checks the DLL export boundary, and writes identities and
artifact hashes to `artifacts.json`. The independent readers use the expected production identity.
`cargo xtask windows inventory` lists the inputs. `cargo xtask windows fingerprint production`
and `cargo xtask windows fingerprint test-api` print the respective ordinary release identities.

`KeyrillProofEditor.exe` and `KeyrillDllSmoke.exe` embed the same
`native/windows/proof_activation.manifest`. Its COM mapping binds the service CLSID from
`native/windows/rust/src/profiles.rs` to the adjacent `KeyrillTsf.dll` through the process
activation context. The smoke checks the adjacent DLL identity and requires activation to fail
when the DLL is absent. The direct factory check runs separately.

The manifest changes activation inside the proof host. Live TSF testing uses the registered
profile and the source selected by the maintainer. Build and controlled checks leave registration,
enablement, and source selection to the maintainer.

The controlled test executable is `target/windows-rust-check/tests/KeyrillProofEditor.exe`.
Its independent Rust raw COM providers drive `KeyrillTsfTest.dll` through the private test API
version 3 and the actual `ITfEditSession::DoEditSession` implementation. Complete literal UTF-16
documents, caret, ownership, and disposition supply the oracles. Client IDs and cookies belong
to the doubles. `native/windows/proof/scenarios.json` maps all 25 former CTest entries and 111
scenario families to Rust tests or an explicit retirement reason. The runner checks the mapping
against the tests it actually executes. The migration validation runs passed all 172 controlled
tests, 31 production runtime tests, 32 test-API runtime tests, 40 proof-library tests, three ABI
tests, and three actual-DLL smoke checks. All required baseline successors executed.

The first maintainer-run Rust registration exposed unterminated description storage, producing
extra characters in the persisted profile names. Commit `2e5fafb` restores terminated UTF-16
storage with the original logical count. Safe owned-buffer and independent counted COM callback
regressions pass. Maintainer re-registration passed fresh-process EN/RU profile-name and
enablement readback.

The runtime uses raw SDK COM vtables and exact HRESULT checks, with typed leases owning each
retained host reference. Rust panics are contained at callbacks. Foreign COM implementations
must obey the standard HRESULT and no-throw contract. The former C++ exception-recovery layer
and its throw-injection schedules are retired. Public COM diagnostic and wire versions retain
their contracts. The private test API is version 3. UIA remains limited to tests and diagnostics.

## Bounded-only field admission

The runtime admits editing only in the compiled proof-editor contract and the exact-version
Notepad main-field contract below. Generic TSF native identity and `IS_DEFAULT` or `IS_TEXT`
do not establish semantic field authority. Browser widgets, Word, terminal wrappers and unknown
Notepad versions pass input without character reads, ACP access or composition effects.
Application settings cannot grant authority. Browser correction remains unimplemented.

Admitted bounded fields still require the current top context, writable status, disabling
compartments, fresh native identity and range-specific scope evidence. Status and compartment
subscriptions revoke pending operations. Scope is the last application query before each
operation, with its native interfaces retained through the final field check and operation.
The host coherence assumptions and browser counterexample are recorded in
[Windows admission stability](windows-admission-contract.md).

Controlled schedules separate generic refusal through normal advised key entry from admitted
bounded fields' late metadata, reference release, context and unknown-effect schedules. Every
expected injected boundary must be reached. This containment does not complete the Windows
application matrix. Installed denied-browser and working bounded-Notepad checks remain separate
maintainer-run gates. Historical observations retain their original artifact identities.

## Bounded Notepad admission and observation

The runtime recognizes packaged x64 Notepad **11.2607.14.0** and **11.2609.11.0**
for the bounded main-document path through the owned-composition executor. Its package family
`Microsoft.WindowsNotepad_8wekyb3d8bbwe`, package name `Microsoft.WindowsNotepad`, architecture,
executable basename and exact version are captured at service activation. Other Notepad versions
remain unadmitted even with an explicit ordinary scope.

A live pre-change run on 11.2609.11.0 confirmed the expected native hierarchy, writable UIA
binding, and missing input-scope result `E_FAIL`. The full public word passed through unchanged,
with zero ACP access or write attempts. The updated exact-version rule has independent
classifier, native-field, scope, refusal, and expired-callback tests for both versions.
The current Rust DLL passed the fixture-policy document checks recorded in
[Rust Notepad verification](#rust-notepad-verification).

Package identity alone grants no editing authority. Before selection ACP or character access,
the edit session validates the canonical TSF top context, active view, exact current-thread
native focus, and freshly captured native field facts. The field must be `RichEditD2DPT`,
control ID zero, on the current process and TSF thread. Its immediate parent must be
`NotepadTextBox`, control ID zero, on that same process and thread. The next parent must be
`Notepad`, control ID zero, in that process and equal both the actual foreground window and
the field's `GA_ROOT`. The root may belong to a different thread. All three nodes must have
complete metadata and be enabled. The field requires `WS_CHILD` and `ES_MULTILINE` and refuses
`ES_PASSWORD`, `ES_READONLY` and `WS_DISABLED`. Dynamic scrollbar bits do not establish identity.
Both ordinary-scope admission and the unavailable-scope exception require this exact hierarchy
and one of these versions. Other Notepad fields and versions remain unadmitted.

Writable context status and all three disabled compartments precede raw selection acquisition.
`GetStatus` requires exactly `S_OK`. A disabling compartment permits `S_OK` with `VT_EMPTY`
or `VT_I4` zero, or the legitimate unset signature `S_FALSE` with `VT_EMPTY`. Other results,
nonempty `S_FALSE` values, malformed values, and interface failures refuse admission.
The raw range must belong to that same context and have valid selection flags. The scope query
then requires exact `S_OK` results, a present property and interface, and exactly one `IS_DEFAULT`
or `IS_TEXT`. The only alternative, `NotepadUnavailableValue`, requires the freshly matched
Notepad field, `GetAppProperty` returning exactly `S_OK` with a property, and `GetValue` returning
exactly `E_FAIL` with its initialized output still `VT_EMPTY`. The compiled main-field contract
supplies ordinary-field authority; the error is not interpreted as a default scope. Other
failures, failed nonempty values, private scopes, multiple scopes and malformed results refuse
before ACP, characters or composition. Proof controls continue to require explicit ordinary scope.

The same private acquisition runs initially and after a finish that requests replan. It checks
generation and the exact native field again after scope callbacks, then obtains ACP. Field
identity lives only in that edit session. A shared private range guard checks fresh native field
identity, canonical range context, writable status, all three compartments, and scope before
each character read, composition start, text replacement, selection update, and composition end.
Callback-capable preparation precedes the guard. The guard rechecks exact field identity and
generation after policy callbacks. These metadata queries cannot make document authority atomic.
Owned range, text, caret, returned range, and acknowledgement checks remain in the single executor.
There is no UIA runtime dependency, admission override, second editor or new editing transport.

Controlled native facts and COM schedules establish these branches, including zero ACP/text/
composition/write before failed admission. Separate live runs loaded the reviewed DLL in fresh
unmodified Notepad processes and checked owned operations against independent full-document
and end-caret UIA predicates. Those runs passed the bounded behavior checks below with zero
Unknown effects. That historical unchanged FINISH performed one exact owned-range replacement
before ending the composition and preserved the original full selection. Actual checks include
whole-document selection and native Undo. Controlled tests additionally cover clone gravity, noncollapsed
selection movement by either native effect, and active-end style. The complete selection-style
matrix has controlled coverage. Mapping a separately requested saved-text focus-loss commit
remains a live gate.
Focus and termination notifications grant no new write authority or saved-text writeback.

### Notepad behavior acceptance

The following checks used the packaged x64 version above and the same reviewed production
service. Generated-key runs used fixture or model policy with public synthetic dictionary files.
Each named public-document predicate checked exact UTF-16 text and a collapsed end caret.
The source, prepared model, dictionary readiness, owned serial, lifecycle, and write outcomes
were observed separately through the content-free service publisher.

| Check | Observed result |
|---|---|
| EN fixture correction and rapid continuation | `привет x`, then native Undo leaves `привет ` |
| EN model input `cgbcjr ` | `список ` with both dictionary sources ready |
| RU model input `цщкдв ` | `world `, then reversal restores `цщкдв ` in the same owned composition |
| Manual conversion and reversal | Exact whole-word results, retained ownership, and one verified write per command |
| Provisional punctuation and PC ё | `[jhjij` converts to `хорошо` and PC backtick input converts to `ёлка` |
| Vocabulary reload during a retained receipt | Reversal remains available in the same owned composition |
| Protected word and forbidden target | `cgbcjr ` stays literal under the corresponding public vocabulary rules |
| Oversized vocabulary reload | The last good protection remains active; restoring empty files restores correction |
| Text surrounding an owned word | Conversion and reversal preserve the whole prefix and suffix. End leaves `left привет right` with its end caret |
| Find and tab changes | Public Find input passes through; returning to the document preserves its exact text and end caret |

Physical runs separately passed manual conversion and boundary reversal while focus stayed
in Notepad. The held-Z run made one verified restoration followed by two acknowledged no-write
repetitions. Ownership survived the observed Alt-then-Ctrl release callbacks. The restored
`ghbdtn ` and its end caret passed both before release and after the later focus transition.
A separate physical run confirmed that switching to chat expires the owned receipt before Z.
An earlier physical attempt also ended its composition before command delivery; its initiating
focus action was unrecorded.

Both modifier release orders, denial, reentry, unknown effects, and lifecycle cancellation have
controlled native coverage. Bare Alt opened normal Notepad keytips in separate generated-key
checks before and after successful commands. The full physical modifier matrix, owned source
changes, owned close, and final callback drainage retain their separate coverage limits.
The controlled-editor lifecycle evidence remains recorded in its own sections.

For an unadmitted Notepad version, after the existing source and loaded-layout checks, the first
key callback in a native
generation requests a separate `TF_ES_SYNC | TF_ES_READ` metadata session. It passes the key.
The session checks the focused document's exact context identity and reports the TSF view's
relationship to native focus without weakening the editing predicate. Known password, disabled
and read-only controls stop before selection. It checks disabling compartments, obtains a raw
selection range and queries the application input scope. A completed ordinary-scope observation
reports `Unsupported` with `acp=0`. Missing, ambiguous and non-text scopes retain their precise
refusal reasons. The observer never queries ACP or a boundary, reads document characters,
plans an engine event, or starts or changes a composition. Unknown hosts
receive no additional observation session. Request result, session result and callback completion
are distinct. Requests expire on return and never retry asynchronously. Lifecycle invalidation
expires cached metadata. Reentrant key callbacks abort observation through the existing generation.

Use only a fresh public test document. These commands take an explicit process ID:

```powershell
KeyrillHostInspect.exe build
KeyrillHostInspect.exe host TARGET_PID
KeyrillHostInspect.exe service TARGET_PID --arm-seconds 120
KeyrillHostInspect.exe service TARGET_PID
KeyrillHostInspect.exe service TARGET_PID --arm-seconds 120 --expect-server-build REVIEWED_SERVER_SHA256
KeyrillHostInspect.exe field TARGET_PID
KeyrillHostInspect.exe public-case TARGET_PID empty
KeyrillHostInspect.exe public-case TARGET_PID probe-g
KeyrillHostInspect.exe public-case TARGET_PID fixture-en-boundary
KeyrillHostInspect.exe public-case TARGET_PID fixture-en-corrected
```

`host` reports package version and focused native window class without reading window titles or
document text. `service --arm-seconds` creates an ephemeral page for each captured live thread
and holds the pages for the specified period, capped at one hour. Arm before the maintainer
activates the source. A newly created thread needs explicit rearming and source reactivation.
The thread snapshot does not retain thread lifetime. The inspector omits only a signaled,
exited thread or an `OpenThread` failure with `ERROR_INVALID_PARAMETER`. It retains live thread
handles, requires at least one, and revalidates the held process before arming. Access denial,
owner mismatch and unexpected API errors stop capture. Content-free failures report the exact
TID, owner, Win32 error and thread/process wait results alongside the target PID and creation time.
There is no automatic snapshot refresh or retry.
The ordinary `service` command opens existing pages with read access only. A missing, busy,
stale, incompatible or inaccessible observation does not establish service inactivity.
By default, `build_match=1` compares the published `build` digest with this inspector's digest.
An updated observer can inspect a separately reviewed installed server with the service-only
`--expect-server-build` option. It requires exactly 64 hexadecimal characters, accepts either
letter case and compares canonical 32-bit words. The service header prints `observer_native_build`,
`expected_service_build` and whether that expectation was explicit. Each available publication
prints its actual `build` digest. `build_match` and successful availability then require that
actual digest to equal the expected server digest. The observer's own build command remains
unchanged, and the option does not bypass protocol, identity or payload validation.
Check the actual loaded DLL's identity separately. Registry agreement is insufficient.

`field` captures the actual foreground focus, its owning PID and TID, process and retained
foreground and focused thread creation times, root and up to eight parents. The foreground
window and focused field may belong to different threads in the same process. Both retained
threads must remain live with the same creation times, and both GUI thread queries must report
the exact focused field under the actual foreground root. Each bounded native window record contains
its class, control ID, styles and enabled status. Missing or truncated facts remain explicit.
It requires the focused UIA element's PID and native HWND to match that exact focused window,
and `CompareElements` to match `ElementFromHandle` for the window. A same-process ancestor or
windowless UIA element does not establish this binding. It reports the UIA control type,
password/enabled evidence and independent ValuePattern and TextPattern read-only observations.
It never fetches names, values, titles, document characters or selection.

`public-case` checks one compiled public document and reports only the case, verdict and refusal.
The cases are `empty` for zero UTF-16 units, `probe-g` for `g`, `fixture-en-boundary` for
`ghbdtn` followed by one Space, and `fixture-en-corrected` for `привет` followed by one Space.
The literal baseline retains its original meaning. The corrected case does not accept it.
The additional compiled cases check these exact documents. A visible `␠` denotes one Space.

| Case | Exact document |
|---|---|
| `fixture-en-word` | `ghbdtn` |
| `fixture-ru-word` | `привет` |
| `fixture-continuation` | `привет␠x` |
| `fixture-two-words` | `привет␠привет␠` |
| `manual-provisional` | `хорошо` |
| `manual-provisional-literal` | `[jhjij` |
| `manual-pc` | `ёлка` |
| `manual-pc-literal` | `` `krf␠ `` |
| `model-en` | `список␠` |
| `model-en-literal` | `cgbcjr␠` |
| `model-ru` | `world␠` |
| `model-ru-literal` | `цщкдв␠` |
| `prefix-suffix` | `left␠привет␠right` |

All require exactly one collapsed selection at the document end. The focused UIA control must
report Document type, positive enabled and non-password evidence, exact native binding and
TextPattern. Native password, read-only or disabled evidence vetoes the check. At least one
read-only channel must successfully return exact Boolean false; the other may be unavailable or
unsupported. A true, mixed or malformed observation refuses the check. Native styles never
establish positive writability.

The checker reads at most the compiled expected length plus one UTF-16 unit and compares exact
code units and length, including embedded NUL and terminal carriage return. It does not normalize
provider text. The check requires an actual returned BSTR, including an allocated empty BSTR for
`empty`. An `S_OK` result with null text returns `TextUnavailable` under this tool's contract.
It revalidates retained process/thread identity, actual native focus and bounded
native facts around provider calls, and fresh UIA identity and safety evidence before character
access, after it and before success. Observed drift refuses the check. These calls do not lock
the document atomically. Neither command changes values, selection or focus, performs editing,
or grants runtime admission. The helper module links only into the inspector and its tests.

No inspector command registers the DLL, selects an input source, changes policy, requests text
from the service or widens admission. The maintainer registers the reviewed exact DLL path with
the existing registration tool and selects the source. First confirm a fresh activation,
matching source digest and actual loaded DLL. New host versions require their own owned-composition
and physical application acceptance under the bounded contract above.

### Observation protocol

Protocol v4 uses a local-session page named by protocol, PID, process creation time and TID.
Arming refuses an existing object and gives mapping read/write rights only to the current logon
SID through an explicit protected DACL. The publisher opens only an already armed page.
An atomic nonblocking claim allows one service instance per thread page. A page-resident,
nonwrapping allocator distinguishes instances across DLL unload and reload. A colliding
instance cannot publish or release another instance's claim. Exhausted allocation or publication
sequence requires handles to drain and explicit rearming.

Every wire field is an aligned 32-bit Windows integer. Payload encoding enumerates fixed-width
fields and reserved zeros explicitly. It copies no native structs, booleans, padding, keys,
text, titles, paths, dictionary entries or absolute text ranges. The snapshot includes metadata
refusal, activation and callback counters, the existing V2 lifecycle journal and owned serial,
runtime policy/readiness and a deterministic SHA-256 digest of the Windows native sources.
Decoding validates enums and the existing service and lifecycle invariants.

The protocol-4 payload has 512 DWORDs in the same 4096-byte page. Its first 177 words keep the
existing field order. Three observation header words hold sequence, saturation, and count.
Thirty-two ten-word rows follow. Word 500 carries the client PE machine, followed by eleven
required-zero words. The versioned mapping name isolates this layout from frozen protocols
3 and 2 and protocol 1, whose 192-word payload
has fifteen required-zero words after its 177 fields. COM V1 remains 220 bytes. COM V2 and
the status codecs retain their existing contracts.

Each observation row contains sequence, lifecycle cursor, owned-present, owned serial, event
kind, command route, command intent, command arrival ID, modifier knowledge or release class,
and release delivery. Unused rows are ten zero words. The codec in
`native/windows/rust/src/observation.rs` owns all numeric tags. Command kinds 1 through 6 mean arrival, admission refusal, exact-modifier
refusal, stale-dispatch refusal, executor dispatch, and callback exception. Kind 7 means
release. Route 1 means ordinary and 2 means preserved. Intent 1 means convert and 2 means
reverse. Command rows have zero release delivery. Release rows have zero route, intent, and
command ID in protocol 2. Protocols 3 and 4 use release row word 5 for the Boolean disposition and
word 6 for authority validation, with 0 unchecked, 1 matched and 2 refused. Word 7 stays zero.
Validation covers the receiving canonical context, current source, focus and native field metadata.
Release class 1 through 6 means left Ctrl, right Ctrl, left Alt, right Alt,
unresolved Ctrl, and unresolved Alt. Delivery 1 means TEST and 2 means ACTUAL.

Modifier knowledge zero means unknown. Known knowledge is `0x100 | mask`, with bits 0 through
5 holding left Ctrl, left Alt, right Ctrl, right Alt, any Shift, and any Windows. The projection
uses only the keyboard state already acquired by admission. Any Shift combines generic,
left, and right Shift. Any Windows combines left and right Windows. Caps Lock, text, raw keys,
native pointers, and raw flags have no observation fields. Generic Ctrl and Alt releases use
their extended bit to choose left or right. Explicit sided arguments whose extended bit
contradicts that side produce the matching unresolved class. TEST and ACTUAL release methods
record independently. A matching witnessed command can predict or consume its Alt release as
described below. Other releases pass.

Kind 8 records the Alt observer, with word 8 equal to 1 for arrival, 2 for admission and 3 for
refusal. Its other four body words are zero. Kind 9 records executor completion, using the
command route, intent and arrival ID in words 5 through 7. Word 8 is the Boolean witness
captured at dispatch. Word 9 is 1 for refusal, 2 for unknown effect, 3 for an acknowledged
no-write reservation and 4 for a verified retained manual MARK. These records never grant
runtime authority. A completion is separate from release permission, which can be cancelled
by reentry even when the command's verified write succeeds. All records keep the fixed row
capacity. The decoder rejects invalid tags, malformed Boolean values, contradictory release
validation, duplicate completion and inconsistent retained command identity.

A preserved arrival has unknown modifiers. It permits admission refusal, exact-modifier
refusal, stale dispatch, executor dispatch, or an actual outer callback exception. An ordinary
arrival starts after translation recognizes manual or reverse and has known exact modifiers.
It permits stale dispatch, executor dispatch, or an actual outer callback exception. A dispatch
may be followed by one actual outer exception. Other terminal progress has no continuation.
Known facts stay identical for a retained command. The decoder rejects inconsistent grammar
and commits its output only after every field and reserved word passes validation.

The inspector prints `incomplete=1` for an arrival with no retained continuation and
`arrival_evicted=1` for a continuation whose arrival left the window. `ExecutorDispatch`
records entry into the existing executor. The separate completion record and attempted,
verified, refused, and unknown counters describe
the editing outcome. Each row samples owned-present separately from serial, including the
existing allocation-saturation case where ownership has serial zero.

For `lifecycle_after=c`, lifecycle event c precedes the observation, and event c+1 follows it.
Thus a release with cursor below a retained termination sequence precedes that termination.
An equal or larger cursor follows it. This ordering remains usable despite older evictions
while the anchor is unsaturated. Both journals expose their own saturation. The final
representable observation sequence is appended, marks saturation, and freezes the journal.
Inactive tickets leave runtime handling intact. Observations survive activation changes
within the same Service instance.

`history_evicted=1` reports an evicted observation prefix. `complete_interval=1` requires
consecutive available samples from the same mapping identity, publisher instance, native
build, and activation, coherent overlapping rows in both histories, full intervening coverage,
and unsaturated histories and counters. A missing baseline or `complete_interval=0` leaves
absence unknown. Old evictions can coexist with complete coverage of a later bounded interval.
The inspector prints snapshots with mismatched builds for diagnosis and reports their
`build_match=0` status.

Controlled mapped-Service schedules cover both command routes, pre-acquisition unknown and
post-acquisition known refusals, exact-modifier refusal, stale dispatch, executor refusal,
independent release identities, nested commands, and releases around exact termination.
Pure schedules cover all 512 high-bit source combinations, strict wire rejection, overflow,
saturation, and frozen protocol-1 header rejection. Direct `GetKeyboardState` failure and
its call count lack an injected seam and receive source review. The Notepad acceptance above
records physical command retention separately from these controlled schedules.

Windows provides [atomic aligned 32-bit access](https://learn.microsoft.com/en-us/windows/win32/sync/interlocked-variable-access).
Writers use [interlocked stores with a full memory barrier](https://learn.microsoft.com/en-us/windows/win32/api/winnt/nf-winnt-interlockedexchange)
around an odd and even sequence. Readers use [MSVC `/volatile:ms` loads](https://learn.microsoft.com/en-us/cpp/build/reference/volatile-volatile-keyword-interpretation)
and full Windows memory barriers, then
recheck sequence, claim and immutable identity. Readers never issue an interlocked read-modify-write
against read-only pages. Three bounded attempts return Busy if coherence cannot be established.
The production tests run a separate process with an actual `PAGE_READONLY` view, plus collision,
reopen, malformed identity, exhausted identity, incomplete publication and concurrent writer schedules.
This is diagnostic evidence, not authentication against a malicious process in the same logon.

Publication happens after the outermost service callback. It copies existing scalar observations
without waiting, allocating, performing I/O or querying COM or the host. Missing diagnostics keep
the same admission and key disposition. Controlled tests compare the exact document, caret,
service snapshot and lifecycle with publication enabled and disabled. Live Notepad runs separately
observed packaged-process mapping access and ordinary service routing.

## Manual conversion and reversal

In an admitted owned word, left Ctrl + left Alt + physical K asks the shared Rust session for
manual conversion. Left Ctrl + left Alt + physical Z reverses the current owned receipt.
Caps Lock is allowed. Shift, either Windows key, right Ctrl, and right Alt reject these chords.
AltGr does not invoke a command. The shortcuts are local input-method keys, not global hotkeys.
Backspace edits owned text only without Shift. Escape restores and finishes an owned receipt,
or finishes the word unchanged and passes through when no receipt exists.

Ctrl, Alt, Shift, and Windows modifier downs pass after the existing profile and loaded-layout
checks, without requesting an edit session or finishing the mark. Modifier keyups pass except
for one validated left Alt release belonging to a witnessed successful command.
An unmatched completed shortcut finishes a still-owned word and passes the original key.
The translator accepts bounded printable UTF-16 from the verified EN or RU substitute layout
using `ToUnicodeEx` without changing its keyboard state. Rust decides whether punctuation is
provisional word text or a finish request. PC backtick can therefore start `ёлка` as `` `krf ``,
and `[jhjij` can convert to `хорошо`. Coalesced repeats remain finish-and-pass requests.

Ordinary Notepad uses native command delivery.
Activation registers K and Z with `ITfKeystrokeMgr::PreserveKey`, using distinct stable GUIDs
and exactly `TF_MOD_LCONTROL | TF_MOD_LALT` on keydown. The same binding table registers an
Alt-press observer with `VK_MENU` and `TF_MOD_IGNORE_ALL_MODIFIER`. All registrations must return `S_OK`
before the service becomes active. Failed or duplicate registrations grant no cleanup ownership.
Cancellation accounts for each late success before rollback. All acquired pairs and mandatory
sinks receive one independent cleanup attempt, even when another fails. Unknown acquisition
or cleanup effects fault that service instance. `CONNECT_E_NOCONNECTION` from `UnpreserveKey`
reports an anomaly but establishes absence, so it alone does not fault the instance.
New activation waits until rollback, teardown, captured COM releases, and the whole prior
`ActivateEx` call finish. Optional diagnostics and model startup retain their existing failure policy.

`OnPreservedKey` shares ordinary-key admission for the exact active profile, dedicated substitute
layout, complete loaded-layout set, and real thread keyboard state. Unknown GUIDs pass before
these queries. Known commands require actual left Ctrl and left Alt, with no Shift, right Ctrl,
right Alt, or Windows key. Caps Lock remains allowed. The callback dispatches a zero-text manual
or reverse event through the same executor. It supplies no scan code or repeat count, and the
adapter invents neither. Each repeated manual callback can convert the owned text again.
Repeated reversal cannot reuse an expired receipt. The bounded Notepad physical run above covers
held-Z delivery and its no-write repetitions.
The ordinary `KeyEntered` counter continues to count only ordinary-key callbacks.

The observer always returns FALSE. It revokes the preceding Alt cycle before native queries
and can witness a new cycle only with a known left Alt press, active source, canonical receiving
context, focused native field and current owned composition. It reads no text. A command
without a witness still executes normally and cannot grant release permission.

Only a verified retained manual MARK with successful Rust acknowledgement, completed synchronous
request and session results, and unchanged authority can arm permission on that owned composition.
An acknowledged no-write repeated reversal preserves only the identical permission captured
at dispatch entry. Refusal, unknown effects, incomplete requests and cancellation revoke it.
Callback-free private allocation identities bind the witness and permission. Diagnostic serials
and counters supply no authority. Nested commands, observers and releases cancel an outer
decision, including a release during execution or temporary COM cleanup.

`OnTestKeyUp` predicts without spending. `OnKeyUp` validates independently and can consume one
unambiguous left Alt release, including direct actual delivery without a test. Every actual
Alt release closes its cycle. Ctrl and command-letter releases pass, and either modifier
release order is supported. Source, focus, context, generation, ownership, termination,
ordinary new input and fresh Alt presses expire permission. Release validation reads native
metadata only, with no text query, composition readback, edit session, Rust plan or write.

This path assumes a bounded host contract for packaged x64 Notepad 11.2607.14.0.
Within unchanged source, context, generation and owned composition, either the witnessed
cycle's actual release arrives, or a fresh observed Alt press or invalidation revokes it
before a later cycle's release arrives. Missing both the original release and a later observer
is indistinguishable and excluded. Controlled tests document that limit. An actual occurrence
rejects this route. Live runs observed the observer, command completion, retained ownership,
and matching Alt release. Separate generated-key checks preserved bare-Alt menu behavior.
The combined observer and release intervention does not isolate each mechanism's contribution.

Controlled registration and callback tests do not prove delivery in ordinary Notepad.
When changing this route or admitting another host, run live acceptance with the reviewed DLL,
a fresh owned `ghbdtn`, and the original
left Ctrl + left Alt + K chord with no intervening focus or source action. Delivery must precede
composition termination and produce one verified write, exact `привет` without Space, the end
caret, and zero Unknown effects. Check reversal and that host's other application behavior separately.
The same owned composition must survive command release with no menu transition. Separate
bare Alt must retain normal menu behavior after success, refusal, cancellation and focus or
source changes. Record physical press and release order, held commands, and physical admission
separately. The bounded results above cover the observed sequences. Further modifier combinations
and host versions need their own checks. Absent, late, or unusable delivery rejects the tested route.
A callback after termination cannot recover the committed word or establish new editing authority.

Commands use the same owned composition, exact range/text/caret checks, readback, and Rust
acknowledgement as fixture correction. A successful acknowledged command can consume its key
without writing, including manual conversion with no word or reversal with no receipt.
Admission failure, denied edit sessions, and refusal before a write pass the original key.
An attempted unknown mark consumes without replay. An unknown finish passes an ordinary next
key once and retains a reserved command's consumed disposition. Unknown effects never trigger
a repair or another transport.

Controlled tests drive `OnKeyDown` and `OnKeyUp` through complete modifier sequences using
thread-local `SetKeyboardState`. They check whole A and B documents, prefix/suffix preservation,
the caret, composition identity, command expiry, denied and failed requests, extra modifiers,
Shift transitions, exclusions, reentry, and unknown effects. Actual `ToUnicodeEx` checks cover
PC OEM keys with Shift and Caps Lock in each already loaded supported layout. These are
controlled observations, not physical keyboard or live TSF routing evidence. Separate automated
real-editor runs passed the manual commands and PC printable-key cases described below.

## Persisted preferences store

The Rust `settings` API stores one complete per-user `REG_BINARY` value named `Preferences`
under `HKCU\Software\Keyrill`, using the explicit 64-bit registry view for both client bitnesses.
It contains automatic inference, source-switching preference, both dictionary choices, and
restrictive application overrides. Shipping defaults disable automatic inference, enable
source switching, and select the default dictionary locations.

Loads have no initialization write. Save validates the entire candidate, compares its opaque
revision under a user-SID-bound cross-session mutex, writes one complete value, and requires
exact fresh readback. Every save creates a fresh nonce. An old saved revision conflicts after
deletion or recreation. A missing revision compares with current absence. Invalid, partial,
future-schema, or inaccessible values leave a worker reader's last valid snapshot intact.

The mutex requires the current user's exact owner and private DACL plus an explicit medium
mandatory-integrity label. Native tests verify the actual label and reject a preexisting
low-integrity object. Actual cooperation between elevated and normal writers and separate logon
sessions still needs deployment validation. The native settings editor and runtime use this store.
The existing vocabulary worker prepares a complete settings and dictionary publication. Invalid
updates retain the last valid complete configuration. Native source switching remains in progress.

Application overrides match canonical executable paths or package families. The most restrictive
matching override wins. VS Code, VS Code Insiders, Word, and Windows Terminal default to Manual.
An explicit Automatic override follows the global automatic-correction switch. Manual retains
owned words for K and Z with inference disabled. Off passes input with zero character inspection
or composition.

Compatible updates adopt at a natural owned-word finish. A newly restrictive policy blocks new
work and requests a bounded unchanged finish through the same executor. Denied retirement keeps
the complete update pending. Focus, source, and exact termination preserve that pending update.
An unknown effect keeps editing stopped while configuration maintenance continues.

### Native settings editor

Run `KeyrillSettings.exe` to edit the automatic-correction and source-switch preferences, both
dictionary choices, and application overrides. Save validates the complete draft and requires
verified readback. Conflicting saves retain the draft and require Reload. Select a row to change
its identity or policy. Renaming to another existing identity is rejected. New starts a separate row.

The standard TSF Properties operation supports direct class-factory `ITfFnConfigure` and
`GetFunction(GUID_NULL, IID_ITfFnConfigure)` on the same service object. Its inherited
`ITfFunction` returns a caller-owned display name. Show requires an exact product language/profile
pair, validates an optional owner, launches the adjacent same-architecture Settings executable,
and waits for its exit with COM-aware pumping. Settings owns its standalone message loop.

Show requires a complete unsigned package in its exact manifest-digest directory, the current
production build identity, declared DLL and Settings hashes, and matching PE architecture.
Retained ordinary file and directory handles prevent launch-path replacement through process
creation. An elevated caller additionally requires administrator/System ownership and ACLs that
withhold writes from other principals for the package root, architecture and launch inputs.
Manifest hashes identify bytes in this unsigned package. They provide no publisher authentication
and do not identify the already loaded in-memory DLL image. Unpackaged development output reports
configuration unavailable. Running Settings directly remains available.

One Settings-owned worker prepares the embedded model and dictionaries off the UI thread using
the last verified loaded or saved complete snapshot. Draft edits leave that binding in place.
Verified Save or Reload restarts the check. A conflict retains and identifies the previous report.
The UI polls immutable reports and shows pending, last-good and error states. Live host adoption
is unavailable because this window has no bound host. Existing native inspectors retain their
independent runtime-evidence role.

`KeyrillSettings.exe --preview` uses synthetic in-memory defaults and disables local resource
checks. Ordinary isolated UI fixtures also disable preflight. Explicit integration tests prepare
isolated temporary dictionaries. Native tests cover SDK pointer/reference/thread contracts,
profile refusal, retained launch paths, reparse points, privileged peer refusal, actual preview
child lifetime and modal pumping, preserved host quit requests, complete Save/Reload/Close,
conflicts, stale reports and resource-error retention. Preview-helper success and ordinary
packaged SDK Show success are separate assertions. Installed-candidate interaction and physical
application acceptance remain maintainer gates.

## Public Terminal child

### Input scope in Windows Terminal 1.25.2733.0

The Windows Terminal 1.25.2733.0 source shows an admission limitation. Its TSF
context owner returns `VT_EMPTY` for input scope unless `DefaultInputScope` is
`AlphanumericHalfWidth`. In that case it supplies `IS_ALPHANUMERIC_HALFWIDTH`. Neither
value identifies an ordinary, echo-enabled child input buffer. Candidate `f3831bbe` also
requires a supported field before inspecting characters or starting a composition.
Terminal has no such field in this candidate.
[Terminal control settings](https://github.com/microsoft/terminal/blob/v1.25.2733.0/src/cascadia/TerminalControl/TermControl.cpp#L985-L986),
[Terminal TSF attribute and scope](https://github.com/microsoft/terminal/blob/v1.25.2733.0/src/tsf/Implementation.cpp#L386-L397),
[scope value](https://github.com/microsoft/terminal/blob/v1.25.2733.0/src/tsf/Implementation.cpp#L544-L556).

The TSF attribute depends on the Terminal setting. `ENABLE_ECHO_INPUT` is a mode on the
console input buffer. The published scope values supply no evidence that the child has
enabled echo. Terminal correction and manual conversion remain unqualified. The paired
public console and no-echo runs below test the installed behavior.
[Console input mode](https://github.com/microsoft/terminal/blob/v1.25.2733.0/src/host/getset.cpp#L334-L405).

With candidate `f3831bbe`, the maintainer entered the public original-en sequence in both
modes. Each child received the expected nine bytes and verified restoration of the original
console mode and input code page. The observer armed against the Terminal window's process
before source selection. It obtained no Keyrill service publication during these runs.
A subsequent module check found Terminal Control and MSCTF in that process. It found no
loaded Keyrill DLL at that time. The actual key callback, refusal stage and no-echo exclusion
inside Keyrill remain unverified.

Terminal Control creates the TSF handle in the UI process. Its thread manager requests
deferred TIP activation. Windows can activate a service asynchronously when the calling
thread gains focus. These flags alone do not establish whether Keyrill activated during
input. A repeat needs the focused Terminal thread and a matching live Keyrill publication
before the child reads its test line.
[Terminal Control](https://github.com/microsoft/terminal/blob/v1.25.2733.0/src/cascadia/TerminalControl/TermControl.cpp#L80-L86),
[Terminal TSF initialization](https://github.com/microsoft/terminal/blob/v1.25.2733.0/src/tsf/Implementation.cpp#L68-L91),
[ActivateEx](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfthreadmgrex-activateex).

### Check child input and echo mode

`KeyrillTerminalProbe.exe` compares a bounded public input line with fixed UTF-8 bytes,
including its final CRLF. It prints the scenario, byte count, and comparison result. Observed
input remains in memory. The ordinary console modes set and verify the input code page and
line/echo flags, then restore and verify the original settings before reporting completion.

In a maintainer-launched Windows Terminal, run one selected public case.

```powershell
& "$bin\KeyrillTerminalProbe.exe" console converted-en
```

With the input source selected by the maintainer, physically type `ghbdtn`, one Space, and
Enter. `converted-en` expects `привет ` followed by CRLF. `converted-ru` expects `world `.
`original-en` and `original-ru` expect `ghbdtn ` and `цщкдв ` respectively, for reversal or
literal-input checks. `continuation` expects `привет x` followed by CRLF.

For the public no-echo exclusion case, use `no-echo original-en` or `no-echo original-ru` and
type only that public original line. The native observer must independently establish zero
character reads, composition starts, and character writes. The child's mode observation cannot
grant editing authority to the input method.

`verify-pipe` exercises the byte comparator with explicitly supplied synthetic standard input.
Automated positive and negative pipe checks cover all five expected lines, extra characters,
wrong conversion, and wrong line endings. These checks leave Terminal application acceptance
pending. Actual Terminal source activation, composition, byte ordering, concurrent output,
shortcuts, cancellation, and no-echo isolation still require the dedicated physical run.

## Acceptance inventory and local results

`native/windows/proof/acceptance.json` defines the approved application/control matrix and its
controlled, generated-input, and physical-input obligations. Both native architecture tracks
are required. Installed-application metadata supplies each actual application architecture.
Historical C++ results retain their separate source and artifact identities.

```powershell
cargo xtask windows acceptance inventory
cargo xtask windows acceptance results .audit/windows-final-results.json
```

The first command validates and prints the expanded matrix. Full check and sanitizer commands
also validate its structure. The second checks a complete local ledger against the current
source and both production identities, fresh publication, armed/observed client identity,
required oracle records, and hashes of existing local evidence files. One session artifact can
support several obligations. Pending, missing, stale, duplicate, or mismatched records fail.

Ledger validation establishes record consistency. Independent artifact review and physical
application observations establish the behavior claimed by those records. Keep completed ledgers
and real desktop evidence under uncommitted `.audit/`.

## Shared model and personal vocabulary

An absent `KEYRILL_WINDOWS_POLICY` selects persisted preferences and continuous worker refresh.
Explicit `off`, `fixture`, and `model` values select the existing proof policies at activation.
Invalid explicit configuration retains the service's previous complete configuration. Its first
invalid configuration uses off. Off disables inference while preserving manual conversion and reversal.

The explicit model policy accepts absolute local paths through `KEYRILL_WINDOWS_EN_DICTIONARY`
and `KEYRILL_WINDOWS_RU_DICTIONARY`. Persisted preferences provide both paths in the shipping mode.
Off and fixture policies ignore these unused bindings. Defaults are
`%USERPROFILE%\vimfiles\spell\en.utf-8.add` and `ru.utf-8.add`. A genuinely
absent default initially supplies empty protection. An explicit absent source remains missing,
so the shared model abstains until both sources are ready. A real empty file is ready after
settling. Invalid or disappearing files retain their last good snapshot for that activation.

The acquisition worker reads at most 1 MiB per source. It admits local fixed NTFS volumes,
opens each component relative to an OS volume handle, and rejects reparse points, offline
files, remote paths, and uncertain metadata before reading contents. Final-file replacement
is allowed; changed identity or observed writes discard the candidate. Two equal observations
are a settling heuristic. They cannot prove an editor has finished saving.

Rust parses the existing Vim format on the worker, including skipped invalid lines and flags.
The TSF thread adopts both immutable snapshots outside callbacks. Reloads preserve the current
word and reversal receipt. Model preparation also runs on the worker; activation remains
manual-only until the prepared policy can be enabled while idle. Workers hold a DLL reference
until their callback returns. Deactivation signals cancellation without joining the worker.

The proof editor exposes content-free readiness in control 111. `KeyrillProofInspect PID runtime`
reads requested and effective policy, revision, source states, and numeric errors.
`model-ready` requires the prepared model and both ready or retained sources in an active
service with a known EN or RU mode. Document-and-caret oracles include `manual`,
`manual-reversed`, `manual-provisional`,
`manual-pc`, `model`, `model-ru`, `model-reversed`, `model-ru-reversed`, and `vocabulary`.
`model-ru-reversed` requires exact A=`цщкдв `, empty B, and A's collapsed end caret.
They read only A and B.
These tools provide test evidence; UI Automation grants no editing authority to the adapter.

### Controlled-editor behavior acceptance

Automated key events through the live TSF proof editor passed the following exact UIA checks.
Each check compared all of A, required empty B, and required a collapsed caret at A's end.
The running editor loaded the frozen adjacent DLL whose hash matched the tested build.

| Input or change | Verified result in A |
|---|---|
| EN `ghbdtn`, then left Ctrl + left Alt + K | `привет` without Space |
| Left Ctrl + left Alt + Z on that owned receipt | `ghbdtn` without Space |
| EN `cgbcjr` then Space under model policy | `список ` |
| Settled EN vocabulary reload, then reversal of that receipt | `cgbcjr ` in the same owned composition |
| EN vocabulary protects `cgbcjr`, then a newly typed word | `cgbcjr ` |
| Malformed vocabulary reload, then a newly typed word | `cgbcjr ` with the last good protection retained |
| Restore empty vocabulary, then a newly typed word | `список ` |
| RU vocabulary forbids `список`, then EN `cgbcjr` and Space | `cgbcjr ` |
| EN `[jhjij`, then manual conversion | `хорошо` |
| EN PC `` `krf ``, then manual conversion | `ёлка` |
| RU `цщкдв` then Space under model policy | `world ` |
| Reversal of that RU receipt | `цщкдв ` in the same owned composition |

Both model directions used ready dictionary snapshots and the shared model, with no runtime
errors. Each manual command and reversal added one verified write. Vocabulary publication
preserved the existing mark and receipt without a document write. All recorded cases had zero
unknown edit effects. Tests used only public synthetic vocabulary in private fixture files.

These runs used agent-generated keys. They add real TSF routing and document evidence without
adding physical keyboard evidence. The separate Notepad acceptance above extends admission to
its version-bounded main document. Further application coverage, settings UI, and packaging
remain later work. Controlled worker tests and actual DLL lifetime checks passed under Windows
AddressSanitizer. Pending I/O completion in
those tests was simulated, so they do not establish actual kernel-pending I/O behavior.

## Editing authority

One STA service owns the engine, generation, and optional mark. An edit request retains its
context and completion result. It expires as soon as the synchronous request returns. No
asynchronous editing work, cached cookie, retry, or key replay is allowed. Admission requires both the
native focused HWND and the thread manager's current top context to match the captured target.

Proof admission requires the named proof executable and root window class, writable controls
101 and 102, a known singleton `IS_DEFAULT` or `IS_TEXT` input scope, and non-disabled context and
thread compartments. The separate version-bounded Notepad main-field contract is described
above. Secure-mode activation is refused. Read-only, password, sensitive, uncertain,
foreign-composed, and unknown controls get no text read and no composition. The proof
executable-name restriction is a support policy, not a security boundary or approval for
arbitrary RichEdit hosts.

Input scope comes from the context owner's read-only application property through
[`ITfContext::GetAppProperty`](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfcontext-getappproperty).
Explicit ordinary scope requires `S_OK`, a non-null property, and `S_OK` from
[`ITfReadOnlyProperty::GetValue`](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfreadonlyproperty-getvalue)
with a non-null `ITfInputScope` value. Its
[`GetInputScopes`](https://learn.microsoft.com/en-us/windows/win32/api/inputscope/nf-inputscope-itfinputscope-getinputscopes)
must return `S_OK`, a non-null array, and exactly one `IS_DEFAULT` or `IS_TEXT`. `S_FALSE` from
either property call refuses admission, including a valid-looking value returned with that status.
Missing, private, password, unknown, mixed, and multiple scopes stay pass-through before text
reads or composition starts. Only the compiled Notepad main field may use the exact
`NotepadUnavailableValue` signature described above. Controlled application-property tests do
not establish live RichEdit admission.

The fixture declares `IS_TEXT` for writable A and B. Control 103 retains its read-only style and
default scope, and control 104 retains its password style and password scope. A native metadata
probe on empty stock RichEdit A observed `S_OK` for explicit `IS_TEXT` setup, the application
getter, range value, interface query, and scope retrieval. The value was `VT_UNKNOWN` and the
array contained exactly one `IS_TEXT`, with matching context, range, native focus, and a granted
synchronous read cookie. An explicit one-element `IS_DEFAULT` list still produced an unavailable
value both empty and after public fixture letter `g`, including a fresh read. Default removes the
scope association. A later fresh launch with explicit `IS_TEXT` returned `E_FAIL` from the range
value getter despite `S_OK` from setup and valid context, range, view, and synchronous edit-session
checks. Explicit `IS_TEXT` can supply admissible metadata but does not guarantee its availability.
Missing metadata still refuses admission. Neither observation establishes an owned composition,
verified correction, or physical acceptance.

Both fixture window procedures attempt `IS_DEFAULT` for every existing known field during parent
`WM_DESTROY`, while the children still exist on their owning thread. Partial setup failure uses
the same teardown. A failed reset does not prevent later children from being attempted.
The helper returns the first non-success result and does not retry after destruction. This is an
attempt to remove metadata under the
[`SetInputScope` lifecycle contract](https://learn.microsoft.com/en-us/windows/win32/api/inputscope/nf-inputscope-setinputscope),
not a document or removal oracle. Controlled tests observe actual child validity, thread ownership,
and reset order during normal and partial teardown.

An owned mark retains canonical context and composition identities, exact ACP extent, and
bounded verified UTF-16 text. Each update checks active composition membership, generation,
expected text, returned extent, and caret. It verifies the returned range, composition range,
text, and caret after the one permitted `SetText` call. Each MARK and changed FINISH that reaches
replacement performs one exact owned-range `SetText`. Ordinary unchanged FINISH performs zero
text writes and one `EndComposition`. Its expected and desired UTF-16 text must equal the owned composition's last
verified text. Retirement retains the unchanged replacement path. `EndComposition` removes the
composing property and counts as an attempted effect before the native call, including when
there was no text write. Unchanged finish requires no expected end caret and preserves the
full selection. It captures the selection's exact ACP start, length, and style and refuses
preparation drift without recapturing. After verified range extent, text and composition end,
it compares the actual full selection with that immutable value. If necessary, it reconstructs
a private range clone using checked `ShiftEnd` and `ShiftStart` metadata operations, then calls `SetSelection`
once. The clone grants no character access. Partial shifts or failed verification stop with an
Unknown outcome and no retry. Fresh policy guards the exact owned text range and each selection
effect separately. Changing finish retains its caret checks and verified end-caret update.
Finish checks the retained exact range's text and extent and verifies that no composition remains
after selection callbacks, then verifies the final selection before acknowledging Rust. A replan
reacquires the actual caret and boundary, rather than inheriting the finished mark's boundary.
Ordinary unchanged FINISH followed by a replanned MARK performs one character write on the new
composition. Controlled operation traces attribute effects by request ordinal,
composition-start serial, operation, and exact span.

If scope or writability excludes the field after an attempted character write, the guard refuses
further character reads. The result is Unknown, correction stops, and the executor performs no replan,
replay, repair, or second character write. A pre-effect refusal follows the existing key
disposition rules. Fresh policy checks do not change the owned-composition transport or make
native success a document oracle.

A refusal after creating an empty owned composition but before `SetText` retains the existing
`KR_REFUSED_BEFORE_WRITE` classification. Composition creation is already a native effect.
The service stops for the activation and performs no further character read or editing effect.
It never replays or repairs the input.
It does not resume ownership on later input.

Request HRESULT, session HRESULT, and actual callback completion all affect key disposition.
A refused edit passes the key. An unknown MARK consumes the possibly inserted key. An unknown
FINISH passes an ordinary next key without replanning, but keeps a reserved command consumed.
Unknown effects stop correction for the activation. No guessed cleanup or committed-range
replacement follows. A nested key invalidates the outer request and passes unchanged.

Each key validates the active TSF profile's exact type, service, profile, language, active flag,
and selected mode before querying `ITfInputProcessorProfileSubstituteLayout` on its captured
profile manager. `GetSubstituteKeyboardLayout` must return `S_OK` and a non-null canonical EN
or RU layout for that exact profile. The active profile's `hkl` and `hklSubstitute` fields do not
supply substitute authority. The independently returned layout must be an exact member of a
complete loaded-layout observation bounded to 64 entries, with matching counts before, during,
and after enumeration. No layout is loaded or activated on this path.

Generation checks surround native queries. Local COM references keep the queried objects alive,
and a separate key-admission guard makes nested keys invalidate and pass through before event
handling. The guard releases before `Service::execute`, whose busy guard owns edit-session
reentry. Diagnostic callback depth supplies no editing authority. Controlled tests inject profile
and loaded-layout dependencies per service instance and exercise actual `OnKeyDown`; they do not
establish physical TSF routing or document acceptance.

Translation uses the validated layout with the current Shift and Caps Lock state and
`ToUnicodeEx`'s no-state-change flag. Synthetic TIP handles are not treated as keyboard layouts.
Only the stable EN and RU proof profiles are accepted. An unsupported profile or layout
invalidates authority and passes without a guessed finish. Unmatched nonmodifier shortcuts,
dead keys, and coalesced repeat messages request a verified owned finish before passing through.

Focus loss, source changes, context changes, and teardown invalidate authority. If a mark
exists, these paths stop the activation without writing it back. A termination callback clears
only its exact captured composition identity. Returning A to B to A never restores old editing
authority. The final fixture run observed exact native termination on each mouse focus change
before invalidation, alongside the expected whole documents and caret. This does not admit
other clients or establish Undo behavior. A later RU source-selection run preserved the public
literal and caret, but its mark had already reached FinishVerified before profile change. That
run does not isolate termination caused by the source-change callback.

## Maintainer registration and selection

For physical results with the installed Rust service, see the application results near the
top of this document. The commands below exercise the proof registrar.
General application reliability remains open. Installation, enablement, source selection,
and permission changes belong to the maintainer. The default build and controlled tests never
perform them. The maintainer-launched native source experiment below explicitly selects RU.

Run status before a change. It prints content-free COM and TSF profile facts without mutation.
Profile presence requires an exact TSF enumeration match and the expected proof description.
An enumeration or description API error reports unavailable, rather than absent. Only a completed
enumeration without a match establishes absence. Registry inspection errors also report unavailable.

```powershell
$bin = (Resolve-Path "target/windows-rust-check").Path
& "$bin/KeyrillRegister.exe" status
& "$bin/KeyrillRegister.exe" check
```

`check` runs the same read-only, complete EN/RU presence preflight that guards enablement.
It exits unsuccessfully if either profile is absent or cannot be inspected. The utility makes no
enablement calls until both profiles pass. After enablement calls, it checks both profiles again
and reads their enabled state before reporting success. A failed readback stops without rollback
or retry; an earlier profile's enablement may already have changed.

After accepting the draft's limits, the maintainer can explicitly register it.

```powershell
& "$bin/KeyrillRegister.exe" register "$bin/KeyrillTsf.dll"
& "$bin/KeyrillRegister.exe" status
& "$bin/KeyrillRegister.exe" check
& "$bin/KeyrillRegister.exe" enable
& "$bin/KeyrillRegister.exe" status
& "$bin/KeyrillProofEditor.exe"
```

Run these commands individually and stop if one fails. Preserve the static stage and HRESULT
from the failure before choosing another registration action.

COM registration writes only the service's `HKCU/Software/Classes/CLSID` key. The utility uses
TSF's profile and category APIs separately. The successful maintainer registration does not
establish ordinary-user registration scope. Each registration and enablement API boundary reports its stage,
static EN/RU label where applicable, and HRESULT, without DLL paths or queried text.
A failure may leave partial registration. A generic HRESULT alone does not establish its cause.
Stop on failure. Do not run enablement to repair incomplete registration. Inspect status before
deciding on cleanup or a diagnostic retry. There is no automatic
elevation or machine-wide fallback. Profile registration, explicit enablement, active source
selection, and service activation are distinct steps. The utility never selects a source.
Its explicit registration command loads the standard substitute layouts without activating them.

Select **Keyrill proof EN** in the proof editor through Windows' input-source UI. For RU checks,
select **Keyrill proof RU**. If either profile is absent or cannot be selected, record the live
gate as blocked. Do not fake a client ID, direct-call the key sink, or substitute an input hook.
Close the proof editor before removing the registration.

```powershell
& "$bin/KeyrillRegister.exe" disable
& "$bin/KeyrillRegister.exe" unregister
& "$bin/KeyrillRegister.exe" status
```

## Physical acceptance still required

Use only public synthetic fixture strings. Start with both writable documents empty, focus A,
select EN, and type physical `ghbdtn` and Space. A must contain exactly `привет ` with a collapsed
caret at UTF-16 offset 7. B must remain empty. Record whole-document and caret assertions before
and after each scenario. TSF composition identity and per-key consumption require native TSF
debugger observation; UIA cannot prove either, the origin of a key, or installed-service routing.

The inspector accepts only a proof editor PID and a named fixture oracle. It refuses other
executables and never inspects excluded controls 103 and 104. It reads A and B through UIA,
checks exact known synthetic text and the collapsed end caret, and prints only pass or fail.
It exposes no replacement, selection, keystroke, clipboard, or generic text-dump operation.

```powershell
& "$bin/KeyrillProofInspect.exe" FIXTURE_PID correction
& "$bin/KeyrillProofInspect.exe" FIXTURE_PID reversal
& "$bin/KeyrillProofInspect.exe" FIXTURE_PID continuation
& "$bin/KeyrillProofInspect.exe" FIXTURE_PID focus
& "$bin/KeyrillProofInspect.exe" FIXTURE_PID empty
```

`correction` expects `привет ` in A, `reversal` expects `ghbdtn `, and `continuation` expects
`привет x`. Those cases expect B empty. The `focus` oracle expects `привет ghbdtn ` in A with
the collapsed end caret and exactly `x` in B. It checks that returning to A does not resume
correction across a committed partial word. This fixture's RichEdit UIA Value includes one final carriage
return beyond the literal document content. The inspector compares the expected string plus
exactly that one marker; it does not trim whitespace or ignore an extra line break.

The real provider passes all four named oracles with literal fixture data. The observer rejects
a moved caret, a noncollapsed selection, an extra space, an extra line break, and a changed B
document. Restoring the expected text and caret makes it pass again. No excluded control is
queried by the inspector. Entering a target string directly calibrates the observer; it does not
prove TSF correction, key consumption, or Undo.

### Observe the host routing boundary

The diagnostic editor adds noneditable `STATIC` control 105 outside the tab order. Its initial
Name starts with `Keyrill host routing v1`. The editor STA records completed DOWN and SYSDOWN
routing outcomes in memory. A 250 ms timer publishes them only from the outer message loop,
after routing returns. Nested window-procedure timer dispatch does no diagnostic work.
Control 105 reports only host routing. Control 106 separately reports the actual service when
the reviewed diagnostic DLL is loaded through the registered profile and proof manifest.

```powershell
& "$bin/KeyrillProofInspect.exe" FIXTURE_PID routing
```

This mode verifies the fixture executable and root class, then reads only control 105's UIA
Name. It never reads A, B, or the excluded controls. A bounded parser rejects unknown versions,
fields, malformed values, and oversized status. Output reconstructs known fields rather than
printing the queried Name. Missing or rejected status exits unsuccessfully.

Read a baseline after source selection and fixture preparation. After one physical fixture
interval, wait for `publication` to advance and read again. An unchanged publication is stale
and cannot establish a zero delta. The publication sequence and every counter saturate at
4294967295. `saturated=1` means deltas are unavailable. There is no reset operation.

| Field | Host observation |
|---|---|
| `publication` | Outer-loop status publication sequence, including quiet intervals |
| `down` | DOWN and SYSDOWN messages reaching the observed outer loop |
| `test_error` | Failed `ITfKeystrokeMgr::TestKeyDown` calls |
| `no_interest` | Successful tests with a false interest output |
| `dispatch` | `ITfKeystrokeMgr::KeyDown` calls after successful test interest |
| `dispatch_error` | Failed dispatch calls, regardless of the eaten output |
| `passed` | Successful dispatch calls with a false eaten output |
| `eaten` | Successful dispatch calls with a true eaten output |
| `first_error` | First failed host call and hexadecimal HRESULT, or `none` |

These are process-lifetime host outcomes, including source-switch shortcuts, not physical-key
counts or service-callback counts. A failure already present at baseline cannot be attributed
to the later fixture interval. No interest cannot distinguish a missing sink from an inactive,
stopped, or mode-zero service. Successful dispatch does not identify a service or refusal reason.
Consumption does not prove an owned composition or document change. Run the document oracle
separately and use one fresh interval to choose the next diagnostic boundary.

### Observe the actual service refusal

The diagnostic editor polls its current thread manager with `GetFunctionProvider(service_id)`
from the same outer-loop timer. It never constructs a service to obtain diagnostics. The service
registers its `ITfFunctionProvider` through `ITfSourceSingle` only in `KeyrillProofEditor.exe`.
Registration is best effort and does not change activation success or editing eligibility.
The v2 private interface copies one fixed native scalar snapshot with an exact size and version.
It contains the frozen v1 payload and a bounded lifecycle journal. The v1 reader keeps its
original IID, layout, and copy contract so an old editor can still read a new DLL.
It exports no document data, native identifiers, offsets, keys, text, pointer values, or cookies.

```powershell
& "$bin/KeyrillProofInspect.exe" FIXTURE_PID service
```

This command reads only noneditable `STATIC` control 106. Its bounded typed codec rejects
unknown versions, fields, enum labels, malformed numbers, inconsistent status, and oversized
input. The inspector reconstructs accepted fields and makes one attempt. A valid unavailable
publication is printed as such. It does not establish service callbacks or physical correction.

`state=Available` means the current poll copied valid data from the registered provider.
`Missing` means TSF returned `TF_E_NOPROVIDER`. Other discovery failures, a successful null
provider, failed or malformed reads, and exceptions report `Error`. An active callback reports
`Busy` with no snapshot. A provider without the v2 interface reports `UnsupportedVersion`,
never an available empty lifecycle journal. A retained interface after deactivation reports `Inactive`. Each poll
replaces the preceding publication, including successful data. There is no cached success.
The service's existing editing `busy_` flag and a scalar callback-depth guard prevent incomplete
reads. The depth guard affects only diagnostic availability and performs no native query.

Read a baseline after fixture preparation, then read again after one physical interval and
advancing `publication`. One UIA sample alone does not prove freshness. The editor retains the
provider's canonical `IUnknown` locally. It exports only a host-local `source` sequence.
`continuity=Reset` begins a new baseline when provider identity or `activation` changes, or
availability recovers. `Same` permits comparison with the previous available baseline.
Editing generation changes do not end diagnostic activation continuity. Every counter saturates
at 4294967295. Saturation is explicit, and `Indeterminate` forbids interval deltas. Activation
serial saturation also stops new diagnostic registration, since an old activation cannot be
distinguished safely. The editor releases its retained identity before COM uninitialization.

`TestEntered`, `Interested`, `KeyEntered`, and `Consumed` count actual service callbacks and their
existing outputs. `HandleEntered`, `Requested`, `SessionEntered`, and `SessionCompleted` separate
handling from lock requests and completed callbacks. Present request and session HRESULTs belong
to the most recent returned request. `Attempted`, `Verified`, `Refused`, and `Unknown` report the
existing write and acknowledgement path, rather than inferring edits from native success.
The named stage fields count refusals. `first` and `latest` give presence, stage, cause, and the
native HRESULT for `Native` causes. A successful `S_FALSE` rejection remains a native result.
A failed predicate after successful calls has its named stage and a zero result.

Counters and first/latest refusal are service-instance lifetime history. They survive
reactivation, while `activation` advances and the host resets its interval baseline. A first
refusal from an earlier activation is not evidence for a later physical interval. `active`,
`stopped`, and `mode` describe the copied state, so a stopped active service remains inspectable.
All recording inside key and edit callbacks changes fixed scalar fields only. It adds no native
queries, allocation, formatting, document writes, or key-disposition decisions. Teardown detaches
diagnostic ownership before cleanup. Failed, throwing, or reentrant partial registration gets
one captured-source cleanup attempt without retrying an unknown operation.

### Observe owned-composition lifecycle

Control 106 publishes `Keyrill service v2` through the same poller, source identity, activation,
and timer as the refusal counters. The new inspector rejects v1 publications. Each lifecycle
record has `sequence/kind/subject/cause/unresolved` scalar fields. The fixed window contains
16 records in sequence order, followed by exactly zero unused entries. `event_count` identifies
the used entries, and `event_sequence` identifies the latest recorded event.

`owned_present` and `owned_serial` derive from the service's current `owned_` object.
`mark_allocated` is a local monotonic ownership label, never a native address or document ID.
A serial labels the ownership established by the service's existing canonical identity query.
It does not independently prove that TSF still exposes a live composition.

| Kind | Value | Observation |
|---|---|---|
| Acquired | 1 | Existing native creation and canonical identity checks installed ownership |
| EndRequested | 2 | The existing ordinary `EndComposition` call is about to run |
| FinishVerified | 3 | Existing post-end checks, local ownership reset, and final generation check succeeded |
| ExactTermination | 4 | The callback matched the existing canonical owned identity |
| DifferentTermination | 5 | The callback differed from the current owned identity, whose serial is the subject |
| TerminationWithoutOwned | 6 | A non-null callback arrived without local ownership, with no identity query |
| TerminationUnresolved | 7 | Null argument, failed identity query, stale state, or exception prevented classification |
| LifecycleInvalidation | 8 | The existing lifecycle path invalidated authority, retaining ownership if present |
| Detached | 9 | Teardown moved ownership out of the service, without claiming native termination |

The cause field is nonzero only for LifecycleInvalidation. Values 1 through 8 mean foreground
loss, document focus, document uninitialization, context push, context pop, profile change,
key admission, and deactivation. The unresolved field is nonzero only for TerminationUnresolved.
Values 1 through 4 mean null argument, failed identity query, stale generation or ownership,
and exception. Other event fields that do not apply are zero.

Compare two Available v2 publications only when publication advances, source and activation
match, the later continuity is Same, and no host, legacy, or lifecycle saturation is present.
Every sequence after the baseline must remain in the later window. Window overrun makes that
interval indeterminate. Sequence and mark allocation saturate without wrapping or changing
editing. If mark allocation is exhausted, ownership can remain present with unusable serial zero.

For a fresh eligible letter, observe Acquired and the same current owned serial. For ordinary
continuation after a retained boundary, observe EndRequested, FinishVerified, then Acquired with
a new serial. For focus, source, and teardown cases, report the actual ordered events without
assuming which callback Windows must deliver. An exact callback establishes its identity check.
An invalidation with a retained serial establishes local retention. A missing event proves neither
native cleanup nor a termination defect. Always run the separate whole-document and caret oracle.

The journal adds no native observation calls. Diagnostic callback guards remain active through
native releases, exception handling, and teardown. They affect read availability only. The bounded
journal stays in memory and never contains text, offsets, keys, pointers, application identifiers,
or paths. No diagnostic record authorizes a write or repairs an unknown edit.

### Repeatable native lock denial

The proof editor selects one immutable startup mode. Start a fresh instance with
`KeyrillProofEditor.exe --deny-tsf-locks`. Before showing the window, it sets `SES_CTFNOLOCK`
on its own writable A control and requires the setter result to match the immediate
`EM_GETEDITSTYLE` readback with that bit set. Uncertain setup exits with code 1 through the
existing teardown. The distinct window caption and noneditable `STATIC` control 107 report
confirmed setup. This is a fixture condition, not an input-method setting or an editing fallback.
Starting without arguments keeps the ordinary fixture behavior. Unknown, duplicate, or extra
arguments exit with code 2 before COM startup.

With A and B initially empty and the reviewed service selected in EN by the maintainer, type
physical `ghbdtn` and Space into A, then run the read-only oracle.

```powershell
& "$bin/KeyrillProofInspect.exe" FIXTURE_PID denial
```

This oracle requires the known setup confirmation, exactly `ghbdtn ` with a collapsed end
caret in A, and empty B. It checks the documents before and after a single 600 ms CLI wait.
Two service publications must advance with the same provider source and activation, both
Available, active, unstopped, EN, and unsaturated. The later publication must retain `Same`
continuity. Both must show positive `Requested`, either a failed native request HRESULT or an
`S_OK` request with a present failed session HRESULT, and a matching latest native refusal.
`SessionEntered`, `SessionCompleted`, `Attempted`, `Verified`, `Unknown`, and `Consumed` must
all remain zero. Missing providers, paused publication, successful sessions, wrong source modes,
and unchanged text alone cannot pass. The wait observes outer-loop publications outside native
input callbacks and never retries an edit. Output contains only the result, without queried text.

Controlled pure schedules cover these acceptance and rejection cases without sleeps. A pass
establishes this fixture's observed native refusal and literal document state. It does not prove
key origin, composition, correction, Undo, or other applications. A later physical run of this
promoted mode passed the strict native-denial and document/caret oracle.

### Observe an owned mark through shutdown

The host-only `--prove-owned-shutdown` mode retains the actual provider's v2 reader through
ordinary window destruction and thread-manager deactivation. An automated real-editor run in RU
mode passed `shutdown-ready` with the exact public partial word, empty B, collapsed end caret,
and an active owned serial. Clicking the title-bar Close button returned exit 0 through a retained
process handle. A separate fresh empty editor passed `empty`, rejected `shutdown-ready`, and
returned exit 16 (`NoOwnedMark`) after the same Close action. Those first runs used agent-generated
key and mouse inputs. Later physical owned-shutdown and fresh empty-close controls passed the
same bounded assertions. An earlier ready-then-Close attempt returned 16 with an unclassified
precondition loss. The attempted Close cancellation actually closed the window, so that attempt
supplies no cancellation evidence.
The mode changes no service editing policy, ABI, or journal capacity.
It cannot be combined with `--deny-tsf-locks`. Unknown, repeated, or combined arguments exit with
code 2 before COM initialization.

Run the following procedure in a fresh proof editor.

1. Start `KeyrillProofEditor.exe --prove-owned-shutdown`. Confirm the caption
   **Keyrill TSF owned shutdown fixture**. The immutable `STATIC` control 108 identifies this mode.
2. Select the existing Keyrill source through the maintainer's input-source UI. Read the actual
   service mode with the `service` inspector case and wait for an Available publication.
3. Before typing, start a separate background observer that retains the process handle and
   polls `KeyrillProofInspect.exe FIXTURE_PID shutdown-ready`. Keep that observer running without
   switching away from the editor to read or acknowledge its result.
4. With A and B empty, type physical `ghbdtn` in EN or the physical `hello` keys in RU. Do not add
   Space. Keep the editor focused for five seconds so the observer can record the ready oracle.
   It checks exactly `ghbdtn` in EN or `руддщ` in RU, empty B,
   and A's collapsed end caret, together with the exact shutdown caption and mode marker.
   It selects the literal from the observed service mode and accepts no arbitrary expected text.
5. Click the real title-bar **Close** button before returning to chat or a terminal. Require the
   observer's recorded ready result and process exit code zero. A missing ready observation or
   nonzero exit cannot pass. Record which source mode supplied the physical keys.
6. Repeat in a separate fresh process with empty A and B. Verify the `empty` oracle, retain the
   process handle, and click **Close**. This empty-close control must exit nonzero.

The content-free exit code preserves the rejection reason after the window disappears.

| Code | Shutdown result |
|---|---|
| 0 | Verified |
| 1 | Host initialization, setup, or message-pump failure |
| 2 | Invalid startup arguments |
| 10 | Missing or repeated close capture |
| 11 | Provider missing after binding |
| 12 | Reader busy |
| 13 | Invalid, failed, stopped, saturated, or contradictory evidence |
| 14 | Provider or reader identity changed |
| 15 | Activation changed |
| 16 | No owned mark at capture |
| 17 | Complete close interval not retained |
| 18 | Unknown effect present |
| 19 | Required new terminal before deactivation absent |
| 20 | Detached event in the close interval |
| 21 | Thread-manager deactivation did not return `S_OK` |

The observer must not substitute Alt+F4 for the title-bar click. That shortcut can finish the mark
before `WM_CLOSE`. The document oracle records the state before close. It cannot verify final
document text after the editor disappears, key origin, or complete callback drainage.

The outer timer binds canonical identities for the current provider and its returned reader.
Both must identify the same actual Service. Once bound, missing or changed identity, activation
change, busy reads, malformed evidence, and saturation reject the run without rearming.
The host's actual window procedure makes a fresh provider identity check and retained-reader
copy on `WM_CLOSE`, before `DefWindowProcW` performs normal destruction. This placement covers
sent close messages dispatched inside the message pump. A timer snapshot never supplies the
close baseline. Reentrant or duplicate close cannot replace the baseline or clear rejection.

Capture requires an active, unstopped service with zero Unknown count and a currently owned,
nonzero, unsaturated serial. After ordinary destruction and actual `ITfThreadMgr::Deactivate`
returning `S_OK`, the probe copies the same retained reader directly. It performs no provider
lookup after destruction. The endpoint must be inactive with the same activation, unchanged
allocation, cleared ownership, zero Unknown count, and no saturation. Its stopped flag may be set.

Every event since capture must fit the fixed 16-event journal, and retained overlap must match
exactly. Counters must not decrease. A new ExactTermination or FinishVerified must name the
captured serial before the first new Deactivate invalidation. FinishVerified also requires a new
preceding EndRequested for that serial. No new Acquired, Detached, or TerminationUnresolved event
is allowed. DifferentTermination and TerminationWithoutOwned do not establish completion, but
may accompany a valid exact terminal. Old terminal records and detached-only teardown cannot pass.
The captured ownership fact does not require the older Acquired event to remain in the journal.

The probe releases its retained references while the COM apartment is alive and caches the
result. Those references deliberately extend Service and Rust engine lifetime. A pass establishes
termination of the captured owned mark before service deactivation and a valid inactive endpoint.
It does not establish unretained shutdown behavior, engine destruction before reference release,
or that all native callbacks have drained. Missing close capture, empty ownership, setup failure,
message-pump failure, and lost history all remain unsuccessful. The observer never forces a
composition end, repairs an edit, or retries an unknown operation.

Controlled tests cover the rejection cases, exact and verified-finish schedules on the actual
Service, both release orders for frozen v1 and v2 readers, and the actual host window procedure
with a hidden test window. The lifetime cases require Service object accounting to drain only
after the final retained reader releases and run under AddressSanitizer. These checks do not
replace the fresh physical positive and empty-close runs above.

### Invoke native source selection with a freshly captured owned mark

The host-only `--prove-owned-en-to-ru` experiment passed its native and independent no-Space
UIA assertions in the real proof editor. It changes no Service editing policy, Rust code, ABI,
profile registration, or journal capacity. The maintainer alone launches this mode because it
selects a source. Default verification exercises controlled
COM doubles and never launches this mode. There are no target arguments, and repeated or
combined flags exit 2 before COM initialization.

The earlier source UI workflow passed its bounded normal-flow checks. Its journal recorded
Acquired, EndRequested, FinishVerified, document-focus notifications, and then ProfileChanged.
The maintainer selected RU, and the document/caret oracle passed after
agent-generated typing and Space. That sequence completed the mark before profile notification.
That workflow does not establish the stronger native experiment below.

In the native experiment, the maintainer launched the fixture and selected EN with empty fields.
Agent-generated keys formed the owned word. One `ChangeCurrentLanguage` call reached the exact
Keyrill RU profile, so no `ActivateProfile` call was needed. The same service recorded
ExactTermination for the captured serial before ProfileChanged, with unchanged input counters
and zero Unknown effects. Before any further input, the independent inspector confirmed the
unchanged public word, empty B, collapsed end caret, frozen native result, and advancing RU
publications. A separate later Space case passed without another correction write. The result
establishes this observed transition, not every activation path or callback order.

Run the primary experiment without any post-operation input.

1. The maintainer starts `KeyrillProofEditor.exe --prove-owned-en-to-ru` from the reviewed build.
   Confirm the **Keyrill TSF owned source fixture** caption. `STATIC` control 109 identifies the
   immutable mode, and control 110 publishes the native result.
2. Select the already registered and enabled **Keyrill proof EN** profile. Start with fresh empty
   A and B. Setup focus changes are allowed before any mark allocation on the bound provider.
3. Focus A and type exactly `ghbdtn`, without Space. The complete public word starts a fixed
   two-second countdown. Leave the editor focused and send no more input.
4. After the countdown, run `KeyrillProofInspect.exe FIXTURE_PID source-activated` from the
   separate observer without clicking the editor or sending End, Space, or any other key.
   Require exact A=`ghbdtn`, empty B, and A's collapsed end caret. The inspector also requires
   the immutable mode, the frozen native Verified result, and advancing actual RU service
   publications. Native Verified alone does not pass the document oracle.
5. Record that primary result before any continuation input. A later Space or continuation
   belongs to a separate case. It must not repair a caret mismatch and then replace the original
   result. An unsuccessful run is terminal and never rearms.

The probe acquires native profile interfaces on the editor STA, then binds the actual provider
and reader by canonical identity at an empty, active, unstopped EN snapshot with zero allocations
and zero Unknown effects. An empty baseline may refresh after setup focus notifications only
while allocation remains zero on that same provider and activation. Once a mark has been
allocated, a missed or ended mark cannot return to setup. Only successive observed prefixes of
the fixed public word can reach the countdown. Focus, lifecycle, and ownership must remain
continuous after acquisition. All five input counters must remain unchanged after arming.

Readiness validates this fixture's public controls 101 and 102, their owning thread and process,
ancestry, IDs, RichEdit class, ordinary writable/non-password styles, and focus before bounded
native text/caret reads. Eligibility is checked again after reading text before querying selection.
It never reads controls 103 or 104. Native readiness is a helper gate, not an independent
document oracle or editing authority. The independent UIA client stays out of the editor STA.

The first source mutation requires a latest monotonic observation no more than 250 ms after
the fixed deadline, including time spent in preparation and the final copy. It does not slide.
Preparatory native queries and public readiness precede the final actual reader `Copy`.
That fresh snapshot is the last potentially reentrant native-object or document query before
one `ChangeCurrentLanguage(RU)` dispatch. Only a non-reentrant `GetTickCount64` observation and
local cancellation/deadline checks follow it. Scheduling still prevents a hard real-time guarantee.
It reports the actual Service's current ownership of its last verified native mark. It does not
enumerate live TSF compositions independently.

After language selection, immediate actual language/profile readbacks and a fresh same-provider
cut must remain safe. If exact Keyrill RU is already active, the probe skips redundant profile
activation and requires completed old-mark evidence. Otherwise it may call `ActivateProfile`
once for fixed Keyrill RU, with a null layout and flags zero, after confirming current language
RU. Every native query precedes the final reader copy for that second mutation too. Any issued
call must return `S_OK`. A failed HRESULT or `S_FALSE` remains a failed run even if later state
looks favorable.

Microsoft documents that zero-flag [ActivateProfile][source-activate] requires the current input
language to match. [ChangeCurrentLanguage][source-language] selects a language and requires a
calling-thread manager. Its documentation does not promise a universal thread-only effect.
The experiment uses no enable, process, session, or deferred-activation flags, and no layout
fallback. It never calls EndComposition, injects input, pumps extra messages, rolls back,
retries, or polls a failed mutation into success.

The final native result requires actual exact RU profile/language readback and the same active,
unstopped RU Service with cleared ownership. The complete first-mutation-to-final interval must
fit the existing 16-event journal, with immutable retained overlap and nondecreasing counters.
No new Acquired, Detached, TerminationUnresolved, or Deactivate event is allowed. An exact
termination of the captured serial, or a new requested-and-verified finish of that serial,
supplies terminal evidence. Foreign callbacks cannot supply it but may coexist with it.
Any change to TestEntered, Interested, KeyEntered, Consumed, or HandleEntered rejects the
source-only claim, including a reentrant key that successfully finishes the captured serial.

The strict v1 native result reports Waiting, Countdown, Running, Verified, or Failed with a
named reason. Its bounded scalars include the language/activation call counts, failed native
result, captured activation/serial, evidence sequences, terminal kind, mutation phase, and
terminal order relative to the first ProfileChanged event when present. A notification is not
required and cannot substitute for actual profile readback. The completed native report stays
frozen. The later inspector result is separate. Close cancellation is sticky, and retained COM
references release before apartment teardown.

Controlled schedules cover source call budgets, exact parameters, conditional omission,
setup/readiness failures, fresh capture, reentry and cancellation, immutable history boundaries,
input interference, and actual Service callbacks without added document writes. Native and ASan
passes establish those controlled properties only. The maintainer-launched desktop experiment
supplies the separate bounded live evidence described above.

[source-activate]: https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofilemgr-activateprofile
[source-language]: https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofiles-changecurrentlanguage

Controlled schedules cover exact refusal stages and native versus predicate failures, request
denial versus unentered/completed sessions, busy and incompatible reads, malformed publication,
provider absence and errors, retained inactive interfaces, independent instances, saturation,
activation continuity, and failed or reentrant registration cleanup. These are controlled COM
observations. Live evidence comes from the registered proof profiles and the separately observed
physical and automated fixture cases. Controlled schedules do not replace those cases.

| Schedule | Historical C++ controlled evidence | Historical C++ live fixture evidence and limits |
|---|---|---|
| `ghbdtn` plus Space and Escape | Exact whole documents, caret, owned identity, and disposition pass | Physical correction passed; automated correction and Escape reversal passed exact document/caret oracles after the caret fix |
| Rapid next letter and moved caret | Bounded finish/replan, exact prefix/suffix preservation pass | Physical rapid continuation and automated continuation passed; adversarial caret drift remains controlled evidence |
| Denied request, failed session result, missing callback, late callback, null composition | Same-service zero-write and pass-through schedules pass | Physical native lock-denial passed with literal text, exact caret, and zero consumption or writes; injected late and null outcomes remain controlled evidence |
| Range, text, caret, active-membership drift; extra foreign composition | Refusal before write passes | Live owned identity and focus notifications were observed; injected drift and foreign-composition cases remain controlled evidence |
| Reentry at exercised COM boundaries and historical C++ exceptions | No new write after invalidation, outcome-derived consumption, and no retry pass | Live focus, source, and teardown callbacks were observed; adversarial reentry and exceptions remain controlled evidence |
| Unknown MARK and FINISH, reserved Escape | Stop without replay or repair passes | Unknown-effect hosts remain unadmitted |
| Focus A to B to A, late old termination | Old authority never revives in doubles | Physical focus preservation passed; automated runs recorded exact termination before focus invalidation and passed the final document/caret oracle; late old callbacks remain controlled evidence |
| Password, read-only, sensitive scope, disabled compartments, secure activation | Zero text reads and writes pass | Physical password/read-only attempts preserved A and B without additional service handling; excluded contents were not queried; other exclusion paths remain controlled evidence |
| Native Undo after correction and continuation | Not modeled as a TSF guarantee | Physical and automated Undo cases preserved the corrected word while removing the continuation; no general Undo-grouping guarantee |
| EN/RU profile and source changes, teardown | Profile-table checks, invalidation, and host-only source-probe schedules pass | Ordinary source UI flow, native EN-to-RU selection from a fresh owned mark, and bounded physical owned-shutdown/empty-close cases passed |

The table records historical C++ x64 MSVC and desktop evidence. Full Rust physical acceptance remains
pending. ARM64, generic editors, ordinary-user registration scope, and general application reliability remain open.
[ADR 007](adr/007-windows-tsf.md) remains proposed.

### Independent generic-host diagnostics

The content-free observer uses mapping protocol 4. The payload remains 512 fixed-width
DWORDs with its existing diagnostic sections. Word 500 carries the client PE machine
(`0x8664` for x64 or `0x014c` for x86), and words 501 through 511 are reserved zero.
Candidate kind 3 identifies a generic supported process for observation. Kinds 1 and 2
retain the proof editor and packaged Notepad identities. Observation carries no editing
authority. The independent reader binds kind, package version, and architecture to its
retained process identity alongside the PID, process creation time, and thread in the
mapping header. Protocol 3 mappings fail the protocol-4 identity check.

### Owned public fixture observers

`KeyrillPublicFixtures DIRECTORY` prepares the shared browser fixture and the deterministic
`keyrill-public-fixture-v1.txt` and `keyrill-public-fixture-v1.rtf` documents. Repeating the
command preserves matching files and refuses conflicting contents. Keep generated files under
untracked `.audit/`. Open the HTML with `?platform=windows&control=input` for Windows scenarios.
The default page retains the existing macOS scenarios. The Windows selector provides input,
textarea, plaintext contenteditable, rich contenteditable, and a second ordinary field.
Read-only, password, and payment-scope siblings expose only metadata to the observer.

Run `KeyrillHostInspect public-fixture PID CONTROL CASE` with an explicit process ID and a
compiled public case such as `empty`, `fixture-en-word`, or `fixture-ru-word`. The accepted
control selectors are `input`, `textarea`, `plain`, `rich`, `field-b`, `vscode`, and `word`.
Browser controls must expose their exact public accessible label and the public Document
ancestor title. Editor documents must expose their exact public filename through the required
provider metadata. Word accepts the exact `keyrill-public-fixture-v1.rtf` name and
`keyrill-public-fixture-v1.rtf  -  Compatibility Mode`, with two spaces on each side of the
hyphen. Other unmeasured Word name forms refuse. A logical control with HWND zero also needs a
bounded same-process ancestor chain, a matching native HWND ancestor, stable RuntimeId, and
fresh focused-element comparison.
A private tab, other field, renamed document, unavailable provider, or changed focus refuses
before document text is read. The generic-host `public-case` path refuses character inspection
without an explicit fixture identity.

The complete expected document includes a synthetic prefix containing an astral character,
the named public word, and the exact suffix. The independent UIA oracle also reads the bounded
ranges before, inside, and after the provider selection. This proves its UTF-16 location and
collapsed caret without counting UIA Character units. Word's expected suffix includes its final
paragraph carriage return. DOM checkpoint and composition counters are separate browser-owned
evidence and cannot establish that the TSF service produced the result. Use `field PID` for
metadata-only exclusion observations and identity-bound `service PID` publication for native
counters. The controlled hidden RichEdit UIA test proves provider text/selection mechanics.
Real browser, VS Code, Word, formatting, and physical-input acceptance remain separate gates.

#### Word editing limits

Manual conversion passes through in the tested public RTF body in Word 16.0.20527.20196
(x64, Compatibility Mode) with candidate `940ee5df`. With Keyrill English active, six
generated letters and the left Ctrl+Alt+K command reached seven granted edit sessions.
In every session, `ITfReadOnlyProperty::GetValue` returned `E_FAIL` for the application
input scope. Keyrill could not establish that the range was safe to inspect. It attempted
zero writes and owned no composition. The independent UIA observer confirmed that the
public word remained `ghbdtn` with the expected caret after the command.

On the same build, a generated letter in a new document outside Compatibility Mode reached
a granted session and returned the same `GetValue` error. A letter in the first cell of a
new two-cell table did the same. Both letters appeared in Word, with zero Keyrill writes and
no owned composition. These checks cover pass-through for the two tested positions.
Formatting preservation during conversion, protected documents, and x86 Word remain open.

#### Prepare the VS Code public editor

Create an owned temporary profile through `Profiles: New Window with Temporary Profile` in
an ordinary VS Code editor window. Open that profile's User Settings JSON and add these settings.

```json
{
  "editor.accessibilitySupport": "on",
  "window.title": "${activeEditorShort}"
}
```

The active-editor template makes the Document title follow the current filename. The public
observer can then use its existing exact filename checks across temporary profile names.
Record the actual profile name displayed by VS Code.

Assign `$code` to the installed Code CLI and `$profileName` to that temporary profile's name.
Assign `$fixture` to the generated public text file's full path. Open the fixture.

```powershell
& $code --profile $profileName --reuse-window --goto "${fixture}:1:20"
```

Confirm the opened editor's profile name and the active file's full path. Focus its public
editor field. Assign `$clientPid` to that window's observed editor process ID, then check the
initial complete document and caret.

```powershell
KeyrillHostInspect public-fixture $clientPid vscode empty
```

This preparation passed the unchanged native document and UTF-16 caret oracle in two fresh
temporary profiles on VS Code Insiders 1.146.0 x64. A wrong public text case failed with
`TextMismatch`, and a wrong fixture selector failed with `Binding`. These results establish the
prepared observer. Default accessibility mode, extension configurations, attributed TSF editing,
physical input, and x86 application acceptance remain open.
