#import <Carbon/Carbon.h>
#import <Foundation/Foundation.h>

#include <cstring>

// Read-only plan for enabling Keyrill's input source; it changes no setting, with or without
// `--plan`. It requires a live Aqua session, finds exactly one parent source with Keyrill's
// identifier, checks the parent's type and its two modes, and reports enable capability and
// the enabled and selected states before and after the run as JSON.
//
// Exit status:
//   0  identity matches, the parent is enable-capable, and the selection didn't change
//   2  unknown arguments
//   3  TIS query failed, identity or capability not confirmed, or JSON encoding failed
//   6  the selected input source changed during the run or couldn't be read afterwards
//   9  not a live Aqua session

static id property(TISInputSourceRef source, CFStringRef key) {
  id value = (__bridge id)TISGetInputSourceProperty(source, key);
  return value ? value : NSNull.null;
}

static NSDictionary *states(NSString *identifier) {
  NSDictionary *filter = @{(__bridge NSString *)kTISPropertyBundleID : identifier};
  CFArrayRef sources = TISCreateInputSourceList((__bridge CFDictionaryRef)filter, true);
  if (!sources)
    return nil;
  NSMutableDictionary *result = [NSMutableDictionary dictionary];
  for (CFIndex i = 0; i < CFArrayGetCount(sources); ++i) {
    TISInputSourceRef source = (TISInputSourceRef)CFArrayGetValueAtIndex(sources, i);
    id name = property(source, kTISPropertyInputSourceID);
    if (![name isKindOfClass:NSString.class] || result[name]) {
      CFRelease(sources);
      return nil;
    }
    result[name] = @{
      @"bundle_id" : property(source, kTISPropertyBundleID),
      @"type" : property(source, kTISPropertyInputSourceType),
      @"enabled" : property(source, kTISPropertyInputSourceIsEnabled),
      @"enable_capable" : property(source, kTISPropertyInputSourceIsEnableCapable),
      @"select_capable" : property(source, kTISPropertyInputSourceIsSelectCapable),
      @"selected" : property(source, kTISPropertyInputSourceIsSelected)
    };
  }
  CFRelease(sources);
  return result;
}

int main(int argc, char **argv) {
  @autoreleasepool {
    if (argc > 2 || (argc == 2 && std::strcmp(argv[1], "--plan") != 0))
      return 2;

    NSString *manager = nil;
    @try {
      NSTask *task = [NSTask new];
      task.executableURL = [NSURL fileURLWithPath:@"/bin/launchctl"];
      task.arguments = @[ @"managername" ];
      NSPipe *output = [NSPipe pipe];
      task.standardOutput = output;
      task.standardError = NSFileHandle.fileHandleWithNullDevice;
      if ([task launchAndReturnError:nil]) {
        NSData *data = [output.fileHandleForReading readDataToEndOfFile];
        [task waitUntilExit];
        if (task.terminationStatus == 0)
          manager = [[[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding]
              stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
      }
    } @catch (NSException *) {
      manager = nil;
    }
    if (![manager isEqualToString:@"Aqua"]) {
      NSDictionary *refusal = @{
        @"action" : @"plan",
        @"refused" : @"live-aqua-session-required",
        @"session_manager" : manager ? (id)manager : NSNull.null
      };
      NSData *json = [NSJSONSerialization dataWithJSONObject:refusal options:0 error:nil];
      if (json)
        [[NSFileHandle fileHandleWithStandardOutput] writeData:json];
      [[NSFileHandle fileHandleWithStandardOutput]
          writeData:[@"\n" dataUsingEncoding:NSUTF8StringEncoding]];
      return 9;
    }

    NSString *identifier = @KR_BUNDLE_IDENTIFIER;
    NSArray<NSString *> *modes =
        @[ @KR_BUNDLE_IDENTIFIER ".English", @KR_BUNDLE_IDENTIFIER ".Russian" ];
    NSDictionary *beforeStates = states(identifier);
    NSDictionary *filter = @{(__bridge NSString *)kTISPropertyInputSourceID : identifier};
    CFArrayRef sources = TISCreateInputSourceList((__bridge CFDictionaryRef)filter, true);
    TISInputSourceRef current = TISCopyCurrentKeyboardInputSource();
    if (!sources || !current) {
      if (sources)
        CFRelease(sources);
      if (current)
        CFRelease(current);
      return 3;
    }

    NSMutableDictionary *report = [NSMutableDictionary dictionary];
    report[@"action"] = @"plan";
    report[@"target"] = identifier;
    report[@"session_manager"] = manager;
    report[@"states_before"] = beforeStates ? beforeStates : (id)NSNull.null;
    report[@"matching_sources"] = @(CFArrayGetCount(sources));
    int result = 3;
    if (CFArrayGetCount(sources) == 1) {
      TISInputSourceRef parent = (TISInputSourceRef)CFArrayGetValueAtIndex(sources, 0);
      CFStringRef bundle = (CFStringRef)TISGetInputSourceProperty(parent, kTISPropertyBundleID);
      CFStringRef type =
          (CFStringRef)TISGetInputSourceProperty(parent, kTISPropertyInputSourceType);
      CFBooleanRef capable =
          (CFBooleanRef)TISGetInputSourceProperty(parent, kTISPropertyInputSourceIsEnableCapable);
      CFBooleanRef enabled =
          (CFBooleanRef)TISGetInputSourceProperty(parent, kTISPropertyInputSourceIsEnabled);
      CFStringRef selected =
          (CFStringRef)TISGetInputSourceProperty(current, kTISPropertyInputSourceID);
      bool identity = bundle && CFEqual(bundle, (__bridge CFStringRef)identifier) && type &&
                      CFEqual(type, kTISTypeKeyboardInputMethodModeEnabled);
      identity = identity && beforeStates.count == 3;
      identity = identity && enabled &&
                 [beforeStates[identifier][@"enabled"] isEqual:(__bridge NSNumber *)enabled];
      for (NSString *mode in modes) {
        NSDictionary *state = beforeStates[mode];
        identity = identity && [state[@"bundle_id"] isEqual:identifier] &&
                   [state[@"type"] isEqual:(__bridge NSString *)kTISTypeKeyboardInputMode] &&
                   [state[@"enabled"] isKindOfClass:NSNumber.class];
      }
      bool stockUS = selected && CFEqual(selected, CFSTR("com.apple.keylayout.US"));
      report[@"identity_matches"] = @(identity);
      report[@"enable_capable"] = @(capable && CFBooleanGetValue(capable));
      report[@"enabled_before"] = @(enabled && CFBooleanGetValue(enabled));
      report[@"selected_before"] = selected ? (__bridge NSString *)selected : (id)NSNull.null;
      report[@"stock_us_before"] = @(stockUS);
      if (identity && capable && CFBooleanGetValue(capable) && enabled)
        result = 0;
    }
    TISInputSourceRef after = TISCopyCurrentKeyboardInputSource();
    if (after) {
      CFStringRef selected =
          (CFStringRef)TISGetInputSourceProperty(after, kTISPropertyInputSourceID);
      CFStringRef before =
          (CFStringRef)TISGetInputSourceProperty(current, kTISPropertyInputSourceID);
      bool unchanged = selected && before && CFEqual(selected, before);
      report[@"selected_after"] = selected ? (__bridge NSString *)selected : (id)NSNull.null;
      report[@"selection_unchanged"] = @(unchanged);
      if (!unchanged)
        result = 6;
      CFRelease(after);
    } else {
      result = 6;
    }
    NSDictionary *afterStates = states(identifier);
    report[@"states_after"] = afterStates ? afterStates : (id)NSNull.null;
    id afterEnabled = afterStates[identifier][@"enabled"];
    report[@"enabled_after"] = afterEnabled ? afterEnabled : (id)NSNull.null;
    bool modesUnchanged = afterStates.count == 3;
    for (NSString *mode in modes)
      modesUnchanged =
          modesUnchanged && [afterStates[mode][@"enabled"] isEqual:beforeStates[mode][@"enabled"]];
    report[@"mode_enabled_states_unchanged"] = @(modesUnchanged);
    NSData *json = [NSJSONSerialization dataWithJSONObject:report
                                                   options:NSJSONWritingPrettyPrinted
                                                     error:nil];
    CFRelease(current);
    CFRelease(sources);
    if (!json)
      return 3;
    [[NSFileHandle fileHandleWithStandardOutput] writeData:json];
    [[NSFileHandle fileHandleWithStandardOutput]
        writeData:[@"\n" dataUsingEncoding:NSUTF8StringEncoding]];
    return result;
  }
}
