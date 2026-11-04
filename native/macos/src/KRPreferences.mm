#import "KRPreferences.h"

#include "keyrill.h"

namespace {

NSString *const kModelKey = @"AutomaticCorrectionEnabled";
NSString *const kSwitchingKey = @"AutomaticInputSourceSwitchingEnabled";
NSString *const kCompatibilityKey = @"CompatibilityEnabled";
NSString *const kUnlistedModelKey = @"UnlistedAutomaticCorrectionEnabled";
NSString *const kPathKeys[2] = {@"EnglishDictionaryPath", @"RussianDictionaryPath"};
NSString *const kLanguages[2] = {@"English", @"Russian"};
NSString *const kDefaultPaths[2] = {@"~/.vim/spell/en.utf-8.add", @"~/.vim/spell/ru.utf-8.add"};

// The runtime's acceptance rule for an explicit path setting
bool usablePath(NSString *path) {
  char bytes[KR_MAX_PATH_BYTES + 1];
  return [path getFileSystemRepresentation:bytes maxLength:sizeof bytes] && bytes[0] == '/';
}

bool isOn(id value) { return [value isKindOfClass:NSNumber.class] && [value boolValue]; }

// Switching and compatibility default to on; a stored value of the wrong type reads as off
bool isOnByDefault(id value) { return !value || isOn(value); }

NSControlStateValue state(bool on) { return on ? NSControlStateValueOn : NSControlStateValueOff; }

NSTextField *label(NSString *text) {
  NSTextField *field = [NSTextField wrappingLabelWithString:text];
  field.preferredMaxLayoutWidth = 460;
  return field;
}

} // namespace

@implementation KRPreferencesController {
  NSDictionary *_snapshot;
  BOOL (^_apply)(NSDictionary *);
  // Path text shown for the snapshot; a field differing from it was edited
  NSString *_shown[2];
}

- (instancetype)initWithPreferences:(NSDictionary *)preferences
                              apply:(BOOL (^)(NSDictionary *))apply {
  NSWindow *window =
      [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 520, 260)
                                  styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
                                    backing:NSBackingStoreBuffered
                                      defer:YES];
  window.title = @"Keyrill Settings";
  window.releasedWhenClosed = NO;
  if ((self = [super initWithWindow:window])) {
    _snapshot = preferences ? [preferences copy] : @{};
    _apply = [apply copy];
    _chooseFile = ^NSURL *(NSString *language) {
      NSOpenPanel *panel = [NSOpenPanel openPanel];
      panel.message = [NSString stringWithFormat:@"Choose the %@ personal dictionary", language];
      panel.canChooseDirectories = NO;
      panel.allowsMultipleSelection = NO;
      return [panel runModal] == NSModalResponseOK ? panel.URL : nil;
    };
    window.delegate = self;
    [self buildIn:window];
    [self load];
  }
  return self;
}

- (NSButton *)button:(NSString *)title action:(SEL)action {
  return [NSButton buttonWithTitle:title target:self action:action];
}

- (void)buildIn:(NSWindow *)window {
  _automaticCorrection = [NSButton checkboxWithTitle:@"Correct wrong-layout words automatically"
                                              target:nil
                                              action:nil];
  _inputSourceSwitching =
      [NSButton checkboxWithTitle:@"Switch the input source to match corrected text"
                           target:nil
                           action:nil];
  _englishPath = [NSTextField textFieldWithString:@""];
  _russianPath = [NSTextField textFieldWithString:@""];
  _englishPath.placeholderString = kDefaultPaths[0];
  _russianPath.placeholderString = kDefaultPaths[1];
  _chooseEnglish = [self button:@"Choose…" action:@selector(choose:)];
  _chooseRussian = [self button:@"Choose…" action:@selector(choose:)];
  _cancelButton = [self button:@"Cancel" action:@selector(cancel:)];
  _cancelButton.keyEquivalent = @"\x1b";
  _saveButton = [self button:@"Save" action:@selector(save:)];
  _saveButton.keyEquivalent = @"\r";
  _compatibilityEnabled =
      [NSButton checkboxWithTitle:@"Use compatibility mode where direct editing is unavailable"
                           target:nil
                           action:nil];
  _unlistedAutomaticCorrection =
      [NSButton checkboxWithTitle:@"Correct automatically on Space in unlisted apps"
                           target:nil
                           action:nil];
  _compatibilityNote =
      label(@"Compatibility mode corrects a word by pressing Backspace and typing the replacement. "
            @"It cannot read the application's text, so it does not check the result. It needs "
            @"Device Control and Data Access (Accessibility), and Input Monitoring access in "
            @"System Settings.");
  _errorLabel = label(@"");
  _errorLabel.textColor = NSColor.systemRedColor;
  _scopeLabel = label(
      @"TextEdit, Safari and Firefox are edited directly. Terminals, code editors, Office and "
      @"apps Keyrill doesn't list use compatibility mode for Control-Option-K, which converts "
      @"the current word, and Control-Option-Z, which reverses the last conversion; password "
      @"managers, remote desktops and virtual machines are off. Change the current app in the "
      @"input menu under Correction in This App. Correction pauses while macOS Secure Input is "
      @"on.");

  NSGridView *paths = [NSGridView gridViewWithViews:@[
    @[ label(@"English dictionary:"), _englishPath, _chooseEnglish ],
    @[ label(@"Russian dictionary:"), _russianPath, _chooseRussian ]
  ]];
  [paths columnAtIndex:1].width = 300;
  NSStackView *buttons = [NSStackView stackViewWithViews:@[ _cancelButton, _saveButton ]];
  NSStackView *content = [NSStackView stackViewWithViews:@[
    _automaticCorrection, _inputSourceSwitching, paths,
    label(@"An empty field uses the default file shown. Dictionaries are read only from the "
          @"chosen files. After Save, return to your document to apply changes."),
    _scopeLabel, _compatibilityNote, _compatibilityEnabled, _unlistedAutomaticCorrection,
    _errorLabel, buttons
  ]];
  content.orientation = NSUserInterfaceLayoutOrientationVertical;
  content.alignment = NSLayoutAttributeLeading;
  content.spacing = 12;
  content.edgeInsets = NSEdgeInsetsMake(20, 20, 20, 20);
  [content setCustomSpacing:20 afterView:_errorLabel];
  window.contentView = content;
  NSSize size = content.fittingSize;
  size.width += content.edgeInsets.right;
  [window setContentSize:size];
}

- (NSTextField *)pathField:(int)index {
  return index ? _russianPath : _englishPath;
}

// Shows the snapshot and clears any refusal
- (void)load {
  _automaticCorrection.state = state(isOn(_snapshot[kModelKey]));
  _inputSourceSwitching.state = state(isOnByDefault(_snapshot[kSwitchingKey]));
  _compatibilityEnabled.state = state(isOnByDefault(_snapshot[kCompatibilityKey]));
  _unlistedAutomaticCorrection.state = state(isOn(_snapshot[kUnlistedModelKey]));
  for (int i = 0; i < 2; ++i) {
    id path = _snapshot[kPathKeys[i]];
    _shown[i] = [path isKindOfClass:NSString.class] ? [path copy] : @"";
    [self pathField:i].stringValue = _shown[i];
  }
  _errorLabel.stringValue = @"";
}

- (void)showWindow:(id)sender {
  [self.window center];
  [super showWindow:sender];
  // The input method is a UI-element process, so its window does not come forward by itself
  [NSApp activateIgnoringOtherApps:YES];
}

- (void)choose:(NSButton *)sender {
  int index = sender == _chooseRussian;
  NSURL *url = _chooseFile(kLanguages[index]);
  if (url.isFileURL && url.path)
    [self pathField:index].stringValue = url.path;
}

- (void)save:(id)sender {
  (void)sender;
  if (!NSThread.isMainThread)
    return;
  NSMutableDictionary *next = [_snapshot mutableCopy];
  next[kModelKey] = @(_automaticCorrection.state == NSControlStateValueOn);
  next[kSwitchingKey] = @(_inputSourceSwitching.state == NSControlStateValueOn);
  next[kCompatibilityKey] = @(_compatibilityEnabled.state == NSControlStateValueOn);
  next[kUnlistedModelKey] = @(_unlistedAutomaticCorrection.state == NSControlStateValueOn);
  for (int i = 0; i < 2; ++i) {
    NSString *path = [[self pathField:i].stringValue copy];
    if ([path isEqualToString:_shown[i]])
      continue;
    if (!path.length) {
      [next removeObjectForKey:kPathKeys[i]];
    } else if (usablePath(path)) {
      next[kPathKeys[i]] = path;
    } else {
      _errorLabel.stringValue =
          [NSString stringWithFormat:@"The %@ dictionary must be an absolute path of at most "
                                     @"%u bytes.",
                                     kLanguages[i], KR_MAX_PATH_BYTES];
      return;
    }
  }
  NSDictionary *applied = [next copy];
  if (!_apply(applied)) {
    _errorLabel.stringValue = @"Settings were not saved. Your changes are still here.";
    return;
  }
  _snapshot = applied;
  [self close];
  [self load];
}

- (void)cancel:(id)sender {
  (void)sender;
  [self close];
  [self load];
}

- (void)windowWillClose:(NSNotification *)notification {
  (void)notification;
  [self load];
}

@end
