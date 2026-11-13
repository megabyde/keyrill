#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

/// Nonmodal alpha settings window over a copy of the persistent-domain snapshot. It never reads
/// dictionary contents or writes defaults: Save hands the edited copy to `apply` on main and
/// closes only when that returns YES. Unrelated keys, including ApplicationTiers, are preserved;
/// AutomaticCorrectionEnabled, AutomaticInputSourceSwitchingEnabled, LearnedWordsEnabled,
/// CompatibilityEnabled and UnlistedAutomaticCorrectionEnabled are written as explicit NSNumbers.
/// Switching and compatibility read as on when absent, the others as off. A path key changes only
/// when its field was edited: an absolute path of at most KR_MAX_PATH_BYTES file-system bytes is
/// stored as a string, an emptied field removes the key, and anything else is refused without
/// applying.
@interface KRPreferencesController : NSWindowController <NSWindowDelegate>

- (instancetype)initWithPreferences:(nullable NSDictionary *)preferences
                   learnedWordsPath:(nullable NSString *)learnedWordsPath
                              apply:(BOOL (^)(NSDictionary *preferences))apply
    NS_DESIGNATED_INITIALIZER;
- (instancetype)initWithWindow:(nullable NSWindow *)window NS_UNAVAILABLE;
- (nullable instancetype)initWithCoder:(NSCoder *)coder NS_UNAVAILABLE;
- (instancetype)init NS_UNAVAILABLE;

/// Returns the file chosen for the named language, or nil when canceled. Defaults to an open
/// panel; replaceable so tests never show a dialog. A language selection only fills its field;
/// "Learn Spelling" must select learnedWordsPath and checks the opt-in box without applying.
@property(nonatomic, copy) NSURL *_Nullable (^chooseFile)(NSString *language);
@property(nonatomic, readonly) NSButton *automaticCorrection;
@property(nonatomic, readonly) NSButton *inputSourceSwitching;
@property(nonatomic, readonly) NSButton *learnedWordsEnabled;
@property(nonatomic, readonly) NSButton *compatibilityEnabled;
@property(nonatomic, readonly) NSButton *unlistedAutomaticCorrection;
/// How compatibility mode edits and what it needs, shown above its options.
@property(nonatomic, readonly) NSTextField *compatibilityNote;
@property(nonatomic, readonly) NSTextField *englishPath;
@property(nonatomic, readonly) NSTextField *russianPath;
@property(nonatomic, readonly) NSButton *chooseEnglish;
@property(nonatomic, readonly) NSButton *chooseRussian;
@property(nonatomic, readonly) NSButton *chooseLearnedWords;
@property(nonatomic, readonly) NSButton *saveButton;
@property(nonatomic, readonly) NSButton *cancelButton;
/// Empty unless the last Save was refused.
@property(nonatomic, readonly) NSTextField *errorLabel;
@property(nonatomic, readonly) NSTextField *scopeLabel;

@end

NS_ASSUME_NONNULL_END
