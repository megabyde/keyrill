#import <Cocoa/Cocoa.h>
#include <cmath>
#include <cstring>
#include <initializer_list>

// Menu bar and palette icons for the input method (KR) and its two modes (EN, RU). The menu bar
// masters are badges like the system's own sources: a rounded rectangle the full width of the
// icon and about two thirds of its height, with the letters cut out, black for the template and
// white for the selected variant. `--check` confirms that Info.plist names each file and that
// each file has 16 and 32 pixel representations of a 16-point image with transparent top and
// bottom rows and the expected ink: black or white for menu bar masters, gray levels with both
// black and white for the two modes' palette badges.
// `--write` and `--write-palette` regenerate them

static NSString *const names[] = {@"Keyrill", @"KeyrillEnglish", @"KeyrillRussian"};
static NSString *const labels[] = {@"KR", @"EN", @"RU"};

static NSString *filename(NSUInteger index, BOOL selected, BOOL palette = NO) {
  return [NSString stringWithFormat:@"%@%@.tif", names[index],
                                    palette ? @"Palette" : (selected ? @"Selected" : @"Template")];
}

// Regenerate manually: AppKit and font versions affect bytes, so builds only validate masters
static BOOL writeIcons(NSString *directory, BOOL paletteOnly = NO) {
  if (![NSFileManager.defaultManager createDirectoryAtPath:directory
                               withIntermediateDirectories:YES
                                                attributes:nil
                                                     error:nil])
    return NO;
  NSFont *font = [NSFont systemFontOfSize:9 weight:NSFontWeightBold];
  if (!font)
    return NO;
  for (NSUInteger index = 0; index < 3; ++index) {
    for (NSUInteger variant : {0, 1, 2}) {
      BOOL palette = variant == 2;
      BOOL selected = variant == 1;
      if ((palette && !index) || (paletteOnly && !palette))
        continue;
      NSMutableArray<NSBitmapImageRep *> *reps = [NSMutableArray array];
      for (NSInteger scale : {1, 2}) {
        NSBitmapImageRep *rep =
            [[NSBitmapImageRep alloc] initWithBitmapDataPlanes:nullptr
                                                    pixelsWide:16 * scale
                                                    pixelsHigh:16 * scale
                                                 bitsPerSample:8
                                               samplesPerPixel:4
                                                      hasAlpha:YES
                                                      isPlanar:NO
                                                colorSpaceName:NSCalibratedRGBColorSpace
                                                   bytesPerRow:0
                                                  bitsPerPixel:0];
        rep.size = NSMakeSize(16, 16);
        NSGraphicsContext *context = [NSGraphicsContext graphicsContextWithBitmapImageRep:rep];
        [NSGraphicsContext saveGraphicsState];
        NSGraphicsContext.currentContext = context;
        [(palette || selected ? NSColor.whiteColor : NSColor.blackColor) setFill];
        [[NSBezierPath bezierPathWithRoundedRect:NSMakeRect(0, 2.5, 16, 11) xRadius:3
                                         yRadius:3] fill];
        // A palette badge prints black letters on white; a menu bar master cuts them out
        if (!palette)
          context.compositingOperation = NSCompositingOperationDestinationOut;
        NSDictionary *attributes =
            @{NSFontAttributeName : font, NSForegroundColorAttributeName : NSColor.blackColor};
        NSSize size = [labels[index] sizeWithAttributes:attributes];
        [labels[index] drawAtPoint:NSMakePoint((16 - size.width) / 2, (16 - size.height) / 2)
                    withAttributes:attributes];
        [NSGraphicsContext restoreGraphicsState];
        [reps addObject:rep];
      }
      NSData *data = [NSBitmapImageRep TIFFRepresentationOfImageRepsInArray:reps];
      if (![data writeToFile:[directory
                                 stringByAppendingPathComponent:filename(index, selected, palette)]
                  atomically:YES])
        return NO;
    }
  }
  return YES;
}

static BOOL validIcon(NSString *path, BOOL selected, BOOL palette = NO) {
  NSArray<NSImageRep *> *reps = [NSBitmapImageRep imageRepsWithContentsOfFile:path];
  if (reps.count != 2)
    return NO;
  NSUInteger scales = 0;
  for (NSImageRep *image in reps) {
    if (![image isKindOfClass:NSBitmapImageRep.class])
      return NO;
    NSBitmapImageRep *rep = (NSBitmapImageRep *)image;
    if (rep.pixelsWide != rep.pixelsHigh || (rep.pixelsWide != 16 && rep.pixelsWide != 32) ||
        !NSEqualSizes(rep.size, NSMakeSize(16, 16)) || !rep.hasAlpha)
      return NO;
    NSUInteger bit = rep.pixelsWide == 16 ? 1 : 2;
    if (scales & bit)
      return NO;
    scales |= bit;
    BOOL hasInk = NO;
    BOOL hasBlack = NO;
    BOOL hasWhite = NO;
    for (NSInteger y = 0; y < rep.pixelsHigh; ++y) {
      for (NSInteger x = 0; x < rep.pixelsWide; ++x) {
        NSColor *color = [[rep colorAtX:x y:y] colorUsingColorSpace:NSColorSpace.sRGBColorSpace];
        if (!color)
          return NO;
        if (color.alphaComponent == 0)
          continue;
        if (y == 0 || y == rep.pixelsHigh - 1)
          return NO;
        CGFloat expected = selected ? 1 : 0;
        if (palette ? (std::fabs(color.redComponent - color.greenComponent) > 0.01 ||
                       std::fabs(color.greenComponent - color.blueComponent) > 0.01)
                    : (std::fabs(color.redComponent - expected) > 0.01 ||
                       std::fabs(color.greenComponent - expected) > 0.01 ||
                       std::fabs(color.blueComponent - expected) > 0.01))
          return NO;
        hasBlack |= color.alphaComponent > 0.99 && color.redComponent < 0.01;
        hasWhite |= color.alphaComponent > 0.99 && color.redComponent > 0.99;
        hasInk = YES;
      }
    }
    if (!hasInk || (palette && (!hasBlack || !hasWhite)))
      return NO;
  }
  return scales == 3;
}

static BOOL checkIcons(NSString *resources, NSString *plistPath) {
  NSDictionary *plist = [NSDictionary dictionaryWithContentsOfURL:[NSURL fileURLWithPath:plistPath]
                                                            error:nil];
  NSDictionary *modes = plist[@"ComponentInputModeDict"][@"tsInputModeListKey"];
  NSDictionary *english = modes[@KR_BUNDLE_IDENTIFIER ".English"];
  NSDictionary *russian = modes[@KR_BUNDLE_IDENTIFIER ".Russian"];
  if (!plist || !english || !russian)
    return NO;
  NSArray *entries = @[ plist, english, russian ];
  for (NSUInteger index = 0; index < 3; ++index) {
    for (BOOL selected : {NO, YES}) {
      NSString *key =
          index == 0
              ? (selected ? @"tsInputMethodAlternateIconFileKey" : @"tsInputMethodIconFileKey")
              : (selected ? @"tsInputModeAlternateMenuIconFileKey" : @"tsInputModeMenuIconFileKey");
      NSString *relative = filename(index, selected);
      if (![entries[index][key] isEqual:relative] ||
          !validIcon([resources stringByAppendingPathComponent:relative], selected))
        return NO;
    }
    if (index &&
        (![entries[index][@"tsInputModePaletteIconFileKey"] isEqual:filename(index, NO, YES)] ||
         !validIcon([resources stringByAppendingPathComponent:filename(index, NO, YES)], NO, YES)))
      return NO;
  }
  return YES;
}

int main(int argc, const char *argv[]) {
  @autoreleasepool {
    BOOL ok = NO;
    if (argc == 3 && std::strcmp(argv[1], "--write") == 0)
      ok = writeIcons([NSString stringWithUTF8String:argv[2]]);
    else if (argc == 3 && std::strcmp(argv[1], "--write-palette") == 0)
      ok = writeIcons([NSString stringWithUTF8String:argv[2]], YES);
    else if (argc == 4 && std::strcmp(argv[1], "--check") == 0)
      ok = checkIcons([NSString stringWithUTF8String:argv[2]],
                      [NSString stringWithUTF8String:argv[3]]);
    else {
      fprintf(stderr, "usage: input_icons --write DIRECTORY | --write-palette DIRECTORY | --check "
                      "RESOURCES INFO_PLIST\n");
      return 2;
    }
    if (!ok)
      fprintf(stderr, "input_icons: icon generation or validation failed\n");
    return ok ? 0 : 1;
  }
}
