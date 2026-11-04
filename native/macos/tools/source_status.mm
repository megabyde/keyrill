#import <Carbon/Carbon.h>
#import <Foundation/Foundation.h>

static id property(TISInputSourceRef source, CFStringRef key) {
  id value = (__bridge id)TISGetInputSourceProperty(source, key);
  return value ? value : NSNull.null;
}

static NSDictionary *facts(TISInputSourceRef source) {
  id icon = property(source, kTISPropertyIconImageURL);
  return @{
    @"id" : property(source, kTISPropertyInputSourceID),
    @"name" : property(source, kTISPropertyLocalizedName),
    @"languages" : property(source, kTISPropertyInputSourceLanguages),
    @"icon_url" : [icon isKindOfClass:NSURL.class] ? [icon absoluteString] : NSNull.null,
    @"bundle_id" : property(source, kTISPropertyBundleID),
    @"mode_id" : property(source, kTISPropertyInputModeID),
    @"type" : property(source, kTISPropertyInputSourceType),
    @"enabled" : property(source, kTISPropertyInputSourceIsEnabled),
    @"select_capable" : property(source, kTISPropertyInputSourceIsSelectCapable),
    @"selected" : property(source, kTISPropertyInputSourceIsSelected)
  };
}

int main() {
  @autoreleasepool {
    TISInputSourceRef current = TISCopyCurrentKeyboardInputSource();
    CFArrayRef sources = TISCreateInputSourceList(nullptr, true);
    if (!current || !sources) {
      if (current)
        CFRelease(current);
      if (sources)
        CFRelease(sources);
      return 2;
    }
    NSMutableArray *keyrill = [NSMutableArray array];
    for (CFIndex i = 0; i < CFArrayGetCount(sources); ++i) {
      TISInputSourceRef source = (TISInputSourceRef)CFArrayGetValueAtIndex(sources, i);
      id identifier = property(source, kTISPropertyInputSourceID);
      if ([identifier isKindOfClass:NSString.class] && [identifier hasPrefix:@KR_BUNDLE_IDENTIFIER])
        [keyrill addObject:facts(source)];
    }
    NSDictionary *report = @{@"current" : facts(current), @"keyrill" : keyrill};
    NSData *json = [NSJSONSerialization dataWithJSONObject:report
                                                   options:NSJSONWritingPrettyPrinted
                                                     error:nil];
    CFRelease(sources);
    CFRelease(current);
    if (!json)
      return 2;
    [[NSFileHandle fileHandleWithStandardOutput] writeData:json];
    [[NSFileHandle fileHandleWithStandardOutput]
        writeData:[@"\n" dataUsingEncoding:NSUTF8StringEncoding]];
  }
  return 0;
}
