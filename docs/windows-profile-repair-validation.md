# Windows profile repair validation

The maintainer's migration stopped after the EN profile description acquired an unexpected suffix.
The original operation remains at its profile write. This change corrects the string storage and
supplies an explicitly approved single-profile recovery path. Actual native qualification initially
stopped at disposable cleanup. The corrected candidate passed all thirteen native cases on each
architecture, followed by approved recovery and installation. Installed application acceptance
remains pending.

The buffer regression is committed as `aa65701`, its correction as `eba8195`, and the bounded repair
as `4fe3846`. The separately gated native qualification is in `bfc073b` and `863607b`. The cleanup
regression is committed as `d187dc9` and its correction as `4471d61`. The frozen replacement was
built at `4471d61fc0f233212ab1d44ac306c1df3174e7a5`. Later documentation does not change its
immutable bytes or source inputs.

## Frozen candidate

| Identity | SHA-256 |
| -- | -- |
| Package | `adafb276a84af1fb86828849d2616b5664b9c006b33a043a852a691bd940306e` |
| Source inputs | `cd57f456678d733a2ac6b6263f1a59bd4ec0d91346a71d8baa9a1317826ed9ea` |
| x64 production build | `0d3892e11e410a00ca4de4d80fdbdf25bb7644dfe66cefa74ad7c5cb7d068dce` |
| x86 production build | `2eba79e11a8003448b162a897719a3d1b2dc10e6d25c8b9b187d94516ae41bbf` |
| x64 DLL bytes | `20eb6d3a55af71f9833119060d5e6ab754f7c41f6d4073dd542d885c036efbb2` |
| x86 DLL bytes | `2b733b4d39cd9c74fc84c8cf6cc783e5e12fa15c2dbfd3857c6f4e9120bea9c2` |
| x64 Setup bytes | `8dc6f1ec21f239dd912d35bd7d2d6f0db45a8b2e767934086c9b5ebd547f2ae7` |

## Implemented behavior

Both TSF profile strings have terminated backing storage. Their explicit SDK counts exclude the
terminators. Safe regressions cover full buffers and counted UTF-16, including empty strings and
surrogate pairs. They do not read beyond the pre-fix allocation.

The recovery approval binds the original receipt, exact raw source, both registry views, protected
resources, current user and verified corrected executable. It permits one profile write to the
original desired value. A durable attempt marker prevents replay after an ambiguous interruption.
Ordinary reconciliation remains unchanged. The original receipt is archived without editing it to
accept the corrupted state.

Unresolved repair history blocks ordinary mutation. After upgrade, retained history prevents
selecting the defective predecessor through corrected Setup. Old executables cannot enforce that
guard and must not be used. Restart readback can verify persistent observations but explicitly
records that preservation of the terminated tool thread's source is unavailable.

Three recovery designs were compared. The selected design avoids the broader unjournaled legacy
registrar batch and the extra EN rewrite of restoration to the original before-state. Independent
design and actual-diff reviews found no remaining actionable issue in this scope.

Qualification cleanup now consumes ownership of a fresh, initially absent test identity. After
native removal, it accepts only the case's exact user Enable DWORD and the fixed English profile
chain. Both views are checked before deletion. Unexpected values or branches stop cleanup. It
removes the exact value and checks each empty key before deleting it bottom-up. The original final
absence and protected-state comparisons remain mandatory. Retained identities from older failed runs
cannot be supplied to this cleanup. Registry inspection and deletion are separate operations; this
test-only mechanism does not exclude a concurrent writer atomically.

## Completed checks

| Check | Result |
| -- | -- |
| Setup tests | 48 passed on x64 and x86 |
| Isolated native registry cleanup regression | Failed before correction; seven cleanup tests passed after correction |
| Clippy with warnings denied | Passed on x64 and x86 |
| `cargo xtask windows check --arch all` | Exit 0 |
| `cargo xtask windows sanitize` | Exit 0, required use-after-free canary detected |
| Controlled TSF suite | 214 passed per normal architecture and under x64 ASan |
| Runtime suites | 80 production and 81 test-API cases passed per native run |
| Setup, proof and ABI suites | 48, 53 and 3 passed per native run |
| Clean dual-architecture package rebuild | All 119 paths and file hashes identical |
| Native package verification and read-only status | Passed on both architectures |
| Default test isolation | Maintainer qualification entries absent from both test lists and normal runs |
| Explicit token refusal | Both feature executables refused the exact native test before registration |
| Real read-only qualification observer | Passed on x64 and x86 under an ordinary token |
| Final package's read-only repair inspection | Exit 0, original cursor and 25 protected resources bound |
| Local handoff checks | Exact test enumeration rejects zero-test success; PowerShell 5.1 and 7 syntax, SID and file checks passed |

The feature-enabled executables each passed 48 ordinary tests with the two explicit native entries
ignored. Their absent-token refusals passed. An initial read-only observer attempt failed because no
foreground window was available. A separate read-only probe confirmed successful TSF queries and
valid layouts, but a null foreground window and thread. After a diagnostic editor supplied a
foreground window, the exact read-only test passed on both architectures. The failed observations
were retained separately.

The isolated cleanup tests cover wrong values, bytes and types, unexpected branches, empty prefixes,
second-view preflight, late content, evidence failure and preservation of an older fixture. They do
not establish independent WOW64 views on this host or inject native removal, value-deletion and
enumeration failures. Those schedules received code review only.

The ASan configuration is experimental. Prebuilt standard libraries and Windows code remain
uninstrumented. The native qualification binary is separate from production packaging and requires
both its explicit build feature and maintainer token. Normal verification never executes its
registration cases.

## Native qualification and installation

The maintainer's first qualification run stopped when foreground focus changed during a
protected-state comparison. A second run passed four complete cases and verified the fifth profile
update, then failed the final cleanup assertion. Reaching that assertion establishes that native
profile removal and service unregistration returned success, but the disposable EN profile's
explicit user `Enable=0` preference remained. Read-only inspection confirmed the machine
registration was absent and the user preference remained in both registry views. The original
installation receipt is unchanged. These results belong to the earlier candidate, not the corrected
frozen package.

Those partial results did not establish complete native preservation or cleanup. Earlier failed
fixtures and their evidence remain retained without retry or guessed cleanup.

The corrected candidate's first attempt passed three cases, then stopped when only the foreground
window and thread changed. All four fresh service identities were independently confirmed absent. A
subsequent maintainer run passed all thirteen cases on x64 and all thirteen on x86. Both evidence
journals end with qualification-passed. The approved profile repair, ordinary recovery and corrected
installation then completed. Independent readback verified the installed package's identity and
integrity, matching installation and operation receipts with 22 of 22 changes complete, both machine
COM registrations, and absence of the former user COM registrations.

Installed application checks, physical input, account/integrity cases, reboot and lifecycle
acceptance remain open. Hosted cross-platform checks were not run in this local pass. See
[package recovery](windows-package.md#bounded-profile-repair) and the
[maintainer checklist](windows-maintainer-checklist.md).

Fix Root Causes kept the correction at the string-storage boundary. Model the Domain shaped the
explicit profile and attempt states. Prove It Works kept actual native preservation and physical
application acceptance separate from provider and package checks.
