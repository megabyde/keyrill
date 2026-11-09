# ADR 002: a selected input method that owns its composition

> [!TIP]
> **Status:** accepted. The default build admits composition only in TextEdit, Safari and Firefox,
> the clients whose composition behavior has been tested.

The user selects Keyrill's EN or RU mode instead of the stock layouts. Keyrill composes the current
word as marked text it owns and changes only that mark, after checking client, generation, exact
UTF-16 text, range and caret. After Space the word and Space stay marked until the next input, so
reversal can update the same mark.

The alternatives were a transparent global observer, post-Space editing through Accessibility,
synthetic deletion and retyping, and absolute IMK ranges over committed text. An observer still
needs a safe way to rewrite committed text, and none of the others supplied one
([ADR 005](005-no-committed-text-rewrite.md)).

The costs are visible: installing and selecting an input method, explaining source switching, and a
marked word that changes underline and Undo behavior per host. Ownership checks are mandatory; a
mark is not a document lock, and a client that ignores or rewrites a marked update can't be
admitted. On this path, committed-text and selection conversion are not supported;
[ADR 006](006-compatibility-backend.md) adds a separate backend that rewrites the tracked word.

Revisit if ordinary bilingual typing, completion or Undo proves unacceptable in applications that
matter, or if a platform offers a stronger editing transaction with adversarial evidence.
