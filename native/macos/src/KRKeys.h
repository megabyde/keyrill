#import <AppKit/AppKit.h>

#include "keyrill.h"

/// Maps one host key-down to a portable event. Text comes from the host's layout translation;
/// keycodes only identify Backspace, Escape and the reserved chords (Control+Option+K manual,
/// Control+Option+Z reverse). Any other Command/Control/Option key is FINISH_REQUEST.
kr_event KREventForKey(unsigned short keyCode, NSEventModifierFlags flags, NSString *characters,
                       uint32_t mode);
