#import "KRKeys.h"

#import <Carbon/Carbon.h>

#include <algorithm>

kr_event KREventForKey(unsigned short keyCode, NSEventModifierFlags flags, NSString *characters,
                       uint32_t mode) {
  kr_event event = {KR_FINISH_REQUEST, mode, KR_CTX_INELIGIBLE, 0, {}};
  NSEventModifierFlags modifiers =
      flags & (NSEventModifierFlagCommand | NSEventModifierFlagControl | NSEventModifierFlagOption |
               NSEventModifierFlagShift);
  constexpr NSEventModifierFlags chord = NSEventModifierFlagControl | NSEventModifierFlagOption;
  if (modifiers == chord && keyCode == kVK_ANSI_K) {
    event.role = KR_MANUAL;
    return event;
  }
  if (modifiers == chord && keyCode == kVK_ANSI_Z) {
    event.role = KR_REVERSE;
    return event;
  }
  if (modifiers & ~NSEventModifierFlagShift)
    return event;
  if (modifiers == 0 && keyCode == kVK_Delete) {
    event.role = KR_BACKSPACE;
    return event;
  }
  if (modifiers == 0 && keyCode == kVK_Escape) {
    event.role = KR_ESCAPE;
    return event;
  }
  NSUInteger length = characters.length;
  if (length == 0 || length > KR_EVENT_CAPACITY)
    return event;
  uint16_t units[KR_EVENT_CAPACITY];
  [characters getCharacters:units range:NSMakeRange(0, length)];
  for (NSUInteger i = 0; i < length; ++i) {
    uint16_t unit = units[i];
    // Control characters (Return, Tab, Backspace with Shift) and AppKit function-key units
    if (unit < 0x20 || unit == 0x7F || (unit >= 0xF700 && unit <= 0xF8FF))
      return event;
  }
  event.role = KR_TEXT;
  event.len = static_cast<uint32_t>(length);
  std::copy(units, units + length, event.units);
  return event;
}
