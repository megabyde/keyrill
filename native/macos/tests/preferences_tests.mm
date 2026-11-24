// Controlled native tests: the settings window's edited copy, driven through its controls. No
// window is shown, no dialog opens and no defaults or files are touched.
#import <Cocoa/Cocoa.h>

#include <cstdio>

#include "keyrill.h"

#import "../src/KRPreferences.h"

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
static NSString *const kEnglish = @"EnglishDictionaryPath";
static NSString *const kRussian = @"RussianDictionaryPath";
static NSString *const kCompatibility = @"CompatibilityEnabled";
static NSString *const kUnlistedModel = @"UnlistedAutomaticCorrectionEnabled";
static NSString *const kSwitching = @"AutomaticInputSourceSwitchingEnabled";

namespace {

// Records each dictionary the window hands to apply, answers with `result`, and notes any call
// made off the main thread
struct Applied {
  NSMutableArray<NSDictionary *> *calls = [NSMutableArray array];
  BOOL result = YES;
  BOOL offMain = NO;
};

// The file chooser fails the test unless a test replaces it, so no open panel can appear
KRPreferencesController *makeController(NSDictionary *preferences, Applied &applied) {
  Applied *record = &applied;
  KRPreferencesController *controller = [[KRPreferencesController alloc]
      initWithPreferences:preferences
                    apply:^BOOL(NSDictionary *next) {
                      record->offMain = record->offMain || !NSThread.isMainThread;
                      [record->calls addObject:next];
                      return record->result;
                    }];
  controller.chooseFile = ^NSURL *(NSString *) {
    CHECK(false);
    return nil;
  };
  return controller;
}

// Identity with the boolean singletons: a saved flag must be a property-list boolean, not an
// integer NSNumber
bool isTrue(id value) { return value == (id)kCFBooleanTrue; }
bool isFalse(id value) { return value == (id)kCFBooleanFalse; }

// The panel requests nonactivating key focus and remains available after focus moves away
void testSettingsPanelConfiguration() {
  currentTest = "nonactivating_settings_panel";
  Applied applied;
  KRPreferencesController *controller = makeController(@{}, applied);
  CHECK([controller.window isKindOfClass:NSPanel.class]);
  CHECK((controller.window.styleMask & NSWindowStyleMaskNonactivatingPanel) != 0);
  CHECK(controller.window.canBecomeKeyWindow);
  CHECK(!((NSPanel *)controller.window).hidesOnDeactivate);
  CHECK(!((NSPanel *)controller.window).becomesKeyOnlyIfNeeded);
  CHECK(controller.englishPath.editable && controller.russianPath.editable);
}

// Save writes back the whole snapshot with only the window's keys changed, on main. A key the
// window doesn't own survives unchanged; dropping it would erase settings such as the
// per-application tiers the input menu stores in the same domain.
void testSnapshotPreservation() {
  currentTest = "snapshot_preservation";
  Applied applied;
  NSDictionary *snapshot = @{@"Unrelated" : @[ @1, @"x" ], kRussian : @"/ru.add"};
  KRPreferencesController *controller = makeController(snapshot, applied);
  CHECK(controller.automaticCorrection.state == NSControlStateValueOff);
  CHECK([controller.russianPath.stringValue isEqualToString:@"/ru.add"]);
  CHECK(controller.englishPath.stringValue.length == 0);
  controller.automaticCorrection.state = NSControlStateValueOn;
  [controller.saveButton performClick:nil];
  CHECK(applied.calls.count == 1 && !applied.offMain);
  NSDictionary *next = applied.calls.firstObject;
  CHECK([next[@"Unrelated"] isEqual:snapshot[@"Unrelated"]]);
  CHECK([next[kRussian] isEqualToString:@"/ru.add"]);
  CHECK(isTrue(next[kModel]));
  // Absent options are shown with their defaults and then saved as explicit values
  CHECK(isTrue(next[kCompatibility]) && isFalse(next[kUnlistedModel]));
  CHECK(isTrue(next[kSwitching]));
  // An untouched missing path stays absent, so the runtime keeps its default discovery
  CHECK(!next[kEnglish] && next.count == 6);
  CHECK(controller.errorLabel.stringValue.length == 0);
}

// Only a true NSNumber reads as enabled; saving always writes an explicit NSNumber
void testCheckboxValues() {
  currentTest = "checkbox_values";
  struct Case {
    id stored;
    NSControlStateValue shown;
  };
  const Case cases[] = {{@YES, NSControlStateValueOn},
                        {@NO, NSControlStateValueOff},
                        {@"YES", NSControlStateValueOff},
                        {nil, NSControlStateValueOff}};
  for (const Case &c : cases) {
    Applied applied;
    KRPreferencesController *controller =
        makeController(c.stored ? @{kModel : c.stored} : @{}, applied);
    CHECK(controller.automaticCorrection.state == c.shown);
    [controller.saveButton performClick:nil];
    CHECK(applied.calls.count == 1);
    CHECK(c.shown == NSControlStateValueOn ? isTrue(applied.calls[0][kModel])
                                           : isFalse(applied.calls[0][kModel]));
  }
  Applied applied;
  KRPreferencesController *controller = makeController(@{kModel : @YES}, applied);
  controller.automaticCorrection.state = NSControlStateValueOff;
  [controller.saveButton performClick:nil];
  CHECK(applied.calls.count == 1 && isFalse(applied.calls[0][kModel]));
}

// Refused paths never reach apply; the edit and an inline reason stay in place
void testUnusablePathsRefused() {
  currentTest = "unusable_paths_refused";
  NSString *tooLong = [@"/" stringByPaddingToLength:KR_MAX_PATH_BYTES + 1
                                         withString:@"a"
                                    startingAtIndex:0];
  NSString *longest = [@"/" stringByPaddingToLength:KR_MAX_PATH_BYTES
                                         withString:@"a"
                                    startingAtIndex:0];
  NSString *nul = [NSString stringWithFormat:@"/en.add%Cx", static_cast<unichar>(0)];
  for (NSString *path in @[ @"relative/en.add", @"~/.vim/spell/en.utf-8.add", nul, tooLong ]) {
    Applied applied;
    KRPreferencesController *controller = makeController(@{kEnglish : @"/old.add"}, applied);
    controller.englishPath.stringValue = path;
    [controller.saveButton performClick:nil];
    CHECK(applied.calls.count == 0);
    CHECK([controller.errorLabel.stringValue containsString:@"English"]);
    CHECK([controller.englishPath.stringValue isEqualToString:path]);
  }
  Applied applied;
  KRPreferencesController *controller = makeController(@{}, applied);
  controller.russianPath.stringValue = longest;
  [controller.saveButton performClick:nil];
  CHECK(applied.calls.count == 1 && [applied.calls[0][kRussian] isEqualToString:longest]);
}

// A chosen file only fills the field; nothing checks that it exists. Clearing a field removes the
// key, and an untouched value of the wrong type is preserved rather than reinterpreted.
void testChosenClearedAndUntouchedPaths() {
  currentTest = "chosen_cleared_untouched_paths";
  Applied applied;
  KRPreferencesController *controller = makeController(
      @{kEnglish : @42,
        kRussian : @"/ru.add"},
      applied);
  CHECK(controller.englishPath.stringValue.length == 0);
  NSMutableArray<NSString *> *asked = [NSMutableArray array];
  NSURL *__block answer = [NSURL fileURLWithPath:@"/nonexistent/keyrill/ru.utf-8.add"];
  controller.chooseFile = ^NSURL *(NSString *language) {
    [asked addObject:language];
    return answer;
  };
  [controller.chooseRussian performClick:nil];
  CHECK([controller.russianPath.stringValue isEqualToString:answer.path]);
  answer = nil;
  [controller.chooseRussian performClick:nil];
  CHECK([controller.russianPath.stringValue isEqualToString:@"/nonexistent/keyrill/ru.utf-8.add"]);
  CHECK(([asked isEqualToArray:@[ @"Russian", @"Russian" ]]));
  [controller.saveButton performClick:nil];
  CHECK(applied.calls.count == 1);
  CHECK([applied.calls[0][kEnglish] isEqual:@42]);
  CHECK([applied.calls[0][kRussian] isEqualToString:@"/nonexistent/keyrill/ru.utf-8.add"]);

  Applied cleared;
  controller = makeController(@{kEnglish : @"/en.add", kRussian : @"/ru.add"}, cleared);
  controller.englishPath.stringValue = @"";
  [controller.saveButton performClick:nil];
  CHECK(cleared.calls.count == 1 && !cleared.calls[0][kEnglish]);
  CHECK([cleared.calls[0][kRussian] isEqualToString:@"/ru.add"]);
}

// Cancel after a refused Save applies nothing, restores every control from the snapshot and
// clears the error
void testCancelDiscardsEdits() {
  currentTest = "cancel_discards_edits";
  Applied applied;
  KRPreferencesController *controller = makeController(
      @{kModel : @NO,
        kEnglish : @"/en.add"},
      applied);
  controller.automaticCorrection.state = NSControlStateValueOn;
  controller.englishPath.stringValue = @"/other.add";
  controller.russianPath.stringValue = @"relative";
  [controller.saveButton performClick:nil];
  CHECK(controller.errorLabel.stringValue.length > 0);
  [controller.cancelButton performClick:nil];
  CHECK(applied.calls.count == 0);
  CHECK(controller.automaticCorrection.state == NSControlStateValueOff);
  CHECK([controller.englishPath.stringValue isEqualToString:@"/en.add"]);
  CHECK(controller.russianPath.stringValue.length == 0);
  CHECK(controller.errorLabel.stringValue.length == 0);
  [controller.saveButton performClick:nil];
  CHECK(applied.calls.count == 1 && [applied.calls[0][kEnglish] isEqualToString:@"/en.add"]);
  CHECK(isFalse(applied.calls[0][kModel]));
}

// A refused apply keeps every edit; the next successful Save becomes the new snapshot
void testFailedApplyRetainsEdits() {
  currentTest = "failed_apply_retains_edits";
  Applied applied;
  applied.result = NO;
  KRPreferencesController *controller = makeController(@{@"Unrelated" : @1}, applied);
  controller.automaticCorrection.state = NSControlStateValueOn;
  controller.englishPath.stringValue = @"/en.add";
  [controller.saveButton performClick:nil];
  CHECK(applied.calls.count == 1);
  CHECK(controller.errorLabel.stringValue.length > 0);
  CHECK(controller.automaticCorrection.state == NSControlStateValueOn);
  CHECK([controller.englishPath.stringValue isEqualToString:@"/en.add"]);
  applied.result = YES;
  [controller.saveButton performClick:nil];
  CHECK(applied.calls.count == 2 && [applied.calls[0] isEqual:applied.calls[1]]);
  CHECK(controller.errorLabel.stringValue.length == 0);
  // After the save, Cancel returns to the saved values, not the original snapshot
  controller.englishPath.stringValue = @"/discarded.add";
  [controller.cancelButton performClick:nil];
  CHECK([controller.englishPath.stringValue isEqualToString:@"/en.add"]);
  CHECK(controller.automaticCorrection.state == NSControlStateValueOn);
  // Saving the reloaded state again leaves the stored path as it is
  [controller.saveButton performClick:nil];
  CHECK(applied.calls.count == 3 && [applied.calls[2] isEqual:applied.calls[1]]);
}

// Switching and compatibility are on when absent, automatic correction in unlisted apps is off;
// each follows an NSNumber and is off for any other type, and every checkbox reads and writes only
// its own key
void testDefaultCheckboxes() {
  currentTest = "default_checkboxes";
  struct Case {
    id stored;
    bool on;
  };
  NSArray<NSString *> *keys = @[ kSwitching, kCompatibility, kUnlistedModel ];
  const bool defaults[] = {true, true, false};
  NSArray<NSButton *> * (^buttons)(KRPreferencesController *) = ^(KRPreferencesController *c) {
    return @[ c.inputSourceSwitching, c.compatibilityEnabled, c.unlistedAutomaticCorrection ];
  };
  for (NSUInteger k = 0; k < keys.count; ++k) {
    const Case cases[] = {{nil, defaults[k]}, {@YES, true},    {@1, true},        {@NO, false},
                          {@0, false},        {@"YES", false}, {@[ @YES ], false}};
    for (const Case &c : cases) {
      Applied applied;
      NSMutableDictionary *snapshot = [NSMutableDictionary dictionary];
      for (NSString *other in keys)
        snapshot[other] = @NO;
      if (c.stored)
        snapshot[keys[k]] = c.stored;
      else
        [snapshot removeObjectForKey:keys[k]];
      KRPreferencesController *controller = makeController(snapshot, applied);
      NSArray<NSButton *> *shown = buttons(controller);
      for (NSUInteger i = 0; i < keys.count; ++i)
        CHECK(shown[i].state == (i == k && c.on ? NSControlStateValueOn : NSControlStateValueOff));
      CHECK(controller.automaticCorrection.state == NSControlStateValueOff);
      [controller.saveButton performClick:nil];
      CHECK(applied.calls.count == 1);
      for (NSUInteger i = 0; i < keys.count; ++i)
        CHECK(i == k && c.on ? isTrue(applied.calls[0][keys[i]])
                             : isFalse(applied.calls[0][keys[i]]));
      CHECK(isFalse(applied.calls[0][kModel]));
    }
  }
  Applied applied;
  KRPreferencesController *controller = makeController(@{kModel : @YES}, applied);
  controller.compatibilityEnabled.state = NSControlStateValueOff;
  controller.inputSourceSwitching.state = NSControlStateValueOff;
  controller.unlistedAutomaticCorrection.state = NSControlStateValueOn;
  [controller.saveButton performClick:nil];
  CHECK(applied.calls.count == 1 && isFalse(applied.calls[0][kCompatibility]));
  CHECK(isFalse(applied.calls[0][kSwitching]));
  CHECK(isTrue(applied.calls[0][kUnlistedModel]) && isTrue(applied.calls[0][kModel]));
  // Cancel returns to the snapshot, including the absent-key defaults
  controller = makeController(@{}, applied);
  for (NSButton *button in buttons(controller))
    button.state =
        button.state == NSControlStateValueOn ? NSControlStateValueOff : NSControlStateValueOn;
  [controller.cancelButton performClick:nil];
  NSArray<NSButton *> *restored = buttons(controller);
  for (NSUInteger i = 0; i < keys.count; ++i)
    CHECK(restored[i].state == (defaults[i] ? NSControlStateValueOn : NSControlStateValueOff));
}

// The scope text and the compatibility note are display only. The scope names the composition
// applications, compatibility mode, both shortcuts, the per-app menu and the Secure Input pause;
// Save passes ApplicationTiers through and adds no key for either text. The note mentions
// Backspace, says the result isn't checked and names both permissions, in plain type and color;
// neither text says experimental, unqualified, trial or TextEdit-only.
void testScopeDisclosureDoesNotBecomeAPreference() {
  currentTest = "scope_disclosure";
  Applied applied;
  NSDictionary *tiers = @{@"com.example.editor" : @"off"};
  KRPreferencesController *controller = makeController(@{@"ApplicationTiers" : tiers}, applied);
  NSString *text = controller.scopeLabel.stringValue;
  for (NSString *part in @[
         @"TextEdit, Safari and Firefox", @"compatibility mode", @"Control-Option-K",
         @"Control-Option-Z", @"Correction in This App", @"pauses while macOS Secure Input is on"
       ])
    CHECK([text containsString:part]);
  [controller.saveButton performClick:nil];
  // Per-app tiers belong to the input menu: the window passes them through unchanged
  CHECK(applied.calls.count == 1 && applied.calls[0].count == 5);
  CHECK([applied.calls[0][@"ApplicationTiers"] isEqual:tiers]);
  NSTextField *note = controller.compatibilityNote;
  CHECK([note.stringValue containsString:@"Backspace"] &&
        [note.stringValue containsString:@"does not check the result"]);
  CHECK([note.stringValue containsString:@"Device Control and Data Access (Accessibility)"]);
  CHECK([note.stringValue containsString:@"Input Monitoring"]);
  CHECK(!(note.font.fontDescriptor.symbolicTraits & NSFontDescriptorTraitBold));
  CHECK(![note.textColor isEqual:NSColor.systemOrangeColor]);
  for (NSString *shown in @[ text, note.stringValue ]) {
    for (NSString *wall in @[ @"xperimental", @"nqualified", @"trial", @"TextEdit-only" ])
      CHECK(![shown containsString:wall]);
  }
}

} // namespace

int main() {
  @autoreleasepool {
    [NSApplication sharedApplication];
    if (!NSThread.isMainThread) {
      std::fprintf(stderr, "preferences_tests: wrong thread\n");
      return 2;
    }
    testSettingsPanelConfiguration();
    testSnapshotPreservation();
    testCheckboxValues();
    testUnusablePathsRefused();
    testChosenClearedAndUntouchedPaths();
    testCancelDiscardsEdits();
    testFailedApplyRetainsEdits();
    testDefaultCheckboxes();
    testScopeDisclosureDoesNotBecomeAPreference();
  }
  if (failures) {
    std::fprintf(stderr, "preferences_tests: %d failure(s)\n", failures);
    return 1;
  }
  std::printf("preferences_tests: passed\n");
  return 0;
}
