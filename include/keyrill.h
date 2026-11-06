/* Keyrill C ABI.
 *
 * Keyrill corrects words typed in the wrong keyboard layout, English QWERTY or Russian ЙЦУКЕН.
 * The engine behind this header is portable Rust; everything native belongs to the adapter that
 * calls it: the input context, the host's text client, focus, Secure Input, the edit and its
 * verification. The engine never sees the document. It sees the keys the adapter reports, keeps
 * the word it believes is in the adapter's owned composition (the marked text the input method
 * itself started and controls), and answers each key with a plan: a proposal that permits at most
 * one native write. The adapter checks the plan against the real composition, makes the write,
 * reads the composition back and acknowledges what happened. The engine's state advances only on
 * an acknowledgement that the write was verified; a plan is a proposal, never editing authority.
 *
 * One key, as the adapter sees it:
 *
 *   key event
 *     |
 *     v
 *   kr_plan_event(engine, &event, &plan)    copies a plan out; confirmed state is unchanged
 *     |
 *     +-- PASS, UNAVAILABLE:  no write
 *     +-- MARK, FINISH:       recheck client, generation, composition == expected, caret;
 *     |                       one write; read the composition back
 *     v
 *   kr_commit(engine, plan.plan_id, outcome)
 *     |                       APPLIED_VERIFIED advances the session; REFUSED_BEFORE_WRITE and
 *     |                       ATTEMPTED_UNKNOWN invalidate it until kr_reset
 *     v
 *   plan.disposition:         KEY_PASS gives the key to the host, KEY_CONSUMED keeps it,
 *                             KEY_REPLAN plans the same event once more from Idle
 *
 * Handles. A kr_engine is one word session for one input context. A kr_vocabulary reads
 * personal dictionary files; any number of engines adopt its published snapshots. Both are
 * opaque, created by *_new and released by *_free, which accepts NULL. Nothing else in this
 * interface allocates on the caller's behalf: kr_event, kr_plan, kr_compatibility_plan, byte
 * buffers and output integers belong to the caller, are read or written only during the call and
 * are never retained. No pointer into Rust storage crosses the interface.
 *
 * Threading. Calls on one engine are serialized on the thread that owns its input context; the
 * engine has no lock of its own, and two engines share nothing mutable. A kr_vocabulary is
 * internally synchronized, with its own contract below. Rust never calls back into the adapter
 * or the host, so no Rust borrow is live while native code runs, and a host callback that
 * reenters the adapter between kr_plan_event and kr_commit may call kr_reset.
 *
 * Text. Every length is a count of UTF-16 units, except byte lengths of UTF-8 dictionaries and
 * paths. Pointers must be aligned, valid for the whole C object they point to and must not
 * alias a handle or each other.
 *
 * Failures. Every function except kr_abi_version and the two *_free returns a status code.
 * Status codes carry no text, path or line number. A Rust panic inside a call is caught at the
 * boundary and poisons that handle: every later call on it returns KR_POISONED and only *_free
 * remains useful (a panic in *_new returns KR_POISONED with no handle). A caught panic does
 * not recover from an allocation failure, which aborts.
 */
#ifndef KEYRILL_H
#define KEYRILL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Version major << 16 | minor; this header is 0.3. Before 1.0 there is no compatibility
 * promise, and a change that breaks callers bumps the minor version. */
#define KR_ABI_VERSION UINT32_C(0x00000003)
/* Capacity of the plan text arrays, in UTF-16 units. */
#define KR_TEXT_CAPACITY 128
/* Capacity of the text one key event carries, in UTF-16 units. */
#define KR_EVENT_CAPACITY 8
/* Largest dictionary, in bytes, whether published directly or read from a file. */
#define KR_MAX_DICTIONARY_BYTES UINT32_C(1048576)

typedef struct kr_engine kr_engine;

enum {
  KR_OK = 0,
  /* A required pointer, the handle included, is NULL. */
  KR_NULL_ARG = 1,
  /* A length exceeds its capacity: event text, dictionary, path or dictionary file. */
  KR_BOUNDS = 2,
  /* A tag, flag or enum field has a value this header does not define, or a non-TEXT event
   * carries text. */
  KR_BAD_TAG = 3,
  /* The plan_id is not the latest plan of this engine: a newer plan, kr_reset or an earlier
   * acknowledgement superseded it. Nothing changes. */
  KR_STALE_PLAN = 4,
  /* The call is not allowed in the current state: the session is invalidated, a plan awaits
   * acknowledgement, a word is owned during configuration, the engine is bound to the other
   * backend, a compatibility acknowledgement does not fit its plan, a source is unconfigured or
   * missing, or a counter is exhausted. Nothing changes. */
  KR_STATE = 5,
  /* A panic was caught in this or an earlier call; the handle is permanently unusable. */
  KR_POISONED = 6,
  /* The abi argument is not this library's version. */
  KR_VERSION = 7,
  /* The embedded model failed validation; the automatic policy stays OFF. */
  KR_MODEL_ASSET = 8,
  /* A dictionary candidate cannot be parsed as a whole; the last good one stays. */
  KR_DICTIONARY_DATA = 9,
  /* A dictionary file could not be opened, read or inspected. */
  KR_SOURCE_IO = 10,
  /* A dictionary file changed identity, size or timestamps while it was read. */
  KR_SOURCE_CHANGED = 11,
  /* A dictionary path names something other than a regular file. */
  KR_SOURCE_NOT_REGULAR = 12,
  /* A path is empty where one is required, relative, contains NUL or, on Windows, is not
   * UTF-8. */
  KR_PATH = 13,
  /* The source was reconfigured while it was read; the read publishes nothing. */
  KR_STALE_CONFIGURATION = 14
};
/* Input mode of an event, and language of a dictionary. */
enum { KR_EN = 1, KR_RU = 2 };
enum { KR_POLICY_OFF = 0, KR_POLICY_FIXTURE = 1, KR_POLICY_MODEL = 2 };
/* Russian layouts differ only on the ё key: backslash on APPLE, backtick on PC. */
enum { KR_KEYBOARD_APPLE = 0, KR_KEYBOARD_PC = 1 };
/* Context matters only for a letter planned from Idle. BOUNDARY means the caret
 * is at the document start or follows whitespace; anything else, including an
 * unread prefix, is INELIGIBLE and starts no composition. */
enum { KR_CTX_BOUNDARY = 0, KR_CTX_INELIGIBLE = 1 };
enum {
  /* The units the key types. A single EN/RU letter starts a word at a boundary or extends the
   * owned one; in English mode so do the keys that type Russian letters, such as [ and ;. Space
   * after an owned word runs automatic correction and is appended to the mark, and the word
   * and Space stay marked as the boundary mark until the next key. Other text finishes the
   * owned word and passes. */
  KR_TEXT = 1,
  /* Remove the last owned unit, which expires a receipt; removing the only one empties the
   * composition and returns to Idle. With no owned word, or on a boundary mark, it finishes
   * the word and passes. */
  KR_BACKSPACE = 2,
  /* With a correction receipt, restore the typed word and consume Escape;
   * otherwise finish an owned word as typed and pass Escape to the host. */
  KR_ESCAPE = 3,
  /* Reserved command: convert the owned word to the other layout. With no owned word the plan
   * is UNAVAILABLE and the command is still consumed. */
  KR_MANUAL = 4,
  /* Reserved command: restore the word as typed while its receipt is valid. Without a receipt
   * the plan is UNAVAILABLE and the command is still consumed. */
  KR_REVERSE = 5,
  /* Any other key or lifecycle request: finish an owned mark, then pass. */
  KR_FINISH_REQUEST = 6
};
/* Plan actions. PASS writes nothing. MARK sets the composition to mark, caret at mark_len.
 * FINISH replaces the composition with mark and commits it, ending the composition.
 * UNAVAILABLE is a reserved command with no owned word to act on; it writes nothing. */
enum { KR_PASS = 0, KR_MARK = 1, KR_FINISH = 2, KR_UNAVAILABLE = 3 };
/* What happens to the key once the plan is acknowledged as verified. REPLAN comes only with
 * FINISH: acknowledge the finish, then plan the same event once more from Idle. */
enum { KR_KEY_PASS = 0, KR_KEY_CONSUMED = 1, KR_KEY_REPLAN = 2 };
/* Acknowledgement outcomes. APPLIED_VERIFIED: the write was made and its readback matched
 * mark, or the plan had no write. REFUSED_BEFORE_WRITE: a check failed and nothing was
 * written. ATTEMPTED_UNKNOWN: a write was attempted and its effect cannot be established. */
enum { KR_APPLIED_VERIFIED = 0, KR_REFUSED_BEFORE_WRITE = 1, KR_ATTEMPTED_UNKNOWN = 2 };

/* One key or command. len counts units and must be 0 unless role is KR_TEXT.
 *
 *   offset  0  role     KR_TEXT .. KR_FINISH_REQUEST
 *           4  mode     KR_EN or KR_RU, the input mode the key arrived in
 *           8  context  KR_CTX_BOUNDARY or KR_CTX_INELIGIBLE
 *          12  len      0 .. KR_EVENT_CAPACITY
 *          16  units    uint16_t[8]                                  32 bytes in all
 */
typedef struct {
  uint32_t role;
  uint32_t mode;
  uint32_t context;
  uint32_t len;
  uint16_t units[KR_EVENT_CAPACITY];
} kr_event;

/* A copied plan. expected is the exact text the owned composition must hold before the write,
 * empty when no word is owned; mark is the text to write. Units past each length are zero.
 *
 *   offset   0  plan_id       token for kr_commit; only the latest plan is accepted
 *            8  action        KR_PASS, KR_MARK, KR_FINISH or KR_UNAVAILABLE
 *           12  disposition   KR_KEY_PASS, KR_KEY_CONSUMED or KR_KEY_REPLAN
 *           16  expected_len
 *           20  mark_len
 *           24  expected      uint16_t[128]
 *          280  mark          uint16_t[128]                          536 bytes in all
 */
typedef struct {
  uint64_t plan_id;
  uint32_t action;
  uint32_t disposition;
  uint32_t expected_len;
  uint32_t mark_len;
  uint16_t expected[KR_TEXT_CAPACITY];
  uint16_t mark[KR_TEXT_CAPACITY];
} kr_plan;

/* Returns the library's KR_ABI_VERSION, for comparison with the header it was built against.
 * It cannot fail. */
uint32_t kr_abi_version(void);
/* Pass KR_ABI_VERSION as abi; any other value returns KR_VERSION. *out is set to NULL first
 * and receives the handle only on KR_OK. The new engine is Idle, with policy OFF, the APPLE
 * keyboard and both dictionaries missing. */
uint32_t kr_engine_new(uint32_t abi, kr_engine **out);
/* Free once, after every call on the engine has returned and no callback can reach it. */
void kr_engine_free(kr_engine *engine); /* NULL is allowed. */
/* Drop the word, its receipt, the pending plan and the word history, and clear an invalidated
 * session. Call it whenever the adapter's generation for the input context changes; a plan made
 * before then becomes STALE_PLAN. Configuration (policy, keyboard, dictionaries) and the
 * selected backend survive; poisoning does not clear.
 */
uint32_t kr_reset(kr_engine *engine);
/* The policy is OFF initially. FIXTURE is a test policy that corrects only
 * ghbdtn and руддщ. Configure outside input callbacks, while Idle with no
 * pending plan. Actual policy changes clear word history; an unchanged policy
 * is an idempotent OK under the same Idle requirement. MODEL validates its
 * asset once per process and uses the frozen default ranking policy, personal
 * vocabulary protection and the selected keyboard. MODEL_ASSET leaves the
 * policy OFF with cleared history, never a fixture fallback. Both dictionaries
 * must be published before MODEL can correct automatically; missing sources
 * still permit manual conversion. Reset preserves configuration.
 */
uint32_t kr_set_automatic_policy(kr_engine *engine, uint32_t policy);
/* The keyboard is APPLE initially and maps manual conversion and MODEL inference.
 * Configure outside input callbacks, while Idle with no pending plan; other
 * values are BAD_TAG. A change keeps word history, which holds finished text,
 * not key positions. Setting the current keyboard is an idempotent OK under the
 * same Idle requirement. Reset preserves the selection.
 */
uint32_t kr_set_keyboard(kr_engine *engine, uint32_t keyboard);
/* Publish a whole UTF-8 dictionary for one language, outside input callbacks. The adapter does
 * any file reading and stability checks itself; no path or file I/O crosses this call, and the
 * bytes are borrowed only for its duration. NULL or non-NULL with len 0 is a valid empty
 * source; NULL with nonzero len is NULL_ARG; len > KR_MAX_DICTIONARY_BYTES is BOUNDS before
 * any byte is read.
 *
 * Parsing follows Vim: a line whose first byte is # is a comment, and a line
 * that isn't UTF-8, an unknown directive or flag, or another invalid entry is
 * skipped and counted. NUL anywhere, or an /encoding= other than utf-8 before the first word,
 * is DICTIONARY_DATA and keeps the last good dictionary.
 *
 * Publication and clearing are allowed while a word is owned but return STATE while a plan
 * awaits acknowledgement. A refused update leaves the pending plan, history, receipt and
 * configuration unchanged. Snapshots are immutable: replacing this engine's copy never changes
 * one that another engine or a vocabulary handle still holds. Clearing makes a language
 * missing, and the automatic policy abstains until both languages have a dictionary again.
 * Neither call restores an invalidated session or reinterprets an existing receipt.
 */
uint32_t kr_set_dictionary_utf8(kr_engine *engine, uint32_t language, const uint8_t *bytes,
                                uint32_t len);
uint32_t kr_clear_dictionary(kr_engine *engine, uint32_t language);
/* Write the number of skipped lines in the language's current dictionary, whether
 * published directly or adopted. A missing language is STATE; skipped_out is
 * written only on OK. The count carries no line numbers or content.
 */
uint32_t kr_dictionary_skipped_lines(kr_engine *engine, uint32_t language, uint32_t *skipped_out);

/* Dictionary files. A kr_vocabulary reads up to six vocabulary sources, the two Vim languages
 * and four learned-word slots, and publishes each as an immutable snapshot. It owns no client,
 * editing authority, timer or worker thread; the adapter decides when to observe.
 *
 *   acquisition queue (serial)          controller thread
 *   kr_vocabulary_observe  ---------->  published snapshots  <------  kr_vocabulary_configure
 *   (opens and reads files)                    |                       kr_adopt_vocabulary
 *                                              v
 *                                  engines' copies, replaced by adopt
 *
 * Call observe only on one serial queue, never on the UI thread or inside a native input
 * callback, since it does file I/O. Configure and adopt run on the controller thread and may
 * overlap an observation; they never wait for file I/O. Free only after every call on the
 * handle has returned; do not wait for file work inside an input callback. An engine keeps the
 * snapshots it adopted after the vocabulary handle is freed. Errors are content-free. A
 * vocabulary handle poisons on its own, permanently and separately from engines.
 */
typedef struct kr_vocabulary kr_vocabulary;
#define KR_MAX_PATH_BYTES UINT32_C(1024)
/* Observation results. PENDING: a valid candidate is staged and waits for a second, equal
 * observation. PUBLISHED: the candidate became the source's snapshot. UNCHANGED: the file still
 * equals the published snapshot. */
enum { KR_SOURCE_PENDING = 0, KR_SOURCE_PUBLISHED = 1, KR_SOURCE_UNCHANGED = 2 };
/* Pass KR_ABI_VERSION as abi; *out is set to NULL first and receives the handle only on KR_OK.
 * The new handle has no configured source and reads nothing. */
uint32_t kr_vocabulary_new(uint32_t abi, kr_vocabulary **out);
void kr_vocabulary_free(kr_vocabulary *vocabulary); /* NULL is allowed. */
/* A source is KR_EN or KR_RU, read as a Vim word list, or a learned slot
 * KR_LEARNED + n for n below KR_LEARNED_SOURCES: a plain list, one word per line,
 * in UTF-8 (BOM optional) or UTF-16 LE with its BOM, as OS spell checkers keep added and
 * excluded words. Learned words protect and forbid like Vim entries, but automatic
 * correction never waits for a learned slot; only the two languages gate it. */
enum { KR_LEARNED = 16, KR_LEARNED_SOURCES = 4 };
/* Configuration flags; see kr_vocabulary_configure. */
enum { KR_SOURCE_MISSING_IS_EMPTY = 1, KR_SOURCE_EXCLUDED = 2 };
/* Point a source at a file, without touching the filesystem. The path bytes are borrowed for
 * this call: absolute, no NUL, at most KR_MAX_PATH_BYTES. POSIX paths are raw bytes with no
 * UTF-8 requirement; Windows paths are UTF-8 with a drive or UNC root. There is no tilde
 * expansion. Zero length (with either pointer) unconfigures the source and ignores the flags
 * the source accepts.
 *
 * The same bytes and flags again are an idempotent OK. A changed path or flag advances the
 * source's generation and makes the source missing at once, so the next adopt drops it; a
 * failed observation of an unchanged configuration keeps the last good snapshot.
 *
 * With KR_SOURCE_MISSING_IS_EMPTY, observing a path that does not exist (ENOENT) publishes an
 * empty source at once instead of failing, until a file at that path has been published; after
 * that ENOENT keeps the last good snapshot like any other error, because editors that save by
 * rename leave the path briefly absent. Other errors are unchanged. KR_SOURCE_EXCLUDED, for
 * learned slots only, makes every word in the list forbidden. A flag bit the source does not
 * accept (an undefined bit, or KR_SOURCE_EXCLUDED for a language) is BAD_TAG, even with zero
 * length. Invalid arguments and generation exhaustion (STATE) change nothing.
 */
uint32_t kr_vocabulary_configure(kr_vocabulary *vocabulary, uint32_t source, const uint8_t *path,
                                 uint32_t len, uint32_t flags);
/* Configure the language's Vim spell file under home, an absolute directory
 * given like a path above: home/.vim/spell/en.utf-8.add or ru.utf-8.add, and
 * home\vimfiles\spell\... on Windows, where Vim's zg creates them while
 * 'spellfile' is empty. KR_SOURCE_MISSING_IS_EMPTY is implied. A learned slot
 * is BAD_TAG and zero length is PATH; a joined path over KR_MAX_PATH_BYTES is
 * BOUNDS. Configuration otherwise behaves as kr_vocabulary_configure, without
 * filesystem access.
 */
uint32_t kr_vocabulary_configure_default(kr_vocabulary *vocabulary, uint32_t language,
                                         const uint8_t *home, uint32_t len);
/* Read the source's file once and write a KR_SOURCE_* result to observation_out, only on OK.
 * The first valid read stages a candidate (PENDING); the next read with equal metadata and
 * bytes publishes it, so a new file takes two calls; only the empty source of
 * KR_SOURCE_MISSING_IS_EMPTY publishes in one. Unconfigured is STATE; a read superseded by
 * reconfiguration is STALE_CONFIGURATION and publishes nothing. Size and parse errors are
 * BOUNDS and DICTIONARY_DATA; other file errors are SOURCE_IO, SOURCE_CHANGED or
 * SOURCE_NOT_REGULAR. A read error discards the staged candidate, and no error replaces the
 * last good snapshot. Two equal observations are a settling heuristic, not proof that a save
 * completed.
 */
uint32_t kr_vocabulary_observe(kr_vocabulary *vocabulary, uint32_t source,
                               uint32_t *observation_out);
/* Copy both language snapshots and the published learned lists from the vocabulary in one step
 * and replace the engine's copies together. A pending plan refuses with STATE and changes
 * nothing. This changes no policy, history, receipt, plan or editing authority; the last
 * successful adopt or direct dictionary publication wins for a language, and learned lists
 * change only here. A poisoned vocabulary leaves the engine unchanged and does not poison it;
 * POISONED may refer to either handle. Copying clones at most six reference-counted pointers,
 * but releasing the previous snapshots can free their memory, so this is not a latency
 * guarantee. No file I/O, parsing, wait on an observation's file read or native call occurs
 * here.
 */
uint32_t kr_adopt_vocabulary(kr_engine *engine, const kr_vocabulary *vocabulary);

/* Plan one event. On OK the plan is copied to out and becomes the engine's only pending plan,
 * replacing any earlier unacknowledged one; no confirmed state changes until kr_commit. An
 * invalidated session, an exhausted token counter or an engine bound to the compatibility
 * backend is STATE.
 */
uint32_t kr_plan_event(kr_engine *engine, const kr_event *event, kr_plan *out);
/* Acknowledge the latest plan with what actually happened. Acknowledge every plan, including
 * PASS and UNAVAILABLE, which have no write and are APPLIED_VERIFIED.
 *
 * One plan permits at most one native write. Before writing, verify the client, the generation,
 * that the owned composition holds exactly expected, and the caret; after writing, verify the
 * actual ranges and text by readback. A FINISH whose mark equals expected replaces the exact
 * composition range and does not depend on the caret. A MARK leaves the caret at mark_len. A
 * MARK with mark_len 0 empties the composition: set empty marked text (setMarkedText on
 * macOS), never insert empty text.
 *
 * APPLIED_VERIFIED advances the session. Either other outcome invalidates it, and kr_plan_event
 * returns STATE until kr_reset. A stale plan_id never changes state. A native callback that
 * reenters during the write and invalidates the context must call kr_reset before it returns;
 * the outer kr_commit then gets STALE_PLAN, which is not an engine refusal.
 *
 * Key disposition after REFUSED_BEFORE_WRITE: nothing was written, so the key passes unchanged.
 * After ATTEMPTED_UNKNOWN, stop this session. A MARK consumes the key without replay, because
 * the write may already have put it in the document. A FINISH with KEY_PASS or KEY_REPLAN passes
 * the ordinary key once, without replanning, since the finish write did not include it.
 * Reserved commands stay consumed. This contains the ambiguity; it does not prove the host kept
 * the input, so do not admit a host that produces unknown outcomes for automatic correction.
 */
uint32_t kr_commit(kr_engine *engine, uint64_t plan_id, uint32_t outcome);

/* Experimental compatibility backend, macOS only, for clients that cannot keep an owned
 * composition: the adapter tracks the input it passes through and rewrites a word with synthetic
 * Backspace and Unicode key events. Nothing here verifies the document; the terms are in
 * docs/adr/006-compatibility-backend.md.
 *
 * A handle selects its backend on the first successful plan; reset preserves that choice. The
 * two plan/commit families cannot be mixed: the other family returns STATE. This tracks
 * observed input, NOT document contents. No input-source or document boundary is verified on
 * activation. Ordinary letters pass; manual and reverse commands and optional Space inference
 * can propose counted deletion and insertion; reversal is a best-effort rewrite to the
 * predicted original, not a receipt. The native host must select this backend before ANY IMK
 * write, require the explicitly enabled experimental policy, check Secure Input and target
 * identity, and tag its own synthetic events to prevent recursion. It must invalidate tracking
 * on observed navigation, mouse, focus, shortcut and source changes. Tags and these checks do
 * not make the target atomic or make deletion counts verifiable.
 */
/* Plan operations. OBSERVE: nothing to dispatch; the key goes to the host. REWRITE: delete
 * delete_units UTF-16 units before the caret and type replacement as synthetic events. */
enum { KR_COMPAT_OBSERVE = 0, KR_COMPAT_REWRITE = 1 };
/* Acknowledgement outcomes. OBSERVED acknowledges an OBSERVE plan and DISPATCHED a submitted
 * REWRITE. REFUSED means nothing was submitted, PARTIAL that only part of the batch was. */
enum {
  KR_COMPAT_OBSERVED = 0,
  KR_COMPAT_DISPATCHED = 1,
  KR_COMPAT_REFUSED = 2,
  KR_COMPAT_PARTIAL = 3
};
/*   offset   0  plan_id
 *            8  operation        KR_COMPAT_OBSERVE or KR_COMPAT_REWRITE
 *           12  disposition      KR_KEY_CONSUMED for MANUAL and REVERSE, else KR_KEY_PASS
 *           16  delete_units     0 unless REWRITE
 *           20  replacement_len  0 unless REWRITE
 *           24  replacement      uint16_t[128]                       280 bytes in all
 */
typedef struct {
  uint64_t plan_id;
  uint32_t operation;
  uint32_t disposition;
  uint32_t delete_units;
  uint32_t replacement_len;
  uint16_t replacement[KR_TEXT_CAPACITY];
} kr_compatibility_plan;
/* automatic is 0/1; 0 disables inference regardless of engine fixture/model policy.
 * Automatic inference requires the run to begin after a Space observed since reset;
 * activation/reset alone does not count as a word boundary. Manual observed-run
 * conversion remains available before that first boundary.
 * Automatic Space rewrites return KR_KEY_PASS: delete_units includes the original
 * Space, and replacement includes its separator. The host must pass that Space
 * before the queued rewrite is delivered; do not consume it or insert it again.
 * event.context is validated but replaced by bounded observed-input context.
 * Escape clears tracking and passes; it does not rewrite previously committed text.
 * Returned strings remain caller-owned fixed arrays. */
uint32_t kr_plan_compatibility_event(kr_engine *engine, const kr_event *event, uint32_t automatic,
                                     kr_compatibility_plan *out);
/* OBSERVED accepts an ordinary input observation; DISPATCHED accepts a rewrite
 * submission. Neither verifies host delivery, deletion or resulting text.
 * A mismatched acknowledgement is STATE with pending state unchanged. Refused or
 * partial dispatch invalidates the tracker until kr_reset; never repair or replay an unknown
 * effect.
 */
uint32_t kr_commit_compatibility(kr_engine *engine, uint64_t plan_id, uint32_t outcome);

#ifdef __cplusplus
}
static_assert(sizeof(kr_event) == 32);
static_assert(sizeof(kr_plan) == 536);
static_assert(offsetof(kr_plan, expected) == 24);
static_assert(sizeof(kr_compatibility_plan) == 280);
#else
_Static_assert(sizeof(kr_event) == 32, "kr_event ABI");
_Static_assert(sizeof(kr_plan) == 536, "kr_plan ABI");
_Static_assert(offsetof(kr_plan, expected) == 24, "kr_plan text offset");
_Static_assert(sizeof(kr_compatibility_plan) == 280, "kr_compatibility_plan ABI");
#endif
#endif
