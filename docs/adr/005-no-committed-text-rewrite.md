# ADR 005: no committed-text rewrite on the composition path

Status: accepted. [ADR 006](006-compatibility-backend.md) makes one explicit, separate exception.

The composition path never falls back to a temporary Accessibility selection, a whole-value write
from a snapshot, counted Backspaces and retyping, clipboard paste, or a remembered absolute range.
Controlled schedules show each of these overwriting an exposed selection, losing concurrent input or
applying a stale range; verification can detect some of that damage but can't undo it. Event tags,
worker queues, focus rechecks and pacing don't make a host atomic, and read access through UI
Automation or Accessibility is not write authority.

So converting committed words and selections remains unsupported. A failed or unknown composition
write stops correction; it never triggers another transport.

Revisit only for a specific control and native API with a serialized or conditional editing contract
and adversarial whole-document evidence, not because ordinary positive tests pass.
