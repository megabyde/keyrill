#import <Foundation/Foundation.h>
#import <InputMethodKit/InputMethodKit.h>

#include "keyrill.h"

NS_ASSUME_NONNULL_BEGIN

@protocol KRVocabularyAdopting <NSObject>
- (uint32_t)adoptVocabulary:(kr_vocabulary *)vocabulary;
@end

/// Most recent input-boundary observation; contains no text, offsets or client identity.
typedef NS_ENUM(uint8_t, KRInputStatus) {
  KRInputNotObserved,
  KRInputReady,
  KRInputExcluded,
  KRInputSecure,
  KRInputContextUnavailable,
  KRInputNotAtBoundary,
  KRInputMarkMismatch,
  KRInputCaretMismatch,
  KRInputTextUnavailable,
  KRInputTextMismatch,
  KRInputInterrupted,
  KRInputEngineUnavailable,
  KRInputClientException,
};

/// Owns one Rust engine handle and the native authority over at most one marked composition.
/// Main thread only. Every client call may reenter; a reentrant callback bumps the generation,
/// resets Rust and drops local authority, and the interrupted operation then refuses or reports
/// an unknown outcome instead of writing again.
@interface KRSession : NSObject <KRVocabularyAdopting>

/// Main-thread-only, process-wide host callback nesting. A nested loop in one controller can
/// run work for another. Begin/end off main are no-ops; the query returns YES off main.
/// End at zero is a no-op. Overflow saturates and permanently defers adoption until process exit.
+ (void)beginInputCallback;
+ (void)endInputCallback;
+ (BOOL)inputCallbackActive;

/// Takes ownership of `engine` and frees it in dealloc. `allowedClients` lists the bundle
/// identifiers in which a composition may start; a nonempty client bundle identifier and every
/// secure-input, ownership and readback check remain required.
- (instancetype)initWithEngine:(kr_engine *)engine
                allowedClients:(NSSet<NSString *> *)allowedClients NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

@property(nonatomic) uint32_t mode;
/// True after an unknown write outcome, a contract violation or a poisoned engine. Ordinary
/// keys then pass with zero client calls until `activate` (never again once poisoned). After a
/// verified write, the first unknown outcome in an activation stops only that word: the next
/// text key re-arms the session and starts from fresh client checks.
@property(nonatomic, readonly) BOOL stopped;
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

/// `event->context` is ignored and computed here from the client. Returns the IMK disposition.
- (BOOL)handleEvent:(const kr_event *)event client:(id<IMKTextInput>)client;
/// Finishes the owned mark if it is still verifiably owned in `client`; otherwise only drops
/// local authority. Returns YES only for a verified Finish acknowledged by the engine.
- (BOOL)finishInClient:(id<IMKTextInput>)client;
/// Finish before replacing configuration. Refuses reentrancy, permanent poison or a newly
/// unknown finish; the caller must keep this session when it returns NO.
- (BOOL)finishForReconfigurationInClient:(id<IMKTextInput>)client;
/// Drops local authority without any client call.
- (void)invalidate;
/// Starts a new client session: drops authority and clears a nonpoisoned stop.
- (void)activate;
/// Adopts immutable dictionary views outside input callbacks. The caller keeps `vocabulary`
/// alive for this call. Returns STATE during host callbacks or dispatch; changes no native
/// editing authority. POISONED may refer to either handle and does not change native stop state.
- (uint32_t)adoptVocabulary:(kr_vocabulary *)vocabulary;

@end

NS_ASSUME_NONNULL_END
