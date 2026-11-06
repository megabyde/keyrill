#import "KRSession.h"
#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

/// The process a rewrite may be posted to, captured for one key. `available` is NO unless the
/// guard is live, Secure Input is off and the client and the frontmost application have the
/// session's bundle identifier; `pid` is then the frontmost application's process. The two
/// generations are the guard and input-source counters at capture.
typedef struct {
  pid_t pid;
  uint64_t generation;
  BOOL available;
  uint64_t sourceGeneration;
} KRCompatibilityTarget;

/// What a dispatch submitted, never what the client did with it.
typedef NS_ENUM(uint8_t, KRCompatibilityDispatch) {
  /// No event was posted.
  KRCompatibilityNotStarted,
  /// Every event of the rewrite was posted.
  KRCompatibilitySubmitted,
  /// Some events may have been posted, then a check failed or posting threw.
  KRCompatibilityPartial,
};

/// The compatibility backend: correction for clients that can't host an owned composition. Keys
/// pass to the client and are only observed, so the engine tracks the run typed since the last
/// boundary in bounded memory; a correction rewrites that run by posting Backspace and Unicode
/// key events to the foreground process. It owns no document range and no composition, and it
/// reads nothing back, so its acknowledgements (OBSERVED, DISPATCHED) report submission and are
/// never APPLIED_VERIFIED. A partial dispatch, or one the engine refuses to acknowledge, stops
/// the session until activation, with no replay; a rewrite refused before its first post only
/// drops tracking. Main thread only. An automatic rewrite at Space after typing "ghbdtn":
///
///   targetSnapshot      guard live, Secure Input off, client and frontmost bundle match
///   kr_plan_compatibility_event   REWRITE: delete 7, replacement "привет ", key passes
///   dispatchRewrite     builds 7 Backspaces, then п р и в е т Space, every event tagged,
///                       and posts them; before each post it rechecks the session
///                       generation, Secure Input, the target pid and the guard generation
///   kr_commit_compatibility   DISPATCHED: submitted, not verified
///   handleEvent         returns NO: the typed Space passes, the posted events arrive after
///                       it, and the 7 deletions include it
///   later key-downs     tagged events, and untagged ones matching the posted sequence,
///                       pass without replanning (shouldPassPostedEvent)
///
/// Nothing here can see a change that the host has queued ahead of the posted events, and
/// typing during the rewrite can delete the wrong characters.
@interface KRCompatibilitySession : NSObject <KRVocabularyAdopting>

/// Takes ownership of engine. No event tap is created and no event is posted at initialization.
- (instancetype)initWithEngine:(kr_engine *)engine
              bundleIdentifier:(NSString *)bundle
                     automatic:(BOOL)automatic NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

/// Input mode (KR_EN or KR_RU) of the tracked word; the controller keeps it equal to its own.
@property(nonatomic) uint32_t mode;
/// YES after a dispatch that posted events but wasn't a full, acknowledged submission, after an
/// engine refusal, an invalid plan or an exception. Reserved commands are then consumed and
/// other keys pass untracked until `activate` (never again once the engine is poisoned).
@property(nonatomic, readonly) BOOL stopped;
/// Content-free status line for the input menu.
@property(nonatomic, readonly) NSString *statusTitle;
/// Mode to request after the most recent automatic rewrite was fully dispatched (the other
/// mode), or after a reversal dispatched for a word kept across a mode change (the word's mode);
/// zero otherwise.
@property(nonatomic, readonly) uint32_t automaticSwitchMode;
/// YES for the Automatic tier, where Space can start a rewrite; NO for Manual only.
@property(nonatomic, readonly) BOOL automatic;
/// Injection seams are synchronous. The dispatch block must not retain plan or stillCurrent.
@property(nonatomic, copy) BOOL (^secureInputEnabled)(void);
/// Defaults to the production checks described at `KRCompatibilityTarget`.
@property(nonatomic, copy) KRCompatibilityTarget (^targetSnapshot)
    (NSString *bundle, id<IMKTextInput> client);
/// Defaults to the dispatch behind `dispatchPlan:`, posting with CGEventPostToPid; calls
/// stillCurrent before each post.
@property(nonatomic, copy) KRCompatibilityDispatch (^dispatchRewrite)
    (pid_t pid, const kr_compatibility_plan *plan, BOOL (^stillCurrent)(void));
/// Untagged key-downs seen by the guard tap. An automatic rewrite needs one since the last rewrite.
@property(nonatomic, copy) uint64_t (^observedKeyCount)(void);

/// Outside input callbacks only. Preflights access, never requests it; a missing permission
/// leaves compatibility unavailable and a later call retries. The guard is a listen-only event
/// tap that reads keycodes and modifier flags, never characters, plus the application-activation
/// and input-source notifications; it remains for process lifetime, and a tap the system
/// disables is re-enabled and expires tracking.
+ (BOOL)startGuardIfPermitted;
/// Guard generation. It advances on each guard event: mouse and scroll events, navigation,
/// Forward Delete, Tab, Return, Escape, a shortcut, application activation, an input-source
/// change and a tap the system disabled. An advance expires tracking, except the source change
/// that echoes Keyrill's own mode request.
+ (uint64_t)inputContextGeneration;
/// Count of input-source change notifications. Comparing its advance with the guard
/// generation's tells a lone source change from any other guard event.
+ (uint64_t)inputSourceGeneration;
/// YES for an event carrying Keyrill's source user-data tag. A host can strip the tag, so NO is
/// not proof of real input.
+ (BOOL)isSyntheticEvent:(nullable CGEventRef)event;
/// Passes posted input without replanning, including exact sequence matches when the event tag
/// is stripped. Echoes are optional: the first untagged mismatch ends the expectation and returns
/// NO so that input reaches tracking. A guard change restarts tracking without stopping.
- (BOOL)shouldPassPostedEvent:(nullable CGEventRef)event;
/// Constructs tagged events and calls post synchronously. Every event is built before the first
/// post, so an unsupported unit or a construction failure posts nothing. An injected post
/// function can inspect events without sending them; the production function calls
/// CGEventPostToPid.
+ (KRCompatibilityDispatch)dispatchPlan:(const kr_compatibility_plan *)plan
                              toProcess:(pid_t)pid
                           stillCurrent:(BOOL (^)(void))stillCurrent
                              postEvent:(void (^)(pid_t, CGEventRef))post;

/// Observes one key, or plans and dispatches a rewrite for it. Returns the IMK disposition: YES
/// only for the reserved commands; an ordinary key, including the Space of an automatic
/// rewrite, passes.
- (BOOL)handleEvent:(const kr_event *)event client:(id<IMKTextInput>)client;
/// Starts a new client session: drops tracking and clears a stop unless the engine is poisoned.
- (void)activate;
/// Only after Keyrill requested this mode and IMK echoed it. Keeps the tracked word and its
/// reversal: a reversal in the new mode rewrites in the word's mode, and any other input first
/// restarts tracking in the new mode with the already-observed trailing Space, without posting.
- (void)keepAcrossAutomaticSwitchToMode:(uint32_t)mode;
/// Drops tracking and the expected posted sequence, advances the session generation and resets
/// Rust. Makes no client call and posts nothing.
- (void)invalidate;
@end

NS_ASSUME_NONNULL_END
