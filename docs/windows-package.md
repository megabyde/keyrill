# Windows package and installation

`KeyrillSetup.exe` owns native installation and recovery through the pinned Rust and windows-rs
dependencies. The maintainer launches machine mutations explicitly with administrator rights.
Input-source enablement and selection are separate maintainer actions. Preferences and personal
dictionaries retain their user-owned lifecycle.

## Candidate preparation

Commit the candidate source before staging. Build both production targets, then stage into a new
directory. Complete packages require the x64 and x86 artifacts from the same source closure.

```console
cargo xtask windows package-build --arch all
cargo xtask windows package --candidate target/candidate
cargo xtask windows package-verify --candidate target/candidate
```

`package-build --arch x64` and `--arch x86` prepare one architecture independently. Outputs live
under `target/windows-package-x64` and `target/windows-package-x86`. Each contains the actual
production DLL, Settings, Register, Setup, and a build receipt. Remapping removes build-machine
paths; the linker receives `/Brepro` and a relative PDB name. Package staging consumes these
receipts and refuses a different source closure, compiler, target, feature set, or file hash.

The candidate contains both client architectures, English/Russian icons, the project license,
model provenance/notices, and dependency license declarations and texts. Release validation
enforces the exact production binaries, exports, resources, and license inventory.
The manifest records the commit, source closure, compiler, model, lockfile, target fingerprints,
and every installed file hash. Its SHA-256 is the candidate identity. Verification works after
copying the directory through read-only validation. Unsigned hashes establish content consistency.
Publisher authenticity requires maintainer signing.

Validation bounds each opened file before allocating or reading its contents. The shared
package reader permits 64 MiB per executable, 4 MiB per public resource and 1 MiB per JSON
file. A complete package permits 256 files including its manifest and 256 MiB in total.
These budgets allow headroom for the embedded model, both client architectures and dependency
licenses. The allowed layout has at most three relative path components. Windows validation
uses whole-path no-reparse native opens and enumerates retained directory handles. It rejects
offline entries and retains directory and file leases until all content checks finish.
Declared PE tables permit 96 sections, 128 exports and
128-byte export names.

The native boundary uses `NtCreateFile` with `OBJ_DONT_REPARSE` for each actual open and
`NtQueryDirectoryFile` with one bounded `FileNamesInformation` entry per query. The general
`OBJECT_ATTRIBUTES` contract documents this flag's reparse refusal. The function-specific
`NtCreateFile` page lists zero and `OBJ_CASE_INSENSITIVE` for that field. Native tests verify
the selected combination on the test host, including a directory converted into a junction
while retained. A handle-based query stays on that original directory and a child path open
refuses the redirect.
[OBJECT_ATTRIBUTES](https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes),
[NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile),
[NtQueryDirectoryFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntquerydirectoryfile)

An elevated host checks the retained package-root, architecture, manifest, DLL and Settings
ACLs before it reads package contents. An ordinary host can validate a complete user-owned
package. Settings launch remains bound to the loaded DLL's production identity, manifest
identity and retained peer hashes.

The launch keeps the immediate-child lease chain through process creation. Native tests on
the local test filesystem observe `ERROR_DIR_NOT_EMPTY` when a retained populated directory
is changed into a junction, and refusal of child deletion and directory replacement.
These checks cover the ordinary local-filesystem contract. Arbitrary filesystem filters and
concurrent DOS drive remapping have no qualification evidence.

Repeat staging into a second new directory from the same build receipts and compare the manifest
and file hashes. Candidate staging is deterministic for those inputs. Independently rebuilding
the binaries is a separate reproducibility check. Signing changes binary bytes and requires a
new manifest, candidate identity, and renewed validation. Signing and publication remain maintainer work.

## Explicit maintainer commands

Run `x64/KeyrillSetup.exe status` for read-only owned registration observations. `verify PACKAGE`
checks candidate contents. Machine mutation commands require an explicit administrator launch
and `--scope machine`.

```console
KeyrillSetup.exe install --scope machine PACKAGE_DIRECTORY
KeyrillSetup.exe repair --scope machine PACKAGE_DIRECTORY
KeyrillSetup.exe migrate-proof --scope machine PACKAGE_DIRECTORY EXISTING_PRODUCTION_PROOF_DLL
KeyrillSetup.exe recover --scope machine
KeyrillSetup.exe rollback --scope machine
KeyrillSetup.exe uninstall --scope machine
KeyrillSetup.exe cleanup --scope machine
```

The standard US and Russian keyboard layouts must already appear in `GetKeyboardLayoutList`.
A missing layout is a pre-mutation refusal with a Windows Settings instruction. Setup uses the
exact full native HKL for `SubstituteKeyboardLayout`. The native API requires a previously loaded
layout handle. [SubstituteKeyboardLayout](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofiles-substitutekeyboardlayout)

## Bounded profile repair

Ordinary recovery refuses a profile that differs from both original receipt states. A separately
approved `repair-profile` command can attempt the next pending EN or RU profile once. Its only
destination is that change's original `after` value, verified against the original staged package.
The original journal schema, comparisons and cursor are unchanged.

Run `inspect-profile-repair` from the corrected candidate's `x64/KeyrillSetup.exe` to print a JSON
draft to standard output. Inspection requires no elevation, creates no installer files and grants
no authority. Save the exact UTF-8 JSON bytes outside the protected installation root. Review the
draft and compute the saved file's SHA-256. Inspection and mutation both verify the executing
Setup against its complete candidate package. The approval pins its executable, package and
production source identities, and requires a package distinct from the original defective one.

The draft binds the original receipt bytes, package, previous identity, cursor and profile. It
contains raw profile type/data in both machine views, exact observations of every other owned
resource, both machine and user TSF and COM trees, raw Enable data, registry descendant names,
the user SID, loaded layouts and the installed receipt's identity or absence. The owner-thread
source is a separate observation. Different inspection and repair processes need not share that
source. Each repair checks that its own source remains unchanged around the native call.

After disposable native qualification and approval review, the maintainer can run these explicit
administrator commands from the corrected candidate. The agent must not run them.

```console
KeyrillSetup.exe repair-profile --scope machine APPROVAL_JSON APPROVAL_SHA256
KeyrillSetup.exe recover --scope machine
KeyrillSetup.exe install --scope machine CORRECTED_CANDIDATE_DIRECTORY
```

Repair archives the exact original receipt and approval under the protected root. It publishes
`prepared`, then a durable `attempted` marker before its one native `AddLanguageProfile` call.
Every native result gets a full readback. Exact destination and protected-state agreement can
verify convergence even after a native HRESULT failure. The outcome retains native and readback
errors. No compensating write is permitted.

After a crash, `prepared` may continue after fresh checks. `attempted` with exact destination
can publish `verified` without another native call. `attempted` with unchanged source cannot
replay. A durable `stopped` record never reopens through repetition, even if later observations
appear repaired. Another attempt requires a fresh reviewed draft explicitly linking the previous
attempt, and its source must still be an exact third state. Protected-resource conflicts require
inspection and remain a stop. This command does not promise recovery from every unknown effect.

`verified` certifies the profile and stable protected state. The attempt marker also retains its
owner-thread source observation. A restart cannot observe that terminated thread again. Its
outcome retains the original observation, records the new thread's observation separately, and
reports source preservation as unavailable after restart. Repeated verification retains that
limitation. Physical source preservation remains an independent maintainer check.

Ordinary mutation refuses unresolved repair history. Verified history retains the original
receipt and package through recovery, upgrade and cleanup. After moving to another installed
package, corrected Setup refuses to select the defective original package through install,
repair or rollback. The retained old executable cannot enforce this restriction and must not
be invoked. A repair history permits at most sixteen explicitly linked approvals.

Provider tests cover authorization, checkpoint failures and no replay. They do not establish
that native TSF preserves Enable, source selection or service metadata on a real host. Disposable
native qualification, final independent installation readback and the physical package matrix
remain maintainer gates.

The disposable native test is compiled only with the off-by-default
`maintainer-native-qualification` Cargo feature. Ordinary verification uses `--include-ignored`,
so an ignored annotation alone does not isolate a registration test. Default Setup test binaries
exclude both qualification entries. The feature-enabled mutation test also refuses before any
platform work unless the maintainer explicitly sets
`KEYRILL_MAINTAINER_NATIVE_PROFILE_QUALIFICATION` to `disposable-profile-qualification-v1`.
Do not set that variable in an automated verification environment.

The exact mutation test is
`native::tsf::tests::disposable_native_profile_repair_preserves_protected_state`.
It uses fresh, absence-checked service and profile GUIDs. Its forwarding provider preserves
the production writer's string pointers and counts while replacing only the validated identity
tuple. Thirteen cases check raw metadata, Enable states, substitute layouts, the RU sibling,
production registration, installation files and observed sources. Valid public icon fixtures and
an append-only evidence file use a unique temporary directory. A failure stops without retry.
Setup and update failures retain the fixture. Cleanup starts only after fresh disposable and
protected observations match the verified case. Each native removal runs once, with its HRESULT
recorded before failure propagation. Native cleanup must leave all COM roots and all machine
TIP roots absent in both views. For an explicitly enabled or disabled case, the fixture owner
then permits only the exact original HKCU EN `Enable` DWORD and its four empty ancestor or
profile keys. Both views must match this fixed chain before any registry deletion. The
absent-Enable case requires HKCU TIP absence. Exact value deletion and bottom-up empty-key
checks stop on unexpected content. Final TIP and COM absence remains mandatory across both
hives and views, with unchanged protected observations. Readbacks and operation errors remain
in the evidence on cleanup failure. The consuming fresh owner accepts no retained identity.
Empty-key inspection and deletion are separate Win32 operations, so they cannot exclude a
concurrent writer between those calls.
Retained test registrations require maintainer inspection. This test has not yet qualified the
host merely because its executable builds.

The separate exact test
`native::tsf::tests::read_only_native_profile_qualification_preflight` needs the same build feature
but no mutation token or elevation. It checks the actual observer and loaded-layout prerequisites
without creating fixtures or registering profiles. A handoff must pin the feature executable's
hash, enumerate the exact mutation test and require a one-test passing result. A zero-test run
does not qualify native behavior.

COM values are owned in both explicit HKLM views. TSF definitions use native profile/category
APIs with fresh registry readback in both views. HKCU profile Enable values are observed
separately and preserved during installation. Scope checks reject unexpected owned descendants,
profile metadata in HKCU, divergent machine views, and unrecognized preexisting registrations.
These registry observations define the tested installation contract. An unsupported shape causes
an explicit scope refusal.

Setup retains NUL-terminated UTF-16 storage for both profile descriptions and icon paths through
`AddLanguageProfile`. Its explicit character counts exclude the terminators and count UTF-16
units, including surrogate pairs. The earlier registrar fix `2e5fafb` addressed persisted suffix
garbage from unterminated description storage. The setup path now applies the same storage rule
to both strings. Safe buffer regressions cover EN, RU, empty strings and surrogate pairs, and
in-process COM tests check exact counted readback and interrupted-operation recovery. Actual
machine registration remains a separate maintainer gate.
[AddLanguageProfile](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofiles-addlanguageprofile)

The machine TIP service root permits either no values or one case-insensitive `Enable` value
whose type and bytes are exactly `REG_SZ` `1` with one UTF-16 terminator. This bounded recognition
comes from observed native registration metadata. Its origin and semantics remain undocumented.
Every service read validates those names and bytes and compares root presence and the raw optional
value across both views before returning the receipt's existing boolean. A mismatch refuses
mutation. The boolean treats a present root with either accepted representation as the same
service state, so receipts do not preserve absence versus string `1`. Unexpected values stop
recovery with its receipt retained. Native TSF calls retain exclusive ownership of service
registration changes. Current-user profile `Enable` values retain their separate exact-value
checks.

Stable class/profile GUIDs survive the proof-to-product transition. Explicit proof migration
accepts only the caller-selected production proof DLL, matching proof names/icon paths, and the
exact current user's shadow COM path and Apartment value.
`KeyrillRegister register --scope machine PACKAGE_DIRECTORY` and `unregister --scope machine` delegate to the
adjacent Setup owner. Enable/disable remain separate explicit maintainer operations.

## Persistence and recovery

Setup creates a protected Program Files `Keyrill` directory. Administrators and SYSTEM can write.
Users can read and execute. Later mutation runs validate that DACL. Both bitnesses resolve the
native Program Files path through the explicit 64-bit machine registry view and compare the
known-folder result where available. An embedded `asInvoker` manifest preserves the caller's
explicit privilege context. An exclusively opened file serializes installers across sessions
and bitness. Normal application and Settings use runs under the user's ordinary token.

The Settings shortcut passes an ordinary absolute drive path to `IShellLinkW::SetPath` after
requiring that it canonicalizes to the exact installed target. Filesystem paths use canonical
form. Native ShellLink probes on the test host accepted 260 UTF-16 units including the terminator
and refused 261. Setup refuses longer Shell targets and representations that change target identity
before writing shortcut scratch data.

Complete verified files are staged before COM points at an immutable `versions/CANDIDATE_HASH`
directory. `operation.json` durably records every exact before/after resource and the last
verified acknowledgement. `installed.json` records the completed result. Cleanup requires the
complete operation to match its durable installed receipt. Recovery freshly reads all resources
and accepts the recorded before/after values. Verified completed effects advance the cursor.
An API failure followed by the desired readback counts as convergence. An unavailable or third
state stops with the receipt preserved for inspection.

Rollback uses the previous verified package through the same reconciler. Same-package repair
preserves that previous version. An unrecognized changed
registration is a conflict, including during uninstall. Uninstall preserves preferences and all
dictionary contents. Old version files remain available to loaded clients. `cleanup` verifies
the exact file receipt, keeps in-use files, and durably records partial cleanup for a later retry.
The current and previous installed versions are retained for rollback. After uninstall, a Setup
copy outside those directories can remove them once client processes and the old Setup have exited.

The automated tests use synthetic PE parser fixtures, injected journal providers, real disposable
HKCU `Software/KeyrillTests` values, and temporary files. The original-name x86 Setup test executable
also passed its actual native Program Files and embedded-manifest checks under an ordinary token.
Maintainer acceptance covers both targets, standard/admin accounts, proof migration,
interrupted registration, upgrade with loaded clients, rollback, uninstall/reinstall, additional
accounts, and sign-in/reboot. Those gates stay pending until executed against the final candidate.
