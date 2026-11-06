# Keyrill engineering

Read `ARCHITECTURE.md` and `docs/development.md` before editing, and the platform document
(`docs/macos.md` or `docs/porting.md`) for native work. If `AGENTS.local.md` exists, read it too; it
holds machine- and team-specific rules that override nothing here.

Rust 2024 owns portable behavior; native adapters own platform objects and editing authority. Forbid
unsafe in the core. Keep raw pointers and unwind containment in the versioned C ABI. Never hold Rust
borrows across native callbacks. Native API success is not a document oracle.

Only edit an owned composition. Validate client, generation, exact UTF-16 mark/text/caret and actual
returned ranges around native calls; committing a mark unchanged replaces its exact range and
doesn't depend on the caret, and a commit the platform requests on focus loss writes back the text
last verified in the mark without reading it again. Each plan permits at most one write. Advance
Rust state only after verified acknowledgement. Unknown effects stop correction with no replay or
guessed repair; the one write allowed after one commits the client's own readback of its marked
text. No AX replacement, clipboard transport, synthetic deletion or committed-range fallback.
Secure, excluded and uncertain controls get no text inspection or composition. Never persist raw
input or personal vocabulary. No runtime network access.

The macOS compatibility backend in `docs/adr/006-compatibility-backend.md` is the one explicit
exception to owned-composition-only editing and the synthetic-deletion prohibition. Keep its
observed/dispatched protocol separate from verified edits. Never invoke it to repair an unknown
composition effect, swallow real input or claim that event submission verifies the document.
Compatibility proposals may submit a bounded batch; the one-write limit still applies to composition
proposals. Do not port it to another platform without a separate decision.

Run `./tools/verify` before integration. Native and ABI changes also require
`./tools/verify sanitize` and review of the actual diff and negative schedules. Say which gates were
skipped or blocked; do not substitute mock tests for physical application tests.

Use Conventional Commits and exact-path staging. Never overwrite unrelated uncommitted work. Never
commit personal dictionaries, signing identities, machine paths or test output from a real desktop.
Installing the input method, selecting sources, granting permissions and publishing releases are the
maintainer's actions, not an agent's.
