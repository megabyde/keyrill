#import <Foundation/Foundation.h>

#include "keyrill.h"

#ifndef KR_BUNDLE_IDENTIFIER
#error "build.sh defines KR_BUNDLE_IDENTIFIER from Info.plist"
#endif

@class KRSession;
@protocol KRVocabularyAdopting;

NS_ASSUME_NONNULL_BEGIN

/// The automatic-correction policy as the input menu reports it.
typedef NS_ENUM(uint8_t, KRPolicyStatus) {
  KRPolicyStatusOff,
  KRPolicyStatusModel,
  /// MODEL was requested but its asset failed validation; engines run OFF.
  KRPolicyStatusModelUnavailable,
};

/// Content-free state of one language's personal source in the shared vocabulary handle. Ready
/// means published there, not yet adopted by any particular engine.
typedef NS_ENUM(uint8_t, KRSourceStatus) {
  /// No vocabulary is in use (policy OFF, or the model failed validation), so no source is
  /// configured or read.
  KRSourceStatusUnconfigured,
  /// Configured, with no settled observation yet.
  KRSourceStatusPending,
  KRSourceStatusReady,
  /// A later read failed; the last good snapshot stays in force.
  KRSourceStatusRetained,
  /// No valid snapshot was ever read, or the path setting is unusable. Automatic correction
  /// abstains; manual conversion still works.
  KRSourceStatusMissing,
};

/// How Keyrill handles one application. Secure Input passes every key regardless of tier.
typedef NS_ENUM(uint8_t, KRTier) {
  /// Pass-through: no client inspection, composition or synthetic input.
  KRTierOff,
  /// Owned composition with automatic and manual correction and exact readback checks.
  KRTierComposition,
  /// Compatibility backend on the manual and reversal commands only.
  KRTierManual,
  /// Compatibility backend with automatic correction on Space as well.
  KRTierAutomatic,
};

typedef struct {
  KRPolicyStatus policy;
  KRSourceStatus english;
  KRSourceStatus russian;
  /// The Learn Spelling list. It protects and forbids words when ready, but correction never
  /// waits for it.
  KRSourceStatus learned;
} KRRuntimeStatus;

/// Process-wide automatic-correction configuration and personal-source acquisition. Policy and
/// paths are read once at initialization. Under MODEL, a serial work queue observes both
/// language sources and the learned list, and published snapshots are adopted on the main thread
/// into the session that last requested them, never inside a host callback. Other sessions refresh
/// on their next request. Main-thread only except where noted. Production keeps one instance for
/// the process lifetime; queued work retains it, so deallocation happens only after that work has
/// drained.
@interface KRRuntime : NSObject

/// MODEL when AutomaticCorrectionEnabled is a true NSNumber, else OFF. Values of any other type
/// count as false.
+ (uint32_t)policyFromPreferences:(nullable NSDictionary *)preferences;

/// `preferences` is the application's persistent domain. Under MODEL, EnglishDictionaryPath and
/// RussianDictionaryPath must be absolute NSStrings of at most KR_MAX_PATH_BYTES file-system
/// bytes; an absent key selects ~/.vim/spell/{en,ru}.utf-8.add, where a file that doesn't exist
/// counts as empty, and any other value leaves that language missing, as does a configured
/// file that doesn't exist. The Learn Spelling list in the AppleSpell group container under the
/// home is read too, a missing file counting as empty. `homeResolver` runs once on `workQueue`.
/// `mainExecutor` must run each block later on the main thread. A nonzero `observationInterval`
/// (nanoseconds) adds a periodic observation timer at start. Validates the model asset here.
- (instancetype)initWithPreferences:(nullable NSDictionary *)preferences
                       homeResolver:(NSString *_Nullable (^)(void))homeResolver
                          workQueue:(dispatch_queue_t)workQueue
                       mainExecutor:(void (^)(dispatch_block_t))mainExecutor
                observationInterval:(uint64_t)observationInterval NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

@property(nonatomic, readonly) KRRuntimeStatus status;
/// CompatibilityEnabled, default on. Off turns the manual-only and automatic tiers into Off.
@property(nonatomic, readonly) BOOL compatibilityEnabled;
/// UnlistedAutomaticCorrectionEnabled, default off: unlisted applications get the automatic tier
/// instead of manual only. Wrong-typed values count as false.
@property(nonatomic, readonly) BOOL unlistedAutomaticCorrection;
@property(nonatomic, readonly) BOOL automaticInputSourceSwitching;
/// Valid entries of ApplicationTiers: bundle identifier to "composition", "automatic", "manual"
/// or "off". Other keys and values are ignored, and Keyrill itself can't be overridden.
@property(nonatomic, readonly) NSDictionary<NSString *, NSString *> *tierOverrides;
/// Bundles whose effective tier is composition: the session's admission set.
@property(nonatomic, readonly) NSSet<NSString *> *compositionClients;

/// The built-in tier, or `unlisted` set to YES and the unlisted default. Composition entries
/// match exactly; the others ignore case, and some match a bundle-identifier prefix.
- (KRTier)defaultTierForBundle:(NSString *)bundle unlisted:(BOOL *_Nullable)unlisted;
/// Effective tier: Off for a missing, non-string or empty identifier and for Keyrill itself,
/// then the user override, then the default; compatibility tiers become Off when
/// compatibility is disabled.
- (KRTier)tierForBundle:(nullable id)bundle;
/// The stored value for a tier, as written to ApplicationTiers.
+ (NSString *)nameForTier:(KRTier)tier;

/// Applies the policy to a fresh engine before it is wrapped in a session. It has no
/// dictionaries until a requested adoption runs, so MODEL abstains for keys planned before then.
- (uint32_t)configureEngine:(kr_engine *)engine;
/// Makes `session` the weak adoption target and schedules a deferred adoption. A refusal while
/// a host callback runs, a plan is pending or the session is busy is retried after a later
/// observation; a released target cancels the retry. No-op unless the policy is MODEL.
- (void)requestAdoptionForSession:(id<KRVocabularyAdopting>)session;
/// Configures paths and starts observation. Under OFF nothing is configured, no work is queued
/// and no file is read.
- (void)start;
/// Queues exactly one observation round. Callable from any thread.
- (void)tick;
/// Cancels the timer; queued and in-flight work then finishes without effect. Never waits.
- (void)stop;

@end

NS_ASSUME_NONNULL_END
