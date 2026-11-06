# ADR 007: Windows Text Services Framework

Status: proposed; needs a runtime proof.

A native TSF text service is the leading Windows design: granted edit sessions, explicit contexts
and owned compositions fit the [architecture](../../ARCHITECTURE.md) better than UI Automation reads
plus synthetic replacement. Prefer `windows-rs` on top of the shared core; a thin native shim is
acceptable.

Microsoft documents separate request and session outcomes for edit sessions, and composition
creation that an owner can reject while still returning success with a null composition. The
reference sample doesn't handle either correctly. Locks are not rollback or a focus guarantee, and
an in-process DLL failure can take down the host editor.

Accept this after physical tests in a native editor show correction, continuation, focus changes,
cancellation and denied sessions with whole-document evidence. Narrow or reject it if those
semantics, or ordinary EN/RU typing, turn out to be impractical. A successful CI build doesn't
change this status. The starting contract is in
[porting](../porting.md#windows-text-services-framework).
