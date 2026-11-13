#import <Cocoa/Cocoa.h>
#import <InputMethodKit/InputMethodKit.h>

#include <pwd.h>
#include <unistd.h>
#include <vector>

#import "KRController.h"
#import "KRPreferences.h"
#import "KRRuntime.h"
#import "KRSession.h"

// The account database, not $HOME, names the directory holding the default personal sources
static NSString *accountHome() {
  long size = sysconf(_SC_GETPW_R_SIZE_MAX);
  std::vector<char> buffer(size > 0 ? static_cast<size_t>(size) : 16384);
  passwd entry;
  passwd *result = nullptr;
  if (getpwuid_r(getuid(), &entry, buffer.data(), buffer.size(), &result) != 0 || !result ||
      !result->pw_dir)
    return nil;
  return [NSFileManager.defaultManager stringWithFileSystemRepresentation:result->pw_dir
                                                                   length:strlen(result->pw_dir)];
}

static KRRuntime *runtimeForPreferences(NSDictionary *preferences) {
  return [[KRRuntime alloc] initWithPreferences:preferences
      homeResolver:^NSString * {
        return accountHome();
      }
      workQueue:dispatch_queue_create("org.keyrill.vocabulary",
                                      dispatch_queue_attr_make_with_qos_class(DISPATCH_QUEUE_SERIAL,
                                                                              QOS_CLASS_UTILITY, 0))
      mainExecutor:^(dispatch_block_t block) {
        dispatch_async(dispatch_get_main_queue(), block);
      }
      observationInterval:2 * NSEC_PER_SEC];
}

// Merges `changes` into the persistent domain, keeping unrelated keys; NSNull removes a key.
// The domain is written only after the replacement runtime is accepted.
static BOOL applyPreferences(NSDictionary *changes) {
  if (!NSThread.isMainThread || [KRSession inputCallbackActive])
    return NO;
  NSString *domain = NSBundle.mainBundle.bundleIdentifier;
  NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
  NSDictionary *current = [defaults persistentDomainForName:domain];
  NSMutableDictionary *next = current ? [current mutableCopy] : [NSMutableDictionary dictionary];
  for (NSString *key in changes) {
    if (changes[key] == NSNull.null)
      [next removeObjectForKey:key];
    else
      next[key] = changes[key];
  }
  KRRuntime *replacement = runtimeForPreferences(next);
  if (!replacement || replacement.status.policy == KRPolicyStatusModelUnavailable ||
      ![KeyrillAlphaController setRuntime:replacement])
    return NO;
  [defaults setPersistentDomain:next forName:domain];
  [replacement start];
  return YES;
}

@interface KRApplicationDelegate : NSObject <NSApplicationDelegate>
@end

@implementation KRApplicationDelegate {
  KRPreferencesController *_preferences;
}

- (void)showKeyrillPreferences:(id)sender {
  if (!NSThread.isMainThread)
    return;
  if (!_preferences) {
    NSString *domain = NSBundle.mainBundle.bundleIdentifier;
    _preferences = [[KRPreferencesController alloc]
        initWithPreferences:[NSUserDefaults.standardUserDefaults persistentDomainForName:domain]
           learnedWordsPath:[KRRuntime learnedWordsPathInHome:accountHome()]
                      apply:^BOOL(NSDictionary *preferences) {
                        NSMutableDictionary *changes = [NSMutableDictionary dictionary];
                        for (NSString *key in @[
                               @"AutomaticCorrectionEnabled", @"EnglishDictionaryPath",
                               @"RussianDictionaryPath", @"CompatibilityEnabled",
                               @"UnlistedAutomaticCorrectionEnabled",
                               @"AutomaticInputSourceSwitchingEnabled", @"LearnedWordsEnabled"
                             ])
                          changes[key] = preferences[key] ? preferences[key] : NSNull.null;
                        return applyPreferences(changes);
                      }];
  }
  [_preferences showWindow:sender];
}

- (void)applicationWillTerminate:(NSNotification *)notification {
  (void)notification;
  [[KeyrillAlphaController activeController] finishForShutdown];
}
@end

int main() {
  @autoreleasepool {
    [NSApplication sharedApplication];
    // Registers the controller class before IMK resolves it by name from Info.plist
    (void)KeyrillAlphaController.class;
    __attribute__((objc_precise_lifetime)) KRApplicationDelegate *delegate =
        [KRApplicationDelegate new];
    NSApp.delegate = delegate;
    NSBundle *bundle = NSBundle.mainBundle;
    NSString *connection = [bundle objectForInfoDictionaryKey:@"InputMethodConnectionName"];
    if (!connection || ![bundle.bundleIdentifier isEqualToString:@KR_BUNDLE_IDENTIFIER])
      return 2;
    KRRuntime *runtime = runtimeForPreferences(
        [NSUserDefaults.standardUserDefaults persistentDomainForName:bundle.bundleIdentifier]);
    if (!runtime || ![KeyrillAlphaController setRuntime:runtime])
      return 2;
    KeyrillAlphaController.preferencesUpdater = ^BOOL(NSDictionary *changes) {
      return applyPreferences(changes);
    };
    [runtime start];
    // The server must outlive the run loop
    __attribute__((objc_precise_lifetime)) IMKServer *server =
        [[IMKServer alloc] initWithName:connection bundleIdentifier:bundle.bundleIdentifier];
    if (!server)
      return 2;
    [NSApp run];
  }
  return 0;
}
