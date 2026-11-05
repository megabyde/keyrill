#ifndef KEYRILL_H
#define KEYRILL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define KR_ABI_VERSION UINT32_C(0x00000002)
#define KR_TEXT_CAPACITY 128
#define KR_EVENT_CAPACITY 8
#define KR_MAX_DICTIONARY_BYTES UINT32_C(1048576)

typedef struct kr_engine kr_engine;

enum {
  KR_OK = 0,
  KR_NULL_ARG = 1,
  KR_BOUNDS = 2,
  KR_BAD_TAG = 3,
  KR_STALE_PLAN = 4,
  KR_STATE = 5,
  KR_POISONED = 6,
  KR_VERSION = 7,
  KR_MODEL_ASSET = 8,
  KR_DICTIONARY_DATA = 9,
  KR_SOURCE_IO = 10,
  KR_SOURCE_CHANGED = 11,
  KR_SOURCE_NOT_REGULAR = 12,
  KR_PATH = 13,
  KR_STALE_CONFIGURATION = 14
};
enum { KR_EN = 1, KR_RU = 2 };
enum { KR_POLICY_OFF = 0, KR_POLICY_FIXTURE = 1, KR_POLICY_MODEL = 2 };
/* Russian layouts differ only on the ё key: backslash on APPLE, backtick on PC. */
enum { KR_KEYBOARD_APPLE = 0, KR_KEYBOARD_PC = 1 };
/* Context matters only for a letter planned from Idle. BOUNDARY means the caret
 * is at the document start or follows whitespace; anything else, including an
 * unread prefix, is INELIGIBLE and starts no composition. */
enum { KR_CTX_BOUNDARY = 0, KR_CTX_INELIGIBLE = 1 };
enum {
  KR_TEXT = 1,
  KR_BACKSPACE = 2,
  /* With a correction receipt, restore the typed word and consume Escape;
   * otherwise finish an owned word as typed and pass Escape to the host. */
  KR_ESCAPE = 3,
  KR_MANUAL = 4,
  KR_REVERSE = 5,
  /* Any other key or lifecycle request: finish an owned mark, then pass. */
  KR_FINISH_REQUEST = 6
};
enum { KR_PASS = 0, KR_MARK = 1, KR_FINISH = 2, KR_UNAVAILABLE = 3 };
enum { KR_KEY_PASS = 0, KR_KEY_CONSUMED = 1, KR_KEY_REPLAN = 2 };
enum { KR_APPLIED_VERIFIED = 0, KR_REFUSED_BEFORE_WRITE = 1, KR_ATTEMPTED_UNKNOWN = 2 };

typedef struct {
  uint32_t role;
  uint32_t mode;
  uint32_t context;
  uint32_t len;
  uint16_t units[KR_EVENT_CAPACITY];
} kr_event;

typedef struct {
  uint64_t plan_id;
  uint32_t action;
  uint32_t disposition;
  uint32_t expected_len;
  uint32_t mark_len;
  uint16_t expected[KR_TEXT_CAPACITY];
  uint16_t mark[KR_TEXT_CAPACITY];
} kr_plan;

/* Engine calls are serialized on the owning controller's main thread. The macOS
 * source extension specifies its separate acquisition-thread contract below.
 * Handles must be live, owned and freed once, after all operations drain. Input/output
 * pointers must be aligned, nonaliasing and valid for their complete C object.
 * Buffers are copied; no payload pointer into Rust storage crosses this interface.
 * UTF-16 lengths count units, not bytes. Rust never calls IMK/client objects.
 */
uint32_t kr_abi_version(void);
/* Versions encode major << 16 | minor; this is 0.2. Pass KR_ABI_VERSION as abi.
 * Before 1.0 there is no compatibility promise: any other value returns KR_VERSION. */
uint32_t kr_engine_new(uint32_t abi, kr_engine **out);
void kr_engine_free(kr_engine *engine); /* NULL is allowed. */
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
/* Call outside input callbacks. Publish a complete UTF-8 candidate after the
 * host's bounded/stable acquisition checks. No path or filesystem I/O crosses
 * this call. NULL+0 and non-NULL+0 are valid empty sources; NULL with nonzero
 * len is NULL_ARG. len > KR_MAX_DICTIONARY_BYTES is BOUNDS before reading.
 * Parsing follows Vim: a line whose first byte is # is a comment, and a line
 * that isn't UTF-8, an unknown directive or flag, or another invalid entry is
 * skipped and counted. NUL anywhere or a leading /encoding= other than utf-8 is
 * DICTIONARY_DATA and retains last-good protection.
 * Publication/clear are allowed while Owned but return STATE while a proposal
 * awaits acknowledgement. Any refused update leaves the pending token, history,
 * receipt and configuration unchanged. Old readers remain immutable; bytes are
 * borrowed only for this call. Clearing explicitly makes a language missing and
 * automatic policy abstains until both sources are ready again. It does not
 * restore invalidated editing authority or reinterpret an existing receipt.
 */
uint32_t kr_set_dictionary_utf8(kr_engine *engine, uint32_t language, const uint8_t *bytes,
                                uint32_t len);
uint32_t kr_clear_dictionary(kr_engine *engine, uint32_t language);
/* Write the number of skipped lines in the language's current source, whether
 * published directly or adopted. A missing language is STATE; skipped_out is
 * written only on OK. The count carries no line numbers or content.
 */
uint32_t kr_dictionary_skipped_lines(kr_engine *engine, uint32_t language, uint32_t *skipped_out);

/* Dictionary files. The vocabulary handle owns read-only sources; it owns no
 * client, editing authority, timer or worker. Observe only on a serial acquisition
 * queue, never on the UI thread or inside a native input callback. Configure/adopt
 * run on the controller thread. They may overlap observation. Free only after ALL
 * operations on the handle have completed; do not wait for filesystem work inside
 * an input callback. Adopted engines retain their Arcs. All errors are
 * content-free. Poisoning is permanent and separate from engines.
 */
typedef struct kr_vocabulary kr_vocabulary;
#define KR_MAX_PATH_BYTES UINT32_C(1024)
enum { KR_SOURCE_PENDING = 0, KR_SOURCE_PUBLISHED = 1, KR_SOURCE_UNCHANGED = 2 };
uint32_t kr_vocabulary_new(uint32_t abi, kr_vocabulary **out);
void kr_vocabulary_free(kr_vocabulary *vocabulary); /* NULL is allowed. */
enum { KR_SOURCE_MISSING_IS_EMPTY = 1 };
/* Borrow path bytes for this call: absolute, no NUL, at most 1024 bytes. POSIX
 * paths are raw bytes with no UTF-8 requirement; Windows paths are UTF-8 with a
 * drive or UNC root. No tilde expansion or filesystem access. Zero length
 * (either pointer) unconfigures the language and ignores flags. Same
 * bytes and flags are idempotent. A changed path or flag advances its generation
 * and immediately makes that language missing; failed reloads of an unchanged
 * configuration retain last-good. With KR_SOURCE_MISSING_IS_EMPTY, observing a
 * path that does not exist (ENOENT) publishes an empty source instead of failing,
 * until a file at that path has been published; after that ENOENT retains
 * last-good like any other error. Other errors are unchanged. Unknown flag bits
 * are BAD_TAG. Invalid arguments and generation exhaustion (STATE) change nothing.
 */
uint32_t kr_vocabulary_configure(kr_vocabulary *vocabulary, uint32_t language, const uint8_t *path,
                                 uint32_t len, uint32_t flags);
/* Configure the language's Vim spell file under home, an absolute directory given
 * like a path above: home/.vim/spell/en.utf-8.add or ru.utf-8.add, and
 * home\vimfiles\spell\... on Windows, where Vim's zg creates them while
 * 'spellfile' is empty. KR_SOURCE_MISSING_IS_EMPTY is implied. Zero length is
 * PATH; a joined path over KR_MAX_PATH_BYTES is BOUNDS. Configuration otherwise
 * behaves as kr_vocabulary_configure, without filesystem access.
 */
uint32_t kr_vocabulary_configure_default(kr_vocabulary *vocabulary, uint32_t language,
                                         const uint8_t *home, uint32_t len);
/* Write observation_out only on OK. Unconfigured is STATE; a superseded read is
 * STALE_CONFIGURATION and publishes nothing. Size/parse errors are BOUNDS /
 * DICTIONARY_DATA; other source errors use SOURCE_IO/CHANGED/NOT_REGULAR.
 * Two equal observations are a settling heuristic, not proof a save completed.
 */
uint32_t kr_vocabulary_observe(kr_vocabulary *vocabulary, uint32_t language,
                               uint32_t *observation_out);
/* Clone both published language views consistently and replace engine snapshots
 * together. Pending plans refuse with STATE and change nothing. This changes no
 * policy, history, receipt, token or editing authority; the last successful adopt
 * or direct dictionary publication wins. A poisoned source leaves the engine
 * unchanged and does not poison it. POISONED may refer to either handle.
 * Cloning is constant-size; releasing previous snapshots can deallocate their
 * data, so this is not a latency guarantee. No filesystem I/O, parse, acquire-lock
 * wait or native call occurs here.
 */
uint32_t kr_adopt_vocabulary(kr_engine *engine, const kr_vocabulary *vocabulary);

uint32_t kr_plan_event(kr_engine *engine, const kr_event *event, kr_plan *out);
/* One plan permits at most one native write. Verify expected text, client,
 * generation, mark and caret before writing; verify actual ranges/text after.
 * APPLIED_VERIFIED advances state. Either other outcome invalidates it. A stale
 * token never changes state. Reentrant callbacks must reset before returning.
 * ATTEMPTED_UNKNOWN stops this session. MARK consumes without replay because the
 * event may already be present. FINISH with PASS/REPLAN passes the ordinary event
 * once, without replanning: that event was not included in the finish write.
 * Reserved commands remain consumed. Input integrity remains unproved in this case.
 * Do not admit a host that produces unknown mutation outcomes for automatic correction.
 * KR_FINISH + KR_KEY_REPLAN: ack FINISH, then plan the same event from Idle.
 * A MARK leaves the caret at mark_len. A MARK with mark_len 0 empties the
 * composition: use setMarkedText, never empty insertText.
 * Every fallible export contains unwinding panics and poisons the handle.
 */
uint32_t kr_commit(kr_engine *engine, uint64_t plan_id, uint32_t outcome);

/* Experimental compatibility backend. A handle selects its backend on the first
 * successful plan; reset preserves that choice. The two plan/commit families
 * cannot be mixed. This tracks observed input, NOT document contents. No
 * input-source/document boundary is verified on activation. Ordinary letters
 * pass; manual/reverse and optional Space inference can propose counted
 * deletion and insertion. The native host must select this backend before ANY
 * IMK write, require the explicitly enabled experimental policy, check Secure
 * Input/target identity, and tag its own synthetic events to prevent recursion.
 * It must invalidate tracking on observed navigation, mouse, focus, shortcut
 * and source changes. Tags and these checks do not make the target atomic or
 * make deletion counts verifiable.
 */
enum { KR_COMPAT_OBSERVE = 0, KR_COMPAT_REWRITE = 1 };
enum {
  KR_COMPAT_OBSERVED = 0,
  KR_COMPAT_DISPATCHED = 1,
  KR_COMPAT_REFUSED = 2,
  KR_COMPAT_PARTIAL = 3
};
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
 * partial dispatch invalidates the tracker; never repair/replay an unknown effect.
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
