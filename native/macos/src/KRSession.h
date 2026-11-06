#import <Foundation/Foundation.h>
#import <InputMethodKit/InputMethodKit.h>

#include "keyrill.h"

NS_ASSUME_NONNULL_BEGIN

/// A session that can take the runtime's published vocabulary snapshots into its engine. Both
/// backends implement it, so `KRRuntime` adopts into whichever session the active controller
/// routed to. Returns the `kr_adopt_vocabulary` status; STATE means "not now", and the runtime
/// retries after a later observation.
@protocol KRVocabularyAdopting <NSObject>
- (uint32_t)adoptVocabulary:(kr_vocabulary *)vocabulary;
@end

/// Most recent input-boundary observation, shown in the input menu as the reason the last word
/// was left alone. It contains no text, offsets or client identity.
typedef NS_ENUM(uint8_t, KRInputStatus) {
  KRInputNotObserved,
  KRInputReady,
  /// The client's bundle identifier is not in the composition set.
  KRInputExcluded,
  KRInputSecure,
  /// The selection was not a collapsed caret, the client already had a mark, or the unit before
  /// the caret came back with another length or range.
  KRInputContextUnavailable,
  /// The UTF-16 unit before the caret is not whitespace: the word continues other text.
  KRInputNotAtBoundary,
  KRInputMarkMismatch,
  KRInputCaretMismatch,
  /// The client returned no string for a range it was asked to read.
  KRInputTextUnavailable,
  /// The readback differed from the expected text, or the client returned a different range.
  KRInputTextMismatch,
  /// A host callback reentered while a plan was in flight.
  KRInputInterrupted,
  /// The engine refused the plan or acknowledgement, or the handle is poisoned or missing.
  KRInputEngineUnavailable,
  KRInputClientException,
};

/// Native authority over one owned composition: the marked text Keyrill itself placed in the
/// client. Every write this class makes targets marked text, never committed text. It owns one
/// Rust engine handle. Rust proposes a plan; this class checks that the client still holds exactly
/// what the plan expects, performs the plan's one write, reads the result back and only then
/// acknowledges it, and the acknowledgement is the only way Rust state advances.
///
/// Main thread only. A client call is synchronous IPC and can spin a nested run loop, so any of
/// them can reenter the controller with a deactivation, a mode change or another key. A
/// reentrant callback bumps the generation, resets Rust and drops local authority; the
/// interrupted operation sees the new generation at its next recheck and refuses before the
/// write or reports an unknown outcome after it, and never writes again. Automatic correction
/// of "ghbdtn" at Space, with the mark at [s, s+6) and the caret at s+6:
///
///   kr_plan_event          MARK, expected "ghbdtn", mark "привет " a proposal only
///   markedRange            == [s, s+6)                             recheck generation
///   selectedRange          == {s+6, 0}                             recheck generation
///   Secure Input           off
///   stringFromRange        == "ghbdtn", actual range == [s, s+6)   recheck generation
///   Secure Input           off
///   -- local authority dropped: from here the outcome is verified or unknown --
///   setMarkedText          "привет ", selection at its end         the plan's one write
///   markedRange            == [s, s+7)                             recheck generation
///   selectedRange          == {s+7, 0}                             recheck generation
///   Secure Input           off
///   stringFromRange        == "привет ", actual range == [s, s+7)  readback
///   kr_commit              APPLIED_VERIFIED: Rust advances, the new mark is owned
///
/// A failed check before the write acknowledges REFUSED_BEFORE_WRITE: nothing was written and
/// the key passes. A failed check after it acknowledges ATTEMPTED_UNKNOWN: correction stops,
/// and a key the write carried is consumed, never replayed. When that check failed on the
/// client's answer rather than on reentrancy, an exception or Secure Input, the session commits
/// the client's own exact readback of its marked range once, if both are still available, so
/// that the next key can't replace it; that is the one write allowed after an unknown outcome,
/// and it repairs nothing.
@interface KRSession : NSObject <KRVocabularyAdopting>

/// Process-wide depth of host input callbacks on the main thread. While it is nonzero,
/// vocabulary adoption waits and runtime replacement and deferred mode requests are refused,
/// because a nested run loop inside one controller's callback can run work queued for another.
/// Begin and end off main are no-ops, and the query returns YES off main. End at zero is a no-op.
/// Overflow saturates and defers adoption until process exit.
+ (void)beginInputCallback;
+ (void)endInputCallback;
+ (BOOL)inputCallbackActive;

/// Takes ownership of `engine` and frees it in dealloc. `allowedClients` lists the bundle
/// identifiers in which a composition may start; a nonempty client bundle identifier and every
/// secure-input, ownership and readback check remain required.
- (instancetype)initWithEngine:(kr_engine *)engine
                allowedClients:(NSSet<NSString *> *)allowedClients NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

/// Input mode (KR_EN or KR_RU) of a finish the session plans on its own; the controller keeps
/// it equal to its current mode. A key carries its mode in its event.
@property(nonatomic) uint32_t mode;
/// True after an unknown write outcome, a contract violation or a poisoned engine. Ordinary
/// keys then pass with zero client calls until `activate` (never again once poisoned). After a
/// verified write, the first unknown outcome in an activation stops only that word: the next
/// text key re-arms the session and starts from fresh client checks.
@property(nonatomic, readonly) BOOL stopped;
/// YES while the session holds local authority over a verified mark.
@property(nonatomic, readonly) BOOL owned;
@property(nonatomic, readonly) KRInputStatus inputStatus;
/// Mode to request after the most recent verified automatic Space conversion (the other mode),
/// or after a verified reversal or Escape restore of a mark kept across a mode change (the
/// mark's mode); zero otherwise.
@property(nonatomic, readonly) uint32_t automaticSwitchMode;
/// An owned mark ends with its Space, so it can be kept across a mode change: the next event
/// in another mode reverses or restores it in its own mode, or finishes it first.
@property(nonatomic, readonly) BOOL boundaryRetained;
/// Replaceable for controlled tests; defaults to `IsSecureEventInputEnabled`. Checked at entry
/// and again after each client query that precedes a text read or a write.
@property(nonatomic, copy) BOOL (^secureInputEnabled)(void);

/// Plans one key and applies the plan to `client`. `event->context` is ignored and computed
/// here from the client. Returns the IMK disposition: YES consumes the key, NO passes it to the
/// client.
- (BOOL)handleEvent:(const kr_event *)event client:(id<IMKTextInput>)client;
/// Finishes the owned mark if it is still verifiably owned in `client`; otherwise only drops
/// local authority. This is the lifecycle finish: an unchanged commit writes back the text last
/// verified in the mark without reading it again. No authority remains afterwards. Returns YES
/// only for a verified Finish acknowledged by the engine.
- (BOOL)finishInClient:(id<IMKTextInput>)client;
/// Finish before replacing configuration. Refuses reentrancy, permanent poison or a newly
/// unknown finish; the caller must keep this session when it returns NO.
- (BOOL)finishForReconfigurationInClient:(id<IMKTextInput>)client;
/// Drops local authority without any client call: bumps the generation, so in-flight work sees
/// itself as stale, and resets Rust, which expires any plan and reversal receipt.
- (void)invalidate;
/// Starts a new client session: drops authority and clears a nonpoisoned stop.
- (void)activate;
/// Adopts immutable dictionary views outside input callbacks. The caller keeps `vocabulary`
/// alive for this call. Returns STATE during host callbacks or dispatch; changes no native
/// editing authority. POISONED may refer to either handle and does not change native stop state.
- (uint32_t)adoptVocabulary:(kr_vocabulary *)vocabulary;

@end

NS_ASSUME_NONNULL_END
