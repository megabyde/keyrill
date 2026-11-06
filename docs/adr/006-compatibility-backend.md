# ADR 006: compatibility backend on macOS

Status: accepted for the alpha.

Owned composition only works where the client reflects and reads back marked text, which leaves most
applications without correction. Every identified application outside the composition list gets a
tier instead. Manual only covers the built-in terminals, editors with completion and Microsoft
Office, and every unlisted application by default: Keyrill tracks the input it passes through and
rewrites a word by posting Backspace and Unicode key events, but only on the manual conversion and
reversal shortcuts. Automatic adds correction at Space, for unlisted applications when the user
turns it on. Off passes everything through: password managers, remote desktops, virtual machines and
Keyrill itself. The built-in lists ship in the bundle, and the user can override any application's
tier. A user who enables this backend accepts that it can corrupt text; keeping automatic rewrites
opt-in limits that to moments the user chose.

The backend is chosen at activation, before any edit, and is never used to repair a failed
composition write. Automatic correction needs a Space observed since reset before the word, so focus
or a click is not treated as a word boundary. Mouse, navigation, shortcuts and source changes expire
tracking; a non-space separator blocks acquiring the identifier after it. Secure Input, foreground
identity and generations are rechecked before each post, but a synchronous loop can't see a focus
change queued ahead of its events. Synthetic events are tagged, an expected-key sequence catches
stripped tags, and an automatic rewrite needs real keys to have advanced since the last one. None of
this is hardware provenance.

The ABI keeps this separate from composition. `KR_COMPAT_OBSERVED` and `KR_COMPAT_DISPATCHED` mean
submission, never `KR_APPLIED_VERIFIED`; a handle can't mix the two families; a proposal is a
bounded batch rather than one write; and a dispatch that posted anything without completing, or that
the engine refused to acknowledge, stops correction until the next activation, with no replay. A
rewrite refused before its first post changed nothing, so it only drops the tracked word. Reversal
is a best-effort rewrite to a predicted original, not a receipt.

What it gives up: counted deletion is unverifiable, so typing during a rewrite, application edits
and focus changes can delete the wrong characters (the controlled test
`compatibility_known_interleaving_hazard` keeps such a case). Terminals, TUIs and remote sessions
may treat Backspace or keycodes as commands, and raw-key terminals also see the command chords.
Hidden prompts without Secure Input are tracked in memory. It needs Accessibility and Input
Monitoring permissions. For this backend only, this decision overrides S1, S4 and S6 in
[architecture](../../ARCHITECTURE.md#safety-invariants) and
[ADR 005](005-no-committed-text-rewrite.md). Don't describe it as safe, transactional or supported,
and don't carry it to another platform without a separate decision.
