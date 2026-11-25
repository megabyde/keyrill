# Windows validation checklist

Use this worksheet for one prepared candidate. The agent prepares the software, fixtures, commands,
observers, and automated evidence before handing over the relevant stage.

Keep completed results in the local copy at `.audit/windows-manual-checklist.md`. This tracked
document is the reusable blank template. Record public synthetic test data only. The full
implementation and acceptance requirements are in
[the Windows completion plan](../docs/windows-completion-plan.md).

## Current readiness

This template starts with unassigned candidate fields and Pending results. The existing Notepad
proof is historical evidence. Use the prepared local worksheet for the current candidate, commands,
automated evidence, and stage readiness.

| Prepared field | Value filled by agent |
| -- | -- |
| Installation stage | Pending |
| Physical-validation stage | Pending |
| Candidate version and Git commit | Pending |
| Package and production DLL identities | Pending |
| x64 and x86 package contents verified | Pending |
| Windows build and account scope | Pending |
| Exact installation command or installer | Pending |
| Required installer privilege and reason | Pending |
| Read-only installation status command | Pending |
| EN profile label | Pending |
| RU profile label | Pending |
| Fixture launcher and result-collector command | Pending |
| Public vocabulary fixtures and readiness | Pending |
| Repair, upgrade, rollback, uninstall commands | Pending |
| Candidate-specific expected native Undo results | Pending |

## Agent work completed independently

The agent owns these tasks. Installation needed for registered-app testing is the first maintainer
handoff. The agent then resumes automated application tests and prepares the final
physical-validation handoff.

- [ ] Implement shared native TSF admission for validated control contracts across applications.
- [ ] Preserve secure, read-only, excluded, uncertain, and unsupported-control pass-through.
- [ ] Keep the Keyrill source selected and automatic correction enabled for the excluded-field
  checks. Use Manual for Terminal no-echo. Check per-app Off separately in the qualified Notepad
  document by typing `ghbdtn` followed by Space and confirming the literal input.
- [ ] Complete x64/x86 builds, ABI handling, matching-bitness loading, and registration logic.
- [ ] Complete keyboard, manual-command, reversal, source-switch, and lifecycle behavior.
- [ ] Complete persistent settings, per-app restrictions, vocabulary, status, and settings UI.
- [ ] Complete install, repair, upgrade, rollback, and uninstall implementation and isolated tests.
- [ ] Run spot checks after each implementation unit and fix observed failures.
- [ ] Run full required verification, native sanitizer checks, and independent code review.
- [ ] Prepare public fixtures and collect actual application/control versions and bitness.
- [ ] Perform permitted automated application checks with independent UIA document/caret oracles.
- [ ] Freeze and verify the candidate package and every production artifact identity.
- [ ] Fill commands, expected results, physical-case assignments, and result rows for each assigned
  control.
- [ ] Prepare observers and a result collector for the entire guided physical session.
- [ ] Investigate failed results, implement fixes, and prepare a replacement candidate as needed.
- [ ] Run the complete final matrix against the final frozen candidate and publish its report.

## How to record your results

1. Begin a stage after its readiness field says Ready and its commands have been filled.
1. Read an entire physical case before focusing the test application.
1. Keep that application focused throughout the case. Record the result after the sequence ends.
1. Use `Pass`, `Fail`, `Blocked`, or `Not applicable` in result cells. Explain every non-passing
   result.
1. Use `␠` in written results to denote one Space. Type the Space key during tests.
1. Use the physical K key between J and L and the Z key immediately to the left of X.
1. Keep notes limited to the prepared public test text and content-free errors.

Use one result row per assigned application/control and case. The agent expands the result tables
before handoff so each physical sequence has its own place for your observations.

| Session field | Your entry |
| -- | -- |
| Test date |  |
| Candidate identity matches prepared field |  |
| Installation result | Pending |
| Physical-validation result | Pending |
| Package-lifecycle result | Pending |
| Result-collector report location |  |
| Blocking issue or failed case IDs |  |

## A. Install the prepared candidate

These actions belong to the maintainer under `AGENTS.md`.

- [ ] Save work in applications that the prepared installation instructions identify for restart.
- [ ] Run the exact prepared installer or registration command.
- [ ] Handle its documented elevation or permission prompt when required.
- [ ] Run the prepared read-only status command.
- [ ] Confirm both EN and RU profiles have the expected names and enabled state.
- [ ] Report installation success so the agent can launch the fixture and arm its observer.
- [ ] After the agent confirms the observer is armed, select EN through Windows' source UI.
- [ ] Leave the prepared application available for the agent's automated checks.

| Setup observation | Your result or public error |
| -- | -- |
| Installer exit/status |  |
| EN profile visible and selectable |  |
| RU profile visible and selectable |  |
| Unexpected privilege request |  |
| Setup blocker |  |

If setup fails, mark this stage Blocked and record the error. The agent diagnoses the failure and
supplies a corrected, reviewable setup step. Continue with the physical stage after the agent has
completed automated validation and marked it Ready.

## B. Follow the assigned physical cases

The agent fills the exact public target control, process bitness, case IDs, and native Undo
expectation for each row. Cases repeat where distinct native control contracts require them.
Additional browser fields and editor controls receive automated coverage and explicit physical
assignments where required by the final matrix.

| Application/control | Actual bitness filled by agent | Required physical cases filled by agent | Your result and failed IDs |
| -- | -- | -- | -- |
| Proof editor, writable controls | x64 and x86 | Pending | Pending |
| Notepad, main document | Pending | Pending | Pending |
| Chrome, local fixture controls | Pending | Pending | Pending |
| Edge, local fixture controls | Pending | Pending | Pending |
| Firefox, local fixture controls | Pending | Pending | Pending |
| VS Code, prepared editor/search controls | Pending | Pending | Pending |
| Word, prepared body/table-cell controls | Pending | Pending | Pending |
| Windows Terminal, public test-child context | Pending | T01-T03 after feasibility passes | Pending |

### P01. Automatic correction and retained reversal

Use an assigned ordinary document control with automatic correction enabled, automatic source
switching disabled, empty public dictionaries, and the EN profile selected.

1. Clear the prepared test field with Ctrl+A and Backspace.
1. Type `cgbcjr`, then one Space. Expect `список␠`.
1. Immediately press left Ctrl + left Alt + Z once. Expect `cgbcjr␠`.
1. Wait two seconds in the application, then record the final result.

| Result | Actual public text | Notes |
| -- | -- | -- |
| Pending |  |  |

### P02. Manual conversion, reversal, and modifier release

Use the EN profile with automatic source switching disabled.

1. Clear the prepared field and type `ghbdtn` without Space.
1. Hold left Ctrl and left Alt. Press and release K once.
1. Release Alt, then Ctrl. Expect `привет`.
1. Hold left Ctrl and left Alt. Press and release Z once.
1. Release Ctrl, then Alt. Expect `ghbdtn`.
1. Wait two seconds before recording the result.

| Result | Text after K | Final text after Z | Repeated text or unexpected menu |
| -- | -- | -- | -- |
| Pending |  |  |  |

### P02R. Held-command observations

Use the prepared repeat-observation fixture with the EN profile and source switching disabled. The
collector checks command callbacks and exact owned-edit results. Repeated K callbacks can convert
the owned text again. Compare the result with the collector's callback-based expectation.

1. In a fresh field, type `ghbdtn` without Space.
1. Hold left Ctrl and left Alt, then hold K for two seconds.
1. Release K, Alt, and Ctrl in that order. Wait two seconds and record the result.
1. Start a second fresh field and type `ghbdtn` without Space.
1. Press left Ctrl + left Alt + K once, keeping the application focused.
1. Press left Ctrl + left Alt + Z and hold Z for two seconds.
1. Release Z, Ctrl, and Alt in that order. Wait two seconds and record the result.

| Case | Your final public text | Collector result | Unexpected menu or stuck modifier |
| -- | -- | -- | -- |
| Held K |  | Pending |  |
| Held Z |  | Pending |  |

### P03. Rapid continuation and native Undo

Use an assigned document editor with automatic correction enabled, automatic source switching
disabled, empty public dictionaries, and the EN profile selected. The agent supplies that control's
expected native Undo result before assigning this case.

1. Clear the prepared field.
1. Type `cgbcjr`, Space, and X in one quick burst. Expect `список␠x`.
1. Immediately press Ctrl+Z once.
1. Wait two seconds. Compare the result with the prepared native Undo expectation.
1. Record the result after this whole sequence. Native Undo can end Keyrill's ownership.

| Expected native Undo result filled by agent | Your result | Actual public text |
| -- | -- | -- |
| Pending | Pending |  |

### P04. Focus changes expire the partial word

Use the prepared pair of ordinary fields/documents A and B with automatic correction enabled and the
EN profile selected. Both start empty.

1. Type `ghb` in A without Space.
1. Focus B and type `x`.
1. Return to A, press End, then type `dtn` and one Space.
1. Expect A to contain `ghbdtn␠` and B to contain `x`.

| Result | Final A | Final B | Other changes |
| -- | -- | -- | -- |
| Pending |  |  |  |

### P05. External source change ends the old composition

Use the EN profile with automatic source switching disabled.

1. Clear A and type `ghbdtn` without Space.
1. Use Windows' source UI to select the RU profile while A remains the target.
1. Confirm A still contains `ghbdtn`.
1. Press Space. Expect `ghbdtn␠`.
1. Wait two seconds before recording the result.

| Result | Final text | Selected source | Notes |
| -- | -- | -- | -- |
| Pending |  |  |  |

### P06. Automatic source switching and reversal

Use an assigned ordinary document with automatic correction and automatic source switching enabled,
empty public dictionaries, and the EN profile selected.

1. Clear the field and type `cgbcjr` followed by Space.
1. Expect `список␠` with the EN profile indicator while the reversal mark remains owned.
1. Press left Ctrl + left Alt + Z once while staying in the application.
1. Expect `cgbcjr␠` and the EN profile indicator.
1. In a second prepared empty ordinary field, type `cgbcjr`, Space, and the physical X key in one
   burst.
1. Expect `список␠ч` and the RU profile indicator. The collector checks that FINISH precedes source
   selection and fresh admission.
1. Press left Ctrl + left Alt + Z once. Expect `список␠ч` and RU to remain. The old receipt expired
   at FINISH.

| Result | Correction and retained reversal in EN | Next-letter handoff to RU | Late reversal preserves finished word |
| -- | -- | -- | -- |
| Pending |  |  |  |

### P07. Read-only and password exclusions

Use the prepared dedicated fixtures. Use only the public test word in the password fixture. The
observer checks service inspection and write counters. Password values remain unqueried.

1. Focus the read-only fixture and type `ghbdtn` followed by Space.
1. Confirm its visible contents remain unchanged.
1. Focus the password fixture and type the public `ghbdtn` followed by Space.
1. Confirm normal masking behavior and record any visible disruption.

| Result | Read-only unchanged | Normal password masking | Notes |
| -- | -- | -- | -- |
| Pending |  |  |  |

### P08. Owned shutdown and empty-close control

The agent prepares the owned-shutdown fixture and captures its exit status automatically.

1. In the first fresh fixture, select EN and type `ghbdtn` without Space.
1. Wait five seconds, then click its title-bar X while staying in that fixture.
1. In the separately prepared empty fixture, click X without typing.
1. Record the collector's positive and negative results.

The expected bounded fixture outcomes are Verified with exit code 0 for the owned case and
NoOwnedMark with exit code 16 for the empty control. The collector checks actual exit values and
native evidence.

| Result | Owned-close collector result | Empty-close collector result |
| -- | -- | -- |
| Pending |  |  |

### P09. RU correction and Escape reversal

Use an assigned ordinary document with automatic correction enabled, automatic source switching
disabled, empty public dictionaries, and the RU profile selected.

1. Clear the prepared field.
1. Press the physical W, O, R, L, D keys, then Space. Expect `world␠`.
1. Immediately press Escape once. Expect `цщкдв␠`.
1. Wait two seconds before recording the result.

| Result | Text after correction | Text after Escape |
| -- | -- | -- |
| Pending |  |  |

### P10. Assigned physical keyboard edge cases

The agent fills the short sequence and expected public result for the actual keyboard and validated
control. The assigned rows supplement the automated keyboard tests.

| Assigned case | Exact keys and expected result filled by agent | Your result and observation |
| -- | -- | -- |
| Shift and Caps Lock | Pending | Pending |
| PC punctuation and ё | Pending | Pending |
| Dead key, AltGr, or alternate layout pass-through where available | Pending | Pending |

### T01-T03. Windows Terminal

Run these cases after the agent has established Terminal feasibility, prepared the isolated public
test child, and supplied its exact launch instructions and expected byte sequences. The computer-use
skill prohibits agent UI automation of Terminal applications, so you perform the input. The agent
owns the test child, read-only diagnostics, and result assertions.

- [ ] T01. Follow the prepared ordinary-input sequence for owned manual conversion and reversal.
- [ ] T02. Follow the prepared Enter, Tab, navigation, Ctrl+C, pane-change, and output-interference
  sequence.
- [ ] T03. Use the public test word in the prepared no-echo fixture and confirm the collector's
  exclusion result.

| Case | Expected result filled by agent | Your result | Collector result or public error |
| -- | -- | -- | -- |
| T01 | Pending | Pending |  |
| T02 | Pending | Pending |  |
| T03 | Pending | Pending |  |

## C. Validate the prepared installation lifecycle

The agent supplies the exact package commands and disposable test environment for each assigned row.
Run them after saving work as directed by that candidate's instructions.

- [ ] Install on the assigned clean account and confirm both sources are selectable.
- [ ] Re-run installation or repair and confirm the expected installed state.
- [ ] Run the prepared upgrade scenario with the identified test application open.
- [ ] Restart the test application and confirm it loads the expected candidate.
- [ ] Run the prepared interruption/canceled-elevation scenario and its recovery procedure.
- [ ] Run the prepared rollback scenario where the package supports it.
- [ ] Sign out or reboot when assigned and confirm sources/settings remain correct.
- [ ] Run uninstall and confirm Keyrill's owned registrations are removed.
- [ ] Confirm unrelated input sources and the prepared user-owned dictionary remain intact.
- [ ] Reinstall and confirm normal operation.

| Lifecycle case | Your result | Status command or collector result | Notes |
| -- | -- | -- | -- |
| Clean install | Pending |  |  |
| Repair/repeated install | Pending |  |  |
| Upgrade | Pending |  |  |
| Interruption/recovery | Pending |  |  |
| Rollback | Pending |  |  |
| Sign-in/reboot | Pending |  |  |
| Uninstall/reinstall | Pending |  |  |

## D. Return the completed worksheet

- [ ] Record every assigned result and explain Fail, Blocked, or Not applicable entries.
- [ ] Identify the exact candidate used for the entire final pass.
- [ ] Include the prepared collector report location and relevant public errors.
- [ ] Keep the completed worksheet and desktop results in `.audit/`.
- [ ] Share the worksheet with the agent for independent result checks and failure diagnosis.

The agent completes the evidence matrix and fixes failures. A replacement functional candidate
requires the complete final validation pass described in the implementation plan.

| Final assessment | Your entry |
| -- | -- |
| All assigned cases completed |  |
| Failed or blocked case IDs |  |
| Unexpected input loss, duplication, or unrelated changes |  |
| Remaining issue |  |
