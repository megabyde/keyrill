#import "KRCompatibility.h"

#import <Carbon/Carbon.h>

#include <array>
#include <limits>

NSString *const KRCompatibilityReady = @"Compatibility: ready";

namespace {

// "KEYRILLC" in ASCII, stored in kCGEventSourceUserData of every posted event
constexpr int64_t kSyntheticTag = INT64_C(0x4b455952494c4c43);
// Process-wide guard state. The tap's run-loop source is on the main run loop, so the callback
// and every reader run on main
uint64_t guardGeneration = 1;
uint64_t sourceGeneration = 0;
uint64_t observedKeyDowns = 0;
BOOL guardExhausted = NO;
CFMachPortRef guardTap;
CFRunLoopSourceRef guardSource;

// One posted key as the echo matcher expects to see it again
struct PostedKey {
  CGKeyCode code;
  CGEventFlags flags;
  uint16_t unit;
};

// The ANSI key and Shift state that type `unit` in the US or Russian layout. A posted event
// carries the unit as its Unicode string and this key as its keycode and flags, and the echo
// matcher compares all three. A unit with no key here refuses the whole rewrite
bool keyForUnit(uint16_t unit, PostedKey &key) {
  if (unit == 32) {
    key = {kVK_Space, 0, unit};
    return true;
  }
  NSString *lower = @"qwertyuiop[]asdfghjkl;'zxcvbnm,.";
  NSString *upper = @"QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>";
  NSString *russian = @"йцукенгшщзхъфывапролджэячсмитьбю";
  NSString *russianUpper = @"ЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ";
  constexpr CGKeyCode codes[] = {
      kVK_ANSI_Q, kVK_ANSI_W,         kVK_ANSI_E,           kVK_ANSI_R,
      kVK_ANSI_T, kVK_ANSI_Y,         kVK_ANSI_U,           kVK_ANSI_I,
      kVK_ANSI_O, kVK_ANSI_P,         kVK_ANSI_LeftBracket, kVK_ANSI_RightBracket,
      kVK_ANSI_A, kVK_ANSI_S,         kVK_ANSI_D,           kVK_ANSI_F,
      kVK_ANSI_G, kVK_ANSI_H,         kVK_ANSI_J,           kVK_ANSI_K,
      kVK_ANSI_L, kVK_ANSI_Semicolon, kVK_ANSI_Quote,       kVK_ANSI_Z,
      kVK_ANSI_X, kVK_ANSI_C,         kVK_ANSI_V,           kVK_ANSI_B,
      kVK_ANSI_N, kVK_ANSI_M,         kVK_ANSI_Comma,       kVK_ANSI_Period};
  for (NSUInteger i = 0; i < lower.length; ++i) {
    if (unit == [lower characterAtIndex:i] || unit == [russian characterAtIndex:i]) {
      key = {codes[i], 0, unit};
      return true;
    }
    if (unit == [upper characterAtIndex:i] || unit == [russianUpper characterAtIndex:i]) {
      key = {codes[i], kCGEventFlagMaskShift, unit};
      return true;
    }
  }
  if (unit == u'\\' || unit == u'ё' || unit == u'|' || unit == u'Ё') {
    key = {kVK_ANSI_Backslash, (unit == u'|' || unit == u'Ё') ? kCGEventFlagMaskShift : 0, unit};
    return true;
  }
  return false;
}

void invalidateGuard() {
  if (guardGeneration == std::numeric_limits<uint64_t>::max())
    guardExhausted = YES;
  else
    ++guardGeneration;
}

// Set while a rewrite is being posted; guardReady then skips the permission preflights. Those
// are IPC to the TCC daemon and run once per handled key, not for each posted event: a
// revocation between two posts in one synchronous batch is a race that per-event checks can't
// close either, since each check still precedes its post
bool dispatching = false;

// Listen-only tap callback. It reads keycodes and modifier flags, never characters. Plain and
// shifted typing and the two reserved chords keep tracking; any other key with Command, Control
// or Option, navigation, Forward Delete, Tab, Return, Escape and every mouse or scroll event
// expire it. Untagged key-downs are counted for the automatic-rewrite gate; tagged events are
// ignored entirely
CGEventRef observeEvent(CGEventTapProxy, CGEventType type, CGEventRef event, void *) {
  if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
    // Events were missed while the tap was off, so tracking restarts
    invalidateGuard();
    if (guardTap)
      CGEventTapEnable(guardTap, true);
    return event;
  }
  if (!event) {
    invalidateGuard();
    return event;
  }
  if (CGEventGetIntegerValueField(event, kCGEventSourceUserData) == kSyntheticTag)
    return event;
  if (type == kCGEventKeyDown) {
    ++observedKeyDowns;
    CGEventFlags flags =
        CGEventGetFlags(event) & (kCGEventFlagMaskCommand | kCGEventFlagMaskControl |
                                  kCGEventFlagMaskAlternate | kCGEventFlagMaskShift);
    int64_t code = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
    bool reserved = flags == (kCGEventFlagMaskControl | kCGEventFlagMaskAlternate) &&
                    (code == kVK_ANSI_K || code == kVK_ANSI_Z);
    if (!reserved &&
        ((flags & ~kCGEventFlagMaskShift) || code == kVK_LeftArrow || code == kVK_RightArrow ||
         code == kVK_UpArrow || code == kVK_DownArrow || code == kVK_Home || code == kVK_End ||
         code == kVK_PageUp || code == kVK_PageDown || code == kVK_ForwardDelete ||
         code == kVK_Tab || code == kVK_Return || code == kVK_Escape))
      invalidateGuard();
  } else {
    invalidateGuard();
  }
  return event;
}

// Posting is allowed only while the guard can see what would expire tracking
BOOL guardReady() {
  return guardTap && !guardExhausted && CGEventTapIsEnabled(guardTap) &&
         (dispatching || (CGPreflightListenEventAccess() && CGPreflightPostEventAccess()));
}

KRCompatibilityTarget snapshot(NSString *bundle, id<IMKTextInput> client) {
  KRCompatibilityTarget target = {0, guardGeneration, NO, sourceGeneration};
  if (!guardReady() || IsSecureEventInputEnabled())
    return target;
  @try {
    if (![[client bundleIdentifier] isEqualToString:bundle])
      return target;
    NSRunningApplication *front = NSWorkspace.sharedWorkspace.frontmostApplication;
    if (![front.bundleIdentifier isEqualToString:bundle] || front.processIdentifier <= 0 ||
        front.terminated)
      return target;
    target.pid = front.processIdentifier;
    target.available = YES;
  } @catch (NSException *) {
  }
  return target;
}

// The deletion bound of 65 is the engine's 64-unit word limit plus one Space
KRCompatibilityDispatch dispatch(pid_t pid, const kr_compatibility_plan *plan,
                                 BOOL (^stillCurrent)(void), void (^post)(pid_t, CGEventRef)) {
  if (pid <= 0 || plan->operation != KR_COMPAT_REWRITE || plan->delete_units == 0 ||
      plan->delete_units > 65 || plan->replacement_len == 0 ||
      plan->replacement_len > KR_TEXT_CAPACITY)
    return KRCompatibilityNotStarted;
  for (uint32_t i = 0; i < plan->replacement_len; ++i) {
    PostedKey key;
    if (!keyForUnit(plan->replacement[i], key))
      return KRCompatibilityNotStarted;
  }
  CGEventSourceRef source = CGEventSourceCreate(kCGEventSourceStatePrivate);
  if (!source)
    return KRCompatibilityNotStarted;
  std::array<CGEventRef, 2 * (65 + KR_TEXT_CAPACITY)> events = {};
  size_t count = 0;
  bool complete = true;
  for (uint32_t i = 0; i < plan->delete_units + plan->replacement_len && complete; ++i) {
    bool deletion = i < plan->delete_units;
    PostedKey key = {kVK_Delete, 0, 0};
    if (!deletion)
      keyForUnit(plan->replacement[i - plan->delete_units], key);
    for (bool down : {true, false}) {
      CGEventRef event = CGEventCreateKeyboardEvent(source, key.code, down);
      if (!event) {
        complete = false;
        break;
      }
      CGEventSetFlags(event, key.flags);
      CGEventSetIntegerValueField(event, kCGEventSourceUserData, kSyntheticTag);
      if (!deletion)
        CGEventKeyboardSetUnicodeString(event, 1, &plan->replacement[i - plan->delete_units]);
      events[count++] = event;
    }
  }
  CFRelease(source);
  size_t submitted = 0;
  @try {
    if (complete) {
      for (size_t i = 0; i < count; ++i) {
        if (!stillCurrent())
          break;
        // Counted before the call: a post that throws may still have been delivered
        ++submitted;
        post(pid, events[i]);
      }
    }
  } @catch (NSException *) {
    complete = false;
  }
  for (size_t i = 0; i < count; ++i)
    CFRelease(events[i]);
  if (!submitted)
    return KRCompatibilityNotStarted;
  return complete && submitted == count ? KRCompatibilitySubmitted : KRCompatibilityPartial;
}

} // namespace

@implementation KRCompatibilitySession {
  kr_engine *_engine;
  NSString *_bundle;
  BOOL _automatic;
  BOOL _busy;
  BOOL _poisoned;
  uint64_t _generation;
  // Target of the previous key; another pid or guard generation since then restarts tracking
  KRCompatibilityTarget _previous;
  NSString *_statusTitle;
  // The sequence the last rewrite posted and how much of its echo has arrived
  std::array<PostedKey, 65 + KR_TEXT_CAPACITY> _posted;
  uint32_t _postedCount;
  uint32_t _postedIndex;
  uint64_t _postedGuardGeneration;
  // Untagged key-downs counted at the last rewrite; the next automatic one needs the count to
  // have moved
  BOOL _rewriteDispatched;
  uint64_t _observedAtRewrite;
  // One guard change made only of source changes is expected: the echo of Keyrill's request
  BOOL _acceptAutomaticSourceChange;
  // Mode of a tracked word kept across a requested mode change; zero when none
  uint32_t _heldMode;
}

+ (BOOL)isSyntheticEvent:(CGEventRef)event {
  return event && CGEventGetIntegerValueField(event, kCGEventSourceUserData) == kSyntheticTag;
}

+ (uint64_t)inputContextGeneration {
  return guardGeneration;
}

+ (uint64_t)inputSourceGeneration {
  return sourceGeneration;
}

+ (KRCompatibilityDispatch)dispatchPlan:(const kr_compatibility_plan *)plan
                              toProcess:(pid_t)pid
                           stillCurrent:(BOOL (^)(void))stillCurrent
                              postEvent:(void (^)(pid_t, CGEventRef))post {
  if (!plan)
    return KRCompatibilityNotStarted;
  return dispatch(pid, plan, stillCurrent, post);
}

+ (BOOL)startGuardIfPermitted {
  if (!NSThread.isMainThread || [KRSession inputCallbackActive])
    return NO;
  if (guardTap)
    return guardReady();
  if (!CGPreflightListenEventAccess() || !CGPreflightPostEventAccess())
    return NO;
  CGEventMask mask =
      CGEventMaskBit(kCGEventKeyDown) | CGEventMaskBit(kCGEventLeftMouseDown) |
      CGEventMaskBit(kCGEventRightMouseDown) | CGEventMaskBit(kCGEventOtherMouseDown) |
      CGEventMaskBit(kCGEventLeftMouseDragged) | CGEventMaskBit(kCGEventRightMouseDragged) |
      CGEventMaskBit(kCGEventOtherMouseDragged) | CGEventMaskBit(kCGEventScrollWheel);
  guardTap = CGEventTapCreate(kCGSessionEventTap, kCGHeadInsertEventTap,
                              kCGEventTapOptionListenOnly, mask, observeEvent, nullptr);
  if (!guardTap)
    return NO;
  guardSource = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, guardTap, 0);
  if (!guardSource) {
    CFRelease(guardTap);
    guardTap = nullptr;
    return NO;
  }
  CFRunLoopAddSource(CFRunLoopGetMain(), guardSource, kCFRunLoopCommonModes);
  [NSWorkspace.sharedWorkspace.notificationCenter
      addObserverForName:NSWorkspaceDidActivateApplicationNotification
                  object:nil
                   queue:NSOperationQueue.mainQueue
              usingBlock:^(NSNotification *) {
                invalidateGuard();
              }];
  [NSDistributedNotificationCenter.defaultCenter
      addObserverForName:(__bridge NSString *)kTISNotifySelectedKeyboardInputSourceChanged
                  object:nil
                   queue:NSOperationQueue.mainQueue
              usingBlock:^(NSNotification *) {
                ++sourceGeneration;
                invalidateGuard();
              }];
  return guardReady();
}

- (instancetype)initWithEngine:(kr_engine *)engine
              bundleIdentifier:(NSString *)bundle
                     automatic:(BOOL)automatic {
  if ((self = [super init])) {
    _engine = engine;
    _bundle = [bundle copy];
    _automatic = automatic;
    _mode = KR_EN;
    _secureInputEnabled = ^BOOL {
      return IsSecureEventInputEnabled();
    };
    _targetSnapshot = ^KRCompatibilityTarget(NSString *expected, id<IMKTextInput> client) {
      return snapshot(expected, client);
    };
    _observedKeyCount = ^uint64_t {
      return observedKeyDowns;
    };
    _dispatchRewrite = ^KRCompatibilityDispatch(pid_t pid, const kr_compatibility_plan *plan,
                                                BOOL (^stillCurrent)(void)) {
      return dispatch(pid, plan, stillCurrent, ^(pid_t process, CGEventRef event) {
        CGEventPostToPid(process, event);
      });
    };
    [self activate];
  }
  return self;
}

- (void)dealloc {
  kr_engine_free(_engine);
}

- (NSString *)statusTitle {
  return _statusTitle;
}

- (BOOL)shouldPassPostedEvent:(CGEventRef)event {
  if (!NSThread.isMainThread)
    return NO;
  BOOL tagged = [KRCompatibilitySession isSyntheticEvent:event];
  if (!_postedCount)
    return tagged;
  if (guardGeneration != _postedGuardGeneration &&
      ![self rebaselineAutomaticSourceChange:guardGeneration source:sourceGeneration]) {
    // Posted events may still arrive after focus or source changes; restart from no word
    [self invalidate];
    _statusTitle = @"Compatibility: input context changed; tracking restarted";
    return tagged;
  }
  if (event && CGEventGetType(event) != kCGEventKeyDown)
    return tagged;
  bool matches = false;
  if (event) {
    const PostedKey &expected = _posted[_postedIndex];
    CGEventFlags modifiers =
        CGEventGetFlags(event) & (kCGEventFlagMaskShift | kCGEventFlagMaskControl |
                                  kCGEventFlagMaskAlternate | kCGEventFlagMaskCommand);
    UniCharCount count = 0;
    UniChar unit = 0;
    CGEventKeyboardGetUnicodeString(event, 1, &count, &unit);
    matches = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode) == expected.code &&
              modifiers == expected.flags &&
              (expected.code == kVK_Delete || (count == 1 && unit == expected.unit));
  }
  if (matches) {
    if (++_postedIndex == _postedCount)
      _postedCount = _postedIndex = 0;
    return YES;
  }
  // Hosts need not echo posted keys through the input method, so real input ends the expectation
  if (!tagged)
    _postedCount = _postedIndex = 0;
  return tagged;
}

// Accepts a guard advance once after Keyrill's own mode request, when every guard step since
// the baseline was a source change, and moves the baseline to it. Any other guard event in
// between makes the two advances differ
- (BOOL)rebaselineAutomaticSourceChange:(uint64_t)generation source:(uint64_t)source {
  if (!_acceptAutomaticSourceChange || generation <= _previous.generation ||
      source <= _previous.sourceGeneration ||
      generation - _previous.generation != source - _previous.sourceGeneration)
    return NO;
  _previous.generation = generation;
  _previous.sourceGeneration = source;
  _postedGuardGeneration = generation;
  _acceptAutomaticSourceChange = NO;
  return YES;
}

- (void)invalidate {
  if (!NSThread.isMainThread)
    return;
  if (_generation == std::numeric_limits<uint64_t>::max())
    _poisoned = YES;
  else
    ++_generation;
  _previous = {};
  _postedCount = _postedIndex = 0;
  _automaticSwitchMode = 0;
  _acceptAutomaticSourceChange = NO;
  _heldMode = 0;
  if (_engine && kr_reset(_engine) == KR_POISONED)
    _poisoned = YES;
}

- (void)activate {
  if (!NSThread.isMainThread)
    return;
  [self invalidate];
  _stopped = _poisoned || !_engine;
  _statusTitle = _stopped ? @"Compatibility: engine unavailable" : KRCompatibilityReady;
}

- (void)keepAcrossAutomaticSwitchToMode:(uint32_t)mode {
  if (!NSThread.isMainThread || (mode != KR_EN && mode != KR_RU) || _stopped || _poisoned ||
      !_engine)
    return;
  if (!_heldMode)
    _heldMode = _mode;
  _mode = mode;
  if (_heldMode == mode)
    _heldMode = 0;
  _automaticSwitchMode = 0;
  // The source-change notification that follows this echo must not restart tracking, but any
  // other guard event from here on still does
  _postedGuardGeneration = guardGeneration;
  _previous.generation = guardGeneration;
  _previous.sourceGeneration = sourceGeneration;
  _acceptAutomaticSourceChange = YES;
}

// Restarts the engine in `mode` with the trailing Space of the kept word already observed, so
// the next word still has its automatic boundary
- (BOOL)restartTrackingInMode:(uint32_t)mode {
  _heldMode = 0;
  uint32_t status = kr_reset(_engine);
  kr_event separator = {KR_TEXT, mode, KR_CTX_INELIGIBLE, 1, {32}};
  kr_compatibility_plan plan;
  if (status == KR_OK)
    status = kr_plan_compatibility_event(_engine, &separator, 0, &plan);
  if (status == KR_OK && plan.operation == KR_COMPAT_OBSERVE && plan.disposition == KR_KEY_PASS)
    status = kr_commit_compatibility(_engine, plan.plan_id, KR_COMPAT_OBSERVED);
  else if (status == KR_OK)
    status = KR_STATE;
  if (status == KR_OK)
    return YES;
  [self invalidate];
  _poisoned = _poisoned || status == KR_POISONED;
  _stopped = YES;
  _statusTitle = @"Compatibility: engine stopped";
  return NO;
}

- (uint32_t)adoptVocabulary:(kr_vocabulary *)vocabulary {
  if (!NSThread.isMainThread || [KRSession inputCallbackActive] || _busy || !_engine)
    return KR_STATE;
  return kr_adopt_vocabulary(_engine, vocabulary);
}

- (BOOL)handleEvent:(const kr_event *)input client:(id<IMKTextInput>)client {
  __attribute__((objc_precise_lifetime)) KRCompatibilitySession *lifetime = self;
  (void)lifetime;
  if (!NSThread.isMainThread || !input)
    return NO;
  _automaticSwitchMode = 0;
  BOOL reserved = input->role == KR_MANUAL || input->role == KR_REVERSE;
  if (self.secureInputEnabled()) {
    [self invalidate];
    _statusTitle = @"Compatibility: macOS Secure Input is on";
    return NO;
  }
  if (_stopped || !_engine)
    return reserved;
  if (_busy) {
    [self invalidate];
    _statusTitle = @"Compatibility: input interrupted";
    return reserved;
  }
  _busy = YES;
  uint64_t generation = _generation;
  BOOL dispatchEntered = NO;
  kr_compatibility_plan plan = {};
  @try {
    KRCompatibilityTarget target = _targetSnapshot(_bundle, client);
    if (!target.available || generation != _generation) {
      [self invalidate];
      _statusTitle = @"Compatibility: permissions, guard or foreground target unavailable";
      return reserved;
    }
    // Tracking describes input to one process within one guard generation
    BOOL differentProcess = _previous.available && _previous.pid != target.pid;
    BOOL differentGuard = (_previous.available || _acceptAutomaticSourceChange) &&
                          _previous.generation != target.generation;
    if (differentProcess ||
        (differentGuard && ![self rebaselineAutomaticSourceChange:target.generation
                                                           source:target.sourceGeneration])) {
      [self invalidate];
      generation = _generation;
    }
    _previous = target;
    kr_event event = *input;
    BOOL kept = NO;
    if (_heldMode && event.mode != _heldMode) {
      if (event.role == KR_REVERSE) {
        event.mode = _heldMode;
        kept = YES;
      } else if (![self restartTrackingInMode:event.mode]) {
        return reserved;
      }
    }
    uint32_t status = kr_plan_compatibility_event(_engine, &event, _automatic ? 1 : 0, &plan);
    if (status != KR_OK) {
      [self invalidate];
      _poisoned = _poisoned || status == KR_POISONED;
      _stopped = YES;
      _statusTitle = @"Compatibility: engine stopped";
      return reserved;
    }
    if (plan.operation == KR_COMPAT_OBSERVE) {
      status = kr_commit_compatibility(_engine, plan.plan_id, KR_COMPAT_OBSERVED);
      if (status != KR_OK) {
        [self invalidate];
        _stopped = YES;
        _statusTitle = @"Compatibility: observation refused";
        return reserved;
      }
      _statusTitle = KRCompatibilityReady;
      return plan.disposition == KR_KEY_CONSUMED;
    }
    // A stripped echo that reaches tracking must not start another rewrite by itself
    if (!reserved && _rewriteDispatched && self.observedKeyCount() == _observedAtRewrite) {
      status = kr_commit_compatibility(_engine, plan.plan_id, KR_COMPAT_REFUSED);
      [self invalidate];
      _poisoned = _poisoned || status == KR_POISONED;
      _stopped = _poisoned;
      _statusTitle = @"Compatibility: automatic rewrite needs new observed typing";
      return plan.disposition == KR_KEY_CONSUMED;
    }
    _postedCount = plan.delete_units + plan.replacement_len;
    if (plan.delete_units > 65 || plan.replacement_len > KR_TEXT_CAPACITY ||
        _postedCount > _posted.size()) {
      [self invalidate];
      _stopped = YES;
      _statusTitle = @"Compatibility: invalid rewrite plan";
      return reserved;
    }
    for (uint32_t i = 0; i < plan.delete_units; ++i)
      _posted[i] = {kVK_Delete, 0, 0};
    for (uint32_t i = 0; i < plan.replacement_len; ++i) {
      if (!keyForUnit(plan.replacement[i], _posted[plan.delete_units + i])) {
        [self invalidate];
        _statusTitle = @"Compatibility: unsupported replacement key";
        return reserved;
      }
    }
    _postedIndex = 0;
    _postedGuardGeneration = guardGeneration;
    _rewriteDispatched = YES;
    _observedAtRewrite = self.observedKeyCount();
    dispatchEntered = YES;
    dispatching = true;
    KRCompatibilityDispatch result = KRCompatibilityNotStarted;
    @try {
      result = _dispatchRewrite(target.pid, &plan, ^BOOL {
        if (generation != _generation || self.secureInputEnabled())
          return NO;
        KRCompatibilityTarget now = _targetSnapshot(_bundle, client);
        // The snapshot queried the client, and Secure Input can start during any call
        return generation == _generation && now.available && now.pid == target.pid &&
               now.generation == target.generation && !self.secureInputEnabled();
      });
    } @finally {
      dispatching = false;
    }
    // Each outcome names what was submitted; none says what the client did with it
    uint32_t outcome = result == KRCompatibilitySubmitted ? KR_COMPAT_DISPATCHED
                       : result == KRCompatibilityPartial ? KR_COMPAT_PARTIAL
                                                          : KR_COMPAT_REFUSED;
    status = kr_commit_compatibility(_engine, plan.plan_id, outcome);
    if (result != KRCompatibilitySubmitted || status != KR_OK) {
      [self invalidate];
      _poisoned = _poisoned || status == KR_POISONED;
      // Nothing posted drops only the word. Once anything was posted and not acknowledged as a
      // full submission, the text is unknown: stop until activation, with no replay
      _stopped = result != KRCompatibilityNotStarted || _poisoned;
      _statusTitle = _stopped ? @"Compatibility paused until reactivation; check your recent text"
                              : @"Compatibility: rewrite refused before dispatch";
    } else {
      _statusTitle = KRCompatibilityReady;
      if (!reserved && input->role == KR_TEXT && input->len == 1 && input->units[0] == 32)
        _automaticSwitchMode = _mode == KR_EN ? KR_RU : KR_EN;
      // Undoing a correction in the mode Keyrill switched to also undoes the switch
      if (kept)
        _automaticSwitchMode = event.mode;
    }
    return plan.disposition == KR_KEY_CONSUMED;
  } @catch (NSException *) {
    [self invalidate];
    _stopped = YES;
    _statusTitle = @"Compatibility paused until reactivation; check your recent text";
    // Once dispatch began the plan's disposition stands: an automatic rewrite counted the
    // typed Space among its deletions, so that Space must still pass
    return dispatchEntered ? plan.disposition == KR_KEY_CONSUMED : reserved;
  } @finally {
    _busy = NO;
  }
}

@end
