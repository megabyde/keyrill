// Controlled native tests: KRRuntime configuration, source acquisition and adoption against the
// real Rust engine. Observation rounds and main-thread work run only when a test drains them;
// there is no timer, host application or input source. Controlled-native evidence only.
#import <Foundation/Foundation.h>

#include <cstdio>
#include <string>
#include <sys/stat.h>

#import "../src/KRRuntime.h"
#import "../src/KRSession.h"

static int failures = 0;
static const char *currentTest = "";

#define CHECK(condition)                                                                           \
  do {                                                                                             \
    if (!(condition)) {                                                                            \
      ++failures;                                                                                  \
      std::fprintf(stderr, "%s:%d: [%s] CHECK failed: %s\n", __FILE__, __LINE__, currentTest,      \
                   #condition);                                                                    \
    }                                                                                              \
  } while (0)

static NSString *const kModel = @"AutomaticCorrectionEnabled";
static NSString *const kEnglishPath = @"EnglishDictionaryPath";
static NSString *const kRussianPath = @"RussianDictionaryPath";

/// Runs the runtime's work queue and main-thread blocks only when drained, on the main thread.
@interface Harness : NSObject
@property(nonatomic, readonly) dispatch_queue_t queue;
@property(nonatomic, copy) NSString *home;
@property(nonatomic, readonly) NSUInteger homeCalls;
@property(nonatomic, readonly) BOOL homeOnMain;
@property(nonatomic, readonly) NSUInteger posts;
- (KRRuntime *)runtimeWithPreferences:(NSDictionary *)preferences;
- (void)drain;
@end

@implementation Harness {
  NSMutableArray<dispatch_block_t> *_pending;
}

- (instancetype)init {
  if ((self = [super init])) {
    _queue = dispatch_queue_create("org.keyrill.test.runtime", DISPATCH_QUEUE_SERIAL);
    _pending = [NSMutableArray array];
  }
  return self;
}

- (KRRuntime *)runtimeWithPreferences:(NSDictionary *)preferences {
  __weak Harness *weakSelf = self;
  return [[KRRuntime alloc] initWithPreferences:preferences
      homeResolver:^NSString * {
        Harness *harness = weakSelf;
        ++harness->_homeCalls;
        harness->_homeOnMain = harness->_homeOnMain || NSThread.isMainThread;
        return harness.home;
      }
      workQueue:_queue
      mainExecutor:^(dispatch_block_t block) {
        Harness *harness = weakSelf;
        @synchronized(harness) {
          ++harness->_posts;
          [harness->_pending addObject:block];
        }
      }
      observationInterval:0];
}

// Alternates between the work queue and posted main blocks until neither has work
- (void)drain {
  for (;;) {
    dispatch_sync(_queue, ^{
                  });
    NSArray<dispatch_block_t> *blocks;
    @synchronized(self) {
      blocks = [_pending copy];
      [_pending removeAllObjects];
    }
    if (!blocks.count)
      return;
    for (dispatch_block_t block in blocks)
      block();
  }
}

@end

namespace {

NSString *testRoot;

NSString *directory(NSString *name) {
  NSString *path = [testRoot stringByAppendingPathComponent:name];
  CHECK([NSFileManager.defaultManager createDirectoryAtPath:path
                                withIntermediateDirectories:YES
                                                 attributes:nil
                                                      error:nil]);
  return path;
}

// Atomic writes replace the inode, so each rewrite is a new candidate whatever the clock says
void write(NSString *path, const std::string &bytes) {
  CHECK([[NSData dataWithBytes:bytes.data() length:bytes.size()] writeToFile:path atomically:YES]);
}

bool sameStatus(KRRuntimeStatus status, KRPolicyStatus policy, KRSourceStatus english,
                KRSourceStatus russian) {
  return status.policy == policy && status.english == english && status.russian == russian;
}

// The session owns the engine; `engine` stays borrowed for direct plans while the session idles
KRSession *makeSession(KRRuntime *runtime, kr_engine *&engine) {
  engine = nullptr;
  if (kr_engine_new(KR_ABI_VERSION, &engine) != KR_OK || !engine)
    return nil;
  if ([runtime configureEngine:engine] != KR_OK) {
    kr_engine_free(engine);
    engine = nullptr;
    return nil;
  }
  return [[KRSession alloc] initWithEngine:engine allowedClients:[NSSet set]];
}

// Plans `word` and then `last` from a word start in EN, acknowledging every mark, and returns the
// final mark, or nil when the engine does not mark. Leaves the engine idle.
NSString *finalMark(kr_engine *engine, NSString *word, uint32_t lastRole, unichar last) {
  NSString *mark = nil;
  for (NSUInteger i = 0; i <= word.length; ++i) {
    bool final = i == word.length;
    kr_event event = {final ? lastRole : KR_TEXT,
                      KR_EN,
                      i ? KR_CTX_INELIGIBLE : KR_CTX_BOUNDARY,
                      final && lastRole != KR_TEXT ? 0u : 1u,
                      {}};
    event.units[0] = final ? last : [word characterAtIndex:i];
    kr_plan plan;
    if (kr_plan_event(engine, &event, &plan) != KR_OK || plan.action != KR_MARK ||
        kr_commit(engine, plan.plan_id, KR_APPLIED_VERIFIED) != KR_OK) {
      mark = nil;
      break;
    }
    mark = [NSString stringWithCharacters:plan.mark length:plan.mark_len];
  }
  CHECK(kr_reset(engine) == KR_OK);
  return mark;
}

NSString *afterSpace(kr_engine *engine, NSString *word) {
  return finalMark(engine, word, KR_TEXT, u' ');
}

NSString *afterManual(kr_engine *engine, NSString *word) {
  return finalMark(engine, word, KR_MANUAL, 0);
}

void testPolicyResolution() {
  currentTest = "policy_resolution";
  struct Case {
    NSDictionary *preferences;
    uint32_t policy;
  };
  const Case cases[] = {
      {nil, KR_POLICY_OFF},
      {@{}, KR_POLICY_OFF},
      {@{kModel : @YES}, KR_POLICY_MODEL},
      {@{kModel : @NO}, KR_POLICY_OFF},
      {@{kModel : @"YES"}, KR_POLICY_OFF},
      {@{kModel : @[ @YES ]}, KR_POLICY_OFF},
  };
  for (const Case &c : cases)
    CHECK([KRRuntime policyFromPreferences:c.preferences] == c.policy);
}

// OFF never resolves a home, configures a path, queues work or wakes the main thread, even with
// an existing explicit path setting
void testOffDoesNoSourceWork() {
  currentTest = "off_no_source_work";
  NSString *root = directory(@"inactive");
  NSString *english = [root stringByAppendingPathComponent:@"en.utf-8.add"];
  write(english, "");
  Harness *harness = [Harness new];
  harness.home = root;
  KRRuntime *runtime = [harness runtimeWithPreferences:@{kModel : @NO, kEnglishPath : english}];
  kr_engine *engine;
  KRSession *session = makeSession(runtime, engine);
  CHECK(session);
  [runtime start];
  [runtime requestAdoptionForSession:session];
  [runtime tick];
  [harness drain];
  CHECK(harness.homeCalls == 0 && harness.posts == 0);
  CHECK(sameStatus(runtime.status, KRPolicyStatusOff, KRSourceStatusUnconfigured,
                   KRSourceStatusUnconfigured));
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [runtime stop];
}

// Explicit paths need no home. Two equal reads publish; an unchanged read wakes nothing; the
// requested session adopts the publication and its next decision uses both sources.
void testExplicitSourcesPublishAndAdopt() {
  currentTest = "explicit_sources_publish_and_adopt";
  NSString *root = directory(@"explicit");
  NSString *english = [root stringByAppendingPathComponent:@"en"];
  NSString *russian = [root stringByAppendingPathComponent:@"ru"];
  write(english, "");
  write(russian, "");
  Harness *harness = [Harness new];
  KRRuntime *runtime = [harness runtimeWithPreferences:@{
    kModel : @YES,
    kEnglishPath : english,
    kRussianPath : russian
  }];
  kr_engine *engine;
  KRSession *session = makeSession(runtime, engine);
  [runtime requestAdoptionForSession:session];
  [runtime start];
  [harness drain];
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusPending,
                   KRSourceStatusPending));
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [runtime tick];
  [harness drain];
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusReady, KRSourceStatusReady));
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"привет "]);
  NSUInteger posts = harness.posts;
  [runtime tick];
  [harness drain];
  CHECK(harness.posts == posts && harness.homeCalls == 0);
  [runtime stop];
}

// Absent path settings resolve the account home once, off main, and read the canonical files
void testDefaultPathsFromHome() {
  currentTest = "default_paths_from_home";
  NSString *home = directory(@"home");
  NSString *spell = directory(@"home/.vim/spell");
  write([spell stringByAppendingPathComponent:@"en.utf-8.add"], "ghbdtn\n");
  write([spell stringByAppendingPathComponent:@"ru.utf-8.add"], "");
  Harness *harness = [Harness new];
  harness.home = home;
  KRRuntime *runtime = [harness runtimeWithPreferences:@{kModel : @YES}];
  kr_engine *engine;
  KRSession *session = makeSession(runtime, engine);
  [runtime start];
  [harness drain];
  [runtime tick];
  [harness drain];
  [runtime requestAdoptionForSession:session];
  [harness drain];
  CHECK(harness.homeCalls == 1 && !harness.homeOnMain);
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusReady, KRSourceStatusReady));
  // The EN entry protects the original; manual conversion still overrides it
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  CHECK([afterManual(engine, @"ghbdtn") isEqualToString:@"привет"]);
  [runtime stop];
}

// A present but unusable path setting never falls back to the canonical file, which exists and is
// valid here; only the language whose setting is absent reads its default
void testUnusablePathSettingsDoNotDiscover() {
  currentTest = "unusable_path_settings";
  NSString *home = directory(@"discover");
  NSString *spell = directory(@"discover/.vim/spell");
  write([spell stringByAppendingPathComponent:@"en.utf-8.add"], "");
  write([spell stringByAppendingPathComponent:@"ru.utf-8.add"], "");
  NSString *tooLong = [@"/" stringByPaddingToLength:KR_MAX_PATH_BYTES + 1
                                         withString:@"a"
                                    startingAtIndex:0];
  NSString *embeddedNul = [[spell stringByAppendingPathComponent:@"en.utf-8.add"]
      stringByAppendingFormat:@"%Cignored", static_cast<unichar>(0)];
  for (id english in @[ @42, @"relative/en.utf-8.add", @"", tooLong, embeddedNul ]) {
    Harness *harness = [Harness new];
    harness.home = home;
    KRRuntime *runtime = [harness runtimeWithPreferences:@{kModel : @YES, kEnglishPath : english}];
    kr_engine *engine;
    KRSession *session = makeSession(runtime, engine);
    CHECK(runtime.status.english == KRSourceStatusMissing);
    // A round queued before the home is resolved observes nothing and reports nothing
    [runtime start];
    [runtime tick];
    [runtime requestAdoptionForSession:session];
    [harness drain];
    CHECK(harness.homeCalls == 1);
    CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusMissing,
                     KRSourceStatusPending));
    [runtime tick];
    [harness drain];
    CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusMissing,
                     KRSourceStatusReady));
    CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
    CHECK([afterManual(engine, @"ghbdtn") isEqualToString:@"привет"]);
    [runtime stop];
  }
  Harness *harness = [Harness new];
  harness.home = home;
  KRRuntime *runtime = [harness runtimeWithPreferences:@{
    kModel : @YES,
    kEnglishPath : @1,
    kRussianPath : @2
  }];
  [runtime start];
  [runtime tick];
  [harness drain];
  CHECK(harness.homeCalls == 0 && harness.posts == 0);
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusMissing,
                   KRSourceStatusMissing));
  [runtime stop];
}

void testUnresolvedHomeAndMissingFile() {
  currentTest = "unresolved_home_and_missing_file";
  Harness *harness = [Harness new];
  KRRuntime *runtime = [harness runtimeWithPreferences:@{kModel : @YES}];
  kr_engine *engine;
  KRSession *session = makeSession(runtime, engine);
  [runtime start];
  [runtime requestAdoptionForSession:session];
  [runtime tick];
  [harness drain];
  CHECK(harness.homeCalls == 1);
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusMissing,
                   KRSourceStatusMissing));
  CHECK([afterManual(engine, @"ghbdtn") isEqualToString:@"привет"]);
  [runtime stop];

  NSString *root = directory(@"missing");
  NSString *english = [root stringByAppendingPathComponent:@"en"];
  write(english, "");
  harness = [Harness new];
  runtime = [harness runtimeWithPreferences:@{
    kModel : @YES,
    kEnglishPath : english,
    kRussianPath : [root stringByAppendingPathComponent:@"absent"]
  }];
  session = makeSession(runtime, engine);
  [runtime start];
  [runtime tick];
  [runtime requestAdoptionForSession:session];
  [harness drain];
  CHECK(
      sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusReady, KRSourceStatusMissing));
  // One missing language keeps automatic correction abstaining; the command still converts
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  CHECK([afterManual(engine, @"ghbdtn") isEqualToString:@"привет"]);
  [runtime stop];
}

// A rejected rewrite keeps the adopted protection; a later valid rewrite replaces it
void testReloadKeepsLastGood() {
  currentTest = "reload_keeps_last_good";
  NSString *root = directory(@"reload");
  NSString *english = [root stringByAppendingPathComponent:@"en"];
  NSString *russian = [root stringByAppendingPathComponent:@"ru"];
  write(english, "ghbdtn\n");
  write(russian, "");
  Harness *harness = [Harness new];
  KRRuntime *runtime = [harness runtimeWithPreferences:@{
    kModel : @YES,
    kEnglishPath : english,
    kRussianPath : russian
  }];
  kr_engine *engine;
  KRSession *session = makeSession(runtime, engine);
  [runtime requestAdoptionForSession:session];
  [runtime start];
  [runtime tick];
  [harness drain];
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);

  write(english, "/encoding=latin1\n");
  [runtime tick];
  [runtime tick];
  [harness drain];
  CHECK(
      sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusRetained, KRSourceStatusReady));
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);

  write(english, "");
  [runtime tick];
  [harness drain];
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusReady, KRSourceStatusReady));
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [runtime tick];
  [harness drain];
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"привет "]);
  [runtime stop];
}

struct Published {
  Harness *harness;
  KRRuntime *runtime;
};

// Both sources published, with no adoption target yet
Published publishedRuntime(NSString *name) {
  NSString *root = directory(name);
  NSString *english = [root stringByAppendingPathComponent:@"en"];
  NSString *russian = [root stringByAppendingPathComponent:@"ru"];
  write(english, "");
  write(russian, "");
  Harness *harness = [Harness new];
  KRRuntime *runtime = [harness runtimeWithPreferences:@{
    kModel : @YES,
    kEnglishPath : english,
    kRussianPath : russian
  }];
  [runtime start];
  [runtime tick];
  [harness drain];
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusReady, KRSourceStatusReady));
  return {harness, runtime};
}

// A request inside a host callback or against a pending plan is refused and retried by the next
// observation round even though the sources did not change
void testRefusedAdoptionRetries() {
  currentTest = "refused_adoption_retries";
  Published published = publishedRuntime(@"retry");
  Harness *harness = published.harness;
  KRRuntime *runtime = published.runtime;
  kr_engine *engine;
  KRSession *session = makeSession(runtime, engine);
  [KRSession beginInputCallback];
  [runtime requestAdoptionForSession:session];
  [harness drain];
  [KRSession endInputCallback];
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [harness drain];
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [runtime tick];
  [harness drain];
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"привет "]);

  kr_engine *pendingEngine;
  KRSession *pendingSession = makeSession(runtime, pendingEngine);
  kr_event event = {KR_TEXT, KR_EN, KR_CTX_BOUNDARY, 1, {u'g'}};
  kr_plan plan;
  CHECK(kr_plan_event(pendingEngine, &event, &plan) == KR_OK && plan.action == KR_MARK);
  [runtime requestAdoptionForSession:pendingSession];
  [harness drain];
  CHECK(kr_commit(pendingEngine, plan.plan_id, KR_APPLIED_VERIFIED) == KR_OK);
  CHECK(kr_reset(pendingEngine) == KR_OK);
  CHECK([afterSpace(pendingEngine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [runtime tick];
  [harness drain];
  CHECK([afterSpace(pendingEngine, @"ghbdtn") isEqualToString:@"привет "]);
  NSUInteger posts = harness.posts;
  [runtime tick];
  [harness drain];
  CHECK(harness.posts == posts);
  [runtime stop];
}

void testLatestTargetOwnsDeferredAdoption() {
  currentTest = "latest_target_owns_deferred_adoption";
  Published published = publishedRuntime(@"latest-target");
  kr_engine *firstEngine, *secondEngine;
  KRSession *first = makeSession(published.runtime, firstEngine);
  KRSession *second = makeSession(published.runtime, secondEngine);
  [published.runtime requestAdoptionForSession:first];
  [published.runtime requestAdoptionForSession:second];
  [published.harness drain];
  CHECK([afterSpace(firstEngine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  CHECK([afterSpace(secondEngine, @"ghbdtn") isEqualToString:@"привет "]);
  [published.runtime requestAdoptionForSession:first];
  [published.harness drain];
  CHECK([afterSpace(firstEngine, @"ghbdtn") isEqualToString:@"привет "]);
  CHECK([afterSpace(secondEngine, @"ghbdtn") isEqualToString:@"привет "]);

  [published.runtime requestAdoptionForSession:second];
  [published.harness drain];
  write([testRoot stringByAppendingPathComponent:@"latest-target/en"], "ghbdtn\n");
  [published.runtime tick];
  [published.harness drain];
  [published.runtime tick];
  [published.harness drain];
  // Only the latest requester gets the new protection; the other engine keeps its old view
  CHECK([afterSpace(firstEngine, @"ghbdtn") isEqualToString:@"привет "]);
  CHECK([afterSpace(secondEngine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [published.runtime requestAdoptionForSession:first];
  [published.harness drain];
  CHECK([afterSpace(firstEngine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  CHECK([afterSpace(secondEngine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [published.runtime stop];
}

// A refused request whose session is then released stops retrying instead of waking main forever
void testReleasedTargetCancelsRetry() {
  currentTest = "released_target_cancels_retry";
  Published published = publishedRuntime(@"released");
  Harness *harness = published.harness;
  KRRuntime *runtime = published.runtime;
  @autoreleasepool {
    kr_engine *engine;
    KRSession *session = makeSession(runtime, engine);
    [KRSession beginInputCallback];
    [runtime requestAdoptionForSession:session];
    [harness drain];
    [KRSession endInputCallback];
  }
  NSUInteger posts = harness.posts;
  [runtime tick];
  [harness drain];
  CHECK(harness.posts == posts + 1);
  [runtime tick];
  [harness drain];
  CHECK(harness.posts == posts + 1);
  [runtime stop];
}

// Work queued before stop finishes without resolving a home, reading, publishing or adopting
void testStopSuppressesLateWork() {
  currentTest = "stop_suppresses_late_work";
  NSString *home = directory(@"late");
  Harness *harness = [Harness new];
  harness.home = home;
  KRRuntime *runtime = [harness runtimeWithPreferences:@{kModel : @YES}];
  dispatch_suspend(harness.queue);
  [runtime start];
  [runtime stop];
  dispatch_resume(harness.queue);
  [harness drain];
  CHECK(harness.homeCalls == 0 && harness.posts == 0);
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusPending,
                   KRSourceStatusPending));

  // A rejected rewrite observed after stop would have shown as retained
  Published published = publishedRuntime(@"late-observation");
  harness = published.harness;
  runtime = published.runtime;
  write([testRoot stringByAppendingPathComponent:@"late-observation/en"], "/encoding=latin1\n");
  NSUInteger posts = harness.posts;
  dispatch_suspend(harness.queue);
  [runtime tick];
  [runtime stop];
  dispatch_resume(harness.queue);
  [harness drain];
  CHECK(harness.posts == posts);
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusReady, KRSourceStatusReady));

  published = publishedRuntime(@"late-main");
  harness = published.harness;
  runtime = published.runtime;
  kr_engine *engine;
  KRSession *session = makeSession(runtime, engine);
  [runtime requestAdoptionForSession:session];
  // The posted adoption block runs only after stop
  [runtime stop];
  [harness drain];
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
}

void testCompatibilityDefaultsAndExplicitFlags() {
  currentTest = "compatibility_defaults_and_flags";
  Harness *h = [Harness new];
  KRRuntime *defaults = [h runtimeWithPreferences:@{}];
  CHECK(defaults.compatibilityEnabled && !defaults.unlistedAutomaticCorrection);
  CHECK(defaults.automaticInputSourceSwitching);
  CHECK(defaults.status.policy == KRPolicyStatusOff);
  for (id value in @[ @NO, @YES, @0, @1, @"YES", NSNull.null ]) {
    BOOL expected = [value isKindOfClass:NSNumber.class] && [value boolValue];
    KRRuntime *enabled = [h runtimeWithPreferences:@{@"CompatibilityEnabled" : value}];
    CHECK(enabled.compatibilityEnabled == expected);
    CHECK(!enabled.unlistedAutomaticCorrection);
    KRRuntime *automatic =
        [h runtimeWithPreferences:@{@"UnlistedAutomaticCorrectionEnabled" : value}];
    CHECK(automatic.compatibilityEnabled);
    CHECK(automatic.unlistedAutomaticCorrection == expected);
    KRRuntime *switching =
        [h runtimeWithPreferences:@{@"AutomaticInputSourceSwitchingEnabled" : value}];
    CHECK(switching.automaticInputSourceSwitching == expected);
    CHECK(switching.compatibilityEnabled && !switching.unlistedAutomaticCorrection);
    CHECK(enabled.automaticInputSourceSwitching && automatic.automaticInputSourceSwitching);
  }
  // The retired key no longer turns on automatic compatibility
  KRRuntime *retired = [h runtimeWithPreferences:@{
    @"CompatibilityAutomaticCorrectionEnabled" : @YES
  }];
  CHECK(!retired.unlistedAutomaticCorrection);
  CHECK([retired tierForBundle:@"com.example.editor"] == KRTierManual);
  CHECK(h.homeCalls == 0 && h.posts == 0);
}

void testTierTable() {
  currentTest = "tier_table";
  Harness *h = [Harness new];
  KRRuntime *runtime = [h runtimeWithPreferences:@{}];
  struct Case {
    NSString *bundle;
    KRTier tier;
  };
  const Case cases[] = {
      {@"com.apple.TextEdit", KRTierComposition},
      {@"com.apple.Safari", KRTierComposition},
      {@"org.mozilla.firefox", KRTierComposition},
      // Composition needs the exact identifier; near misses are unlisted
      {@"com.apple.textedit", KRTierManual},
      {@"com.apple.SafariTechnologyPreview", KRTierManual},
      {@"org.mozilla.firefoxdeveloperedition", KRTierManual},
      {@" com.apple.Safari", KRTierManual},
      {@"com.apple.Terminal", KRTierManual},
      {@"com.googlecode.iterm2", KRTierManual},
      {@"com.mitchellh.ghostty", KRTierManual},
      {@"com.umputun.agterm", KRTierManual},
      {@"com.umputun.agterm.debug", KRTierManual},
      {@"com.apple.dt.Xcode", KRTierManual},
      {@"com.microsoft.VSCode", KRTierManual},
      {@"com.jetbrains.CLion", KRTierManual},
      {@"com.jetbrains.pycharm", KRTierManual},
      {@"com.microsoft.Word", KRTierManual},
      {@"com.microsoft.Powerpoint", KRTierManual},
      {@"com.1password.1password", KRTierOff},
      {@"com.apple.Passwords", KRTierOff},
      {@"com.apple.keychainaccess", KRTierOff},
      {@"com.apple.ScreenSharing", KRTierOff},
      {@"com.microsoft.rdc.macos", KRTierOff},
      {@"com.parallels.desktop.console", KRTierOff},
      {@"com.vmware.fusion", KRTierOff},
      // Case differences still match outside composition
      {@"COM.UTMAPP.UTM", KRTierOff},
      {@KR_BUNDLE_IDENTIFIER, KRTierOff},
      {[@KR_BUNDLE_IDENTIFIER uppercaseString], KRTierOff},
      {@"com.example.editor", KRTierManual},
      // A prefix entry needs its dot
      {@"com.jetbrainsx", KRTierManual},
  };
  for (const Case &c : cases)
    CHECK([runtime tierForBundle:c.bundle] == c.tier);
  for (id bundle in @[ @"", NSNull.null, @1, @[] ])
    CHECK([runtime tierForBundle:bundle] == KRTierOff);
  CHECK([runtime tierForBundle:nil] == KRTierOff);
  BOOL unlisted = NO;
  CHECK([runtime defaultTierForBundle:@"com.example.editor" unlisted:&unlisted] == KRTierManual &&
        unlisted);
  CHECK([runtime defaultTierForBundle:@"com.apple.Terminal" unlisted:&unlisted] == KRTierManual &&
        !unlisted);
  NSSet *builtIn = [NSSet
      setWithObjects:@"com.apple.TextEdit", @"com.apple.Safari", @"org.mozilla.firefox", nil];
  CHECK([runtime.compositionClients isEqualToSet:builtIn]);

  KRRuntime *automatic = [h runtimeWithPreferences:@{@"UnlistedAutomaticCorrectionEnabled" : @YES}];
  CHECK([automatic tierForBundle:@"com.example.editor"] == KRTierAutomatic);
  CHECK([automatic tierForBundle:@"com.apple.Terminal"] == KRTierManual);
  CHECK([automatic tierForBundle:@"com.apple.TextEdit"] == KRTierComposition);

  // Compatibility off turns compatibility tiers off and never into composition
  for (NSDictionary *extra in @[ @{}, @{@"UnlistedAutomaticCorrectionEnabled" : @YES} ]) {
    NSMutableDictionary *preferences = [extra mutableCopy];
    preferences[@"CompatibilityEnabled"] = @NO;
    KRRuntime *off = [h runtimeWithPreferences:preferences];
    for (NSString *bundle in @[
           @"com.example.editor", @"com.umputun.agterm", @"com.apple.Terminal",
           @"com.microsoft.Word"
         ])
      CHECK([off tierForBundle:bundle] == KRTierOff);
    CHECK([off tierForBundle:@"com.apple.Safari"] == KRTierComposition);
    CHECK(![off.compositionClients containsObject:@"com.example.editor"]);
  }

  KRRuntime *overridden = [h runtimeWithPreferences:@{
    @"ApplicationTiers" : @{
      @"com.example.editor" : @"composition",
      @"com.apple.TextEdit" : @"off",
      @"com.apple.Terminal" : @"automatic",
      @"com.1password.1password" : @"manual",
      @KR_BUNDLE_IDENTIFIER : @"composition",
      @"com.example.bad" : @"Composition",
      @"com.example.number" : @1,
      @"" : @"composition",
    }
  }];
  CHECK([overridden tierForBundle:@"com.example.editor"] == KRTierComposition);
  CHECK([overridden tierForBundle:@"com.apple.TextEdit"] == KRTierOff);
  CHECK([overridden tierForBundle:@"com.apple.Terminal"] == KRTierAutomatic);
  CHECK([overridden tierForBundle:@"com.1password.1password"] == KRTierManual);
  CHECK([overridden tierForBundle:@KR_BUNDLE_IDENTIFIER] == KRTierOff);
  CHECK([overridden tierForBundle:@"com.example.bad"] == KRTierManual);
  CHECK([overridden tierForBundle:@"com.example.number"] == KRTierManual);
  NSDictionary *valid = @{
    @"com.example.editor" : @"composition",
    @"com.apple.TextEdit" : @"off",
    @"com.apple.Terminal" : @"automatic",
    @"com.1password.1password" : @"manual",
  };
  CHECK([overridden.tierOverrides isEqualToDictionary:valid]);
  NSSet *composition = [NSSet
      setWithObjects:@"com.example.editor", @"com.apple.Safari", @"org.mozilla.firefox", nil];
  CHECK([overridden.compositionClients isEqualToSet:composition]);
  // Overrides are exact: a case variant falls back to the built-in tier
  CHECK([overridden tierForBundle:@"com.example.EDITOR"] == KRTierManual);
  KRRuntime *malformed = [h runtimeWithPreferences:@{@"ApplicationTiers" : @[ @"off" ]}];
  CHECK(malformed.tierOverrides.count == 0);
  CHECK(h.homeCalls == 0 && h.posts == 0);
}

// Only the default location may be absent: it reads as empty, so automatic correction runs
void testMissingDefaultFilesAreEmpty() {
  currentTest = "missing_default_files_are_empty";
  NSString *home = directory(@"empty-home");
  Harness *harness = [Harness new];
  harness.home = home;
  KRRuntime *runtime = [harness runtimeWithPreferences:@{kModel : @YES}];
  kr_engine *engine;
  KRSession *session = makeSession(runtime, engine);
  [runtime start];
  [runtime requestAdoptionForSession:session];
  [harness drain];
  CHECK(sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusReady, KRSourceStatusReady));
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"привет "]);

  // A file created at the default path then protects its words
  NSString *spell = directory(@"empty-home/.vim/spell");
  write([spell stringByAppendingPathComponent:@"en.utf-8.add"], "ghbdtn\n");
  [runtime tick];
  [runtime tick];
  [harness drain];
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [runtime stop];

  // The same absent file at a configured path keeps that language missing
  harness = [Harness new];
  runtime = [harness runtimeWithPreferences:@{
    kModel : @YES,
    kEnglishPath : [home stringByAppendingPathComponent:@"absent.add"],
  }];
  harness.home = home;
  session = makeSession(runtime, engine);
  [runtime start];
  [runtime requestAdoptionForSession:session];
  [harness drain];
  CHECK(
      sameStatus(runtime.status, KRPolicyStatusModel, KRSourceStatusMissing, KRSourceStatusReady));
  CHECK([afterSpace(engine, @"ghbdtn") isEqualToString:@"ghbdtn "]);
  [runtime stop];
}

} // namespace

int main() {
  @autoreleasepool {
    if (!NSThread.isMainThread || kr_abi_version() != KR_ABI_VERSION) {
      std::fprintf(stderr, "runtime_tests: wrong thread or ABI\n");
      return 2;
    }
    testRoot = [NSTemporaryDirectory()
        stringByAppendingPathComponent:[NSString stringWithFormat:@"keyrill-runtime-%d",
                                                                  NSProcessInfo.processInfo
                                                                      .processIdentifier]];
    if (mkdir(testRoot.fileSystemRepresentation, 0700) != 0) {
      std::fprintf(stderr, "runtime_tests: cannot create %s\n", testRoot.fileSystemRepresentation);
      return 2;
    }
    testPolicyResolution();
    testCompatibilityDefaultsAndExplicitFlags();
    testTierTable();
    testMissingDefaultFilesAreEmpty();
    testOffDoesNoSourceWork();
    testExplicitSourcesPublishAndAdopt();
    testDefaultPathsFromHome();
    testUnusablePathSettingsDoNotDiscover();
    testUnresolvedHomeAndMissingFile();
    testReloadKeepsLastGood();
    testRefusedAdoptionRetries();
    testLatestTargetOwnsDeferredAdoption();
    testReleasedTargetCancelsRetry();
    testStopSuppressesLateWork();
    CHECK([NSFileManager.defaultManager removeItemAtPath:testRoot error:nil]);
  }
  if (failures) {
    std::fprintf(stderr, "runtime_tests: %d failure(s)\n", failures);
    return 1;
  }
  std::printf("runtime_tests: passed\n");
  return 0;
}
