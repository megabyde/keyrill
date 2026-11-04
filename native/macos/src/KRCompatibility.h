#import "KRSession.h"
#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

typedef struct {
  pid_t pid;
  uint64_t generation;
  BOOL available;
  uint64_t sourceGeneration;
} KRCompatibilityTarget;

typedef NS_ENUM(uint8_t, KRCompatibilityDispatch) {
  KRCompatibilityNotStarted,
  KRCompatibilitySubmitted,
  KRCompatibilityPartial,
};

/// Synthetic rewriting for an automatically selected external client. It owns a bounded input
/// record, never a document range or composition. Main-thread only; submission is not verification.
@interface KRCompatibilitySession : NSObject <KRVocabularyAdopting>

/// Takes ownership of engine. No event tap is created and no event is posted at initialization.
- (instancetype)initWithEngine:(kr_engine *)engine
              bundleIdentifier:(NSString *)bundle
                     automatic:(BOOL)automatic NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

@property(nonatomic) uint32_t mode;
@property(nonatomic, readonly) BOOL stopped;
@property(nonatomic, readonly) NSString *statusTitle;
/// Mode to request after the most recent automatic rewrite was fully dispatched (the other
/// mode), or after a reversal dispatched for a word kept across a mode change (the word's mode);
/// zero otherwise.
@property(nonatomic, readonly) uint32_t automaticSwitchMode;
@property(nonatomic, readonly) BOOL automatic;
/// Injection seams are synchronous. The dispatch block must not retain plan or stillCurrent.
@property(nonatomic, copy) BOOL (^secureInputEnabled)(void);
@property(nonatomic, copy) KRCompatibilityTarget (^targetSnapshot)
    (NSString *bundle, id<IMKTextInput> client);
@property(nonatomic, copy) KRCompatibilityDispatch (^dispatchRewrite)
    (pid_t pid, const kr_compatibility_plan *plan, BOOL (^stillCurrent)(void));
/// Untagged key-downs seen by the guard tap. An automatic rewrite needs one since the last rewrite.
@property(nonatomic, copy) uint64_t (^observedKeyCount)(void);

/// Outside input callbacks only. Preflights access, never requests it; a missing permission
/// leaves compatibility unavailable and a later call retries. The metadata-only guard remains
/// for process lifetime; a tap the system disables is re-enabled and expires tracking.
+ (BOOL)startGuardIfPermitted;
+ (uint64_t)inputContextGeneration;
+ (uint64_t)inputSourceGeneration;
+ (BOOL)isSyntheticEvent:(nullable CGEventRef)event;
/// Passes posted input without replanning, including exact sequence matches when the event tag
/// is stripped. Echoes are optional: the first untagged mismatch ends the expectation and returns
/// NO so that input reaches tracking. A guard change restarts tracking without stopping.
- (BOOL)shouldPassPostedEvent:(nullable CGEventRef)event;
/// Constructs tagged events and calls post synchronously. An injected post function can inspect
/// events without sending them; the production function calls CGEventPostToPid.
+ (KRCompatibilityDispatch)dispatchPlan:(const kr_compatibility_plan *)plan
                              toProcess:(pid_t)pid
                           stillCurrent:(BOOL (^)(void))stillCurrent
                              postEvent:(void (^)(pid_t, CGEventRef))post;

- (BOOL)handleEvent:(const kr_event *)event client:(id<IMKTextInput>)client;
- (void)activate;
/// Only after Keyrill requested this mode and IMK echoed it. Keeps the tracked word and its
/// reversal: a reversal in the new mode rewrites in the word's mode, and any other input first
/// restarts tracking in the new mode with the already-observed trailing Space, without posting.
- (void)keepAcrossAutomaticSwitchToMode:(uint32_t)mode;
- (void)invalidate;
@end

NS_ASSUME_NONNULL_END
