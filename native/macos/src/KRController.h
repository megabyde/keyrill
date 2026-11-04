#import <InputMethodKit/InputMethodKit.h>

NS_ASSUME_NONNULL_BEGIN

@class KRCompatibilitySession;
@class KRRuntime;

@interface KeyrillAlphaController : IMKInputController
/// Main thread, outside input callbacks. Stops the old observer without waiting for its queue.
/// Controllers apply the replacement at their next activation; poisoned sessions stay stopped.
+ (BOOL)setRuntime:(KRRuntime *)runtime;
/// The most recently activated controller, if it is still alive.
+ (nullable instancetype)activeController;
/// Persists preference changes and installs a runtime built from the result; NSNull removes a
/// key. Runs on main outside input callbacks and returns NO when nothing was applied. Nil
/// refuses per-application tier changes from the input menu.
@property(class, nonatomic, copy, nullable) BOOL (^preferencesUpdater)(NSDictionary *changes);
/// The input-source identifier IMK uses for a Keyrill mode, derived from the bundle identifier.
+ (NSString *)inputModeForMode:(uint32_t)mode;
- (void)finishForShutdown;

/// Replaceable for controlled tests. Secure Input defaults to `IsSecureEventInputEnabled` and
/// is passed to the sessions this controller creates.
@property(nonatomic, copy) BOOL (^secureInputEnabled)(void);
/// Called with each compatibility session this controller creates, before first use; nil by
/// default.
@property(nonatomic, copy, nullable) void (^configureCompatibility)(KRCompatibilitySession *session)
    ;
/// Defaults to `+[KRCompatibilitySession startGuardIfPermitted]`. Runs on main after an
/// activation that selects a compatibility tier, outside the activation callback.
@property(nonatomic, copy) void (^startGuard)(void);
@end

NS_ASSUME_NONNULL_END
