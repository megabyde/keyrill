#import <AppKit/AppKit.h>

#include "keyrill.h"

/// Maps one host key-down to a portable event. Text comes from the host's layout translation;
/// keycodes only identify Backspace, Escape and the reserved chords (Control+Option+K manual,
/// Control+Option+Z reverse). Any other Command/Control/Option key is FINISH_REQUEST, and so is
/// a key whose characters are empty, longer than KR_EVENT_CAPACITY UTF-16 units, or contain a
/// control or AppKit function-key unit.
kr_event KREventForKey(unsigned short keyCode, NSEventModifierFlags flags, NSString *characters,
                       uint32_t mode);
