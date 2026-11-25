# ADR 007: Windows Text Services Framework

Status is accepted for the bounded native editor and Notepad proof. Completion-candidate application
qualification remains pending.

A native TSF text service owns Windows editing authority. Granted edit sessions, explicit contexts,
and owned compositions implement the [architecture](../../ARCHITECTURE.md). The adapter, registrar,
settings, installer, and proof tools use Rust with windows-rs. The shared core and C ABI retain
their portable behavior. UI Automation supplies independent test and diagnostic observations.

Microsoft documents separate request and session outcomes for edit sessions, and composition
creation that an owner can reject while still returning success with a null composition. The
reference sample doesn't handle either correctly. Locks are not rollback or a focus guarantee, and
an in-process DLL failure can take down the host editor.

The [recorded Windows proof](../windows.md) includes native editor and Notepad document/caret
observations, controlled denied sessions, and separate physical keyboard results. Those results
support the bounded TSF choice. The [completion plan](../windows-completion-plan.md) requires
renewed application, physical, and installation validation against one final candidate.

Broader admission requires the [native control contract](../windows-admission-contract.md).
Unsupported controls pass input through. Terminal functional acceptance remains open until ordinary
input can be distinguished from secret input and owned readback and child-byte order are observed. A
hook-and-SendInput backend requires a separate architectural decision.
