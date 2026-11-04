#import "KRSession.h"

#import <Carbon/Carbon.h>
#include <cstring>

namespace {

constexpr NSRange kCurrentSelection = {NSNotFound, 0};
// Keeps start + KR_TEXT_CAPACITY far from overflow without a check at every addition
constexpr NSUInteger kMaxStart = NSUIntegerMax - 4 * KR_TEXT_CAPACITY;
NSUInteger callbackDepth = 0;
bool callbackDepthSaturated = false;

bool hasMark(NSRange range) { return range.location != NSNotFound && range.length != 0; }

bool isWhitespace(unichar unit) {
  return unit == u' ' || unit == u'\t' || unit == u'\n' || unit == u'\r' || unit == 0x00A0;
}

NSString *string(const uint16_t *units, uint32_t length) {
  return [[NSString alloc] initWithCharacters:units length:length];
}

} // namespace

@implementation KRSession {
  kr_engine *_engine;
  NSSet<NSString *> *_allowedClients;
  uint64_t _generation;
  BOOL _busy;
  BOOL _poisoned;
  BOOL _finishVerified;
  // Local authority: the client, document start and exact text of the verified mark; the caret
  // is at its end
  id<IMKTextInput> _owner;
  NSUInteger _start;
  NSString *_text;
  // Mode the verified mark was planned in; it differs from _mode only for a mark kept across a
  // mode change Keyrill requested
  uint32_t _ownerMode;
  // The last write's own readback check failed, with no reentrancy, exception or Secure Input
  BOOL _checkFailed;
  // The current finish was requested by IMK or the controller, not by a typed key
  BOOL _lifecycleFinish;
  // An unknown outcome stops the word; a second one in the same activation, or one in a client
  // that hasn't verified a write since activation, stops the session
  NSUInteger _unknownOutcomes;
  BOOL _verifiedWrite;
  BOOL _rearmable;
  // Collapsed caret observed when the context was read for a composition start
  NSUInteger _contextLocation;
}

+ (void)beginInputCallback {
  if (!NSThread.isMainThread)
    return;
  if (callbackDepth == NSUIntegerMax)
    callbackDepthSaturated = true;
  else
    ++callbackDepth;
}

+ (void)endInputCallback {
  if (NSThread.isMainThread && !callbackDepthSaturated && callbackDepth > 0)
    --callbackDepth;
}

+ (BOOL)inputCallbackActive {
  return !NSThread.isMainThread || callbackDepthSaturated || callbackDepth != 0;
}

- (instancetype)initWithEngine:(kr_engine *)engine
                allowedClients:(NSSet<NSString *> *)allowedClients {
  if ((self = [super init])) {
    _engine = engine;
    _allowedClients = [allowedClients copy];
    _mode = KR_EN;
    _contextLocation = NSNotFound;
    _secureInputEnabled = ^BOOL {
      return IsSecureEventInputEnabled();
    };
  }
  return self;
}

- (void)dealloc {
  kr_engine_free(_engine);
}

- (BOOL)owned {
  return _owner != nil;
}

- (BOOL)boundaryRetained {
  return _owner && [_text hasSuffix:@" "];
}

// Secure Input can start during any client call; recheck before reading text or writing
- (BOOL)secureAfterQuery {
  if (!self.secureInputEnabled())
    return NO;
  _inputStatus = KRInputSecure;
  return YES;
}

- (void)noteStatus:(uint32_t)status {
  if (status == KR_POISONED) {
    _poisoned = YES;
    _inputStatus = KRInputEngineUnavailable;
  }
  if (_poisoned) {
    _stopped = YES;
    _rearmable = NO;
  }
}

- (void)invalidate {
  if (!NSThread.isMainThread)
    return;
  ++_generation;
  _owner = nil;
  _text = nil;
  _ownerMode = 0;
  _contextLocation = NSNotFound;
  _automaticSwitchMode = 0;
  _finishVerified = NO;
  if (_engine)
    [self noteStatus:kr_reset(_engine)];
}

- (void)activate {
  if (!NSThread.isMainThread)
    return;
  [self invalidate];
  _stopped = _poisoned || !_engine;
  _unknownOutcomes = 0;
  _verifiedWrite = NO;
  _rearmable = NO;
  _inputStatus = _stopped ? KRInputEngineUnavailable : KRInputNotObserved;
}

- (uint32_t)adoptVocabulary:(kr_vocabulary *)vocabulary {
  if (!NSThread.isMainThread || [KRSession inputCallbackActive] || _busy || !_engine)
    return KR_STATE;
  // POISONED can describe the source handle; adoption must not poison the native session
  return kr_adopt_vocabulary(_engine, vocabulary);
}

- (BOOL)finishInClient:(id<IMKTextInput>)client {
  // Host calls may release the controller's last owning reference during dispatch
  __attribute__((objc_precise_lifetime)) KRSession *lifetime = self;
  (void)lifetime;
  if (!NSThread.isMainThread)
    return NO;
  if (_busy || !_owner || client != _owner) {
    if (_busy)
      _inputStatus = KRInputInterrupted;
    [self invalidate];
    return NO;
  }
  kr_event event = {KR_FINISH_REQUEST, _mode, KR_CTX_INELIGIBLE, 0, {}};
  _lifecycleFinish = YES;
  @try {
    [self handleEvent:&event client:client];
  } @finally {
    _lifecycleFinish = NO;
  }
  if (_owner)
    [self invalidate];
  return _finishVerified && !_stopped;
}

- (BOOL)finishForReconfigurationInClient:(id<IMKTextInput>)client {
  __attribute__((objc_precise_lifetime)) KRSession *lifetime = self;
  (void)lifetime;
  if (!NSThread.isMainThread || _poisoned)
    return NO;
  if (_busy) {
    [self invalidate];
    return NO;
  }
  BOOL wasStopped = _stopped;
  [self finishInClient:client];
  return !_poisoned && (wasStopped || !_stopped);
}

- (BOOL)handleEvent:(const kr_event *)input client:(id<IMKTextInput>)client {
  __attribute__((objc_precise_lifetime)) KRSession *lifetime = self;
  (void)lifetime;
  if (!NSThread.isMainThread || !input)
    return NO;
  _automaticSwitchMode = 0;
  _finishVerified = NO;
  BOOL reserved = input->role == KR_MANUAL || input->role == KR_REVERSE;
  if (_busy) {
    _inputStatus = KRInputInterrupted;
    [self invalidate];
    return reserved;
  }
  // Nothing from the failed word carries over: the new word gets the same checks as a first one
  if (_stopped && _rearmable && !_poisoned && input->role == KR_TEXT) {
    _stopped = NO;
    _rearmable = NO;
  }
  if (_stopped || !_engine)
    return reserved;
  if (self.secureInputEnabled()) {
    _inputStatus = KRInputSecure;
    [self invalidate];
    return NO;
  }
  if (_owner && client != _owner)
    [self invalidate];
  _busy = YES;
  @try {
    return [self runEvent:*input client:client generation:_generation];
  } @finally {
    _busy = NO;
  }
}

- (BOOL)runEvent:(kr_event)input client:(id<IMKTextInput>)client generation:(uint64_t)generation {
  for (int pass = 0; pass < 2; ++pass) {
    // A boundary mark kept across a requested mode change is planned in its own mode, where the
    // engine reverses or restores it. Text typed in the new mode can't be classified in the
    // mark's mode, so the mark is finished there first and the key is then planned in its own
    // mode from the finished boundary. Any other mode mismatch reaches the engine, which refuses it
    kr_event event = input;
    bool kept = self.boundaryRetained && _ownerMode != input.mode;
    bool finishFirst = kept && input.role == KR_TEXT;
    if (kept)
      event.mode = _ownerMode;
    if (finishFirst)
      event = {KR_FINISH_REQUEST, _ownerMode, KR_CTX_INELIGIBLE, 0, {}};
    if (!_owner) {
      if (![self clientAllowed:client generation:generation])
        return NO;
      event.context =
          event.role == KR_TEXT ? [self contextIn:client generation:generation] : KR_CTX_INELIGIBLE;
    } else {
      event.context = KR_CTX_INELIGIBLE;
    }
    if (generation != _generation)
      return NO;

    kr_plan plan;
    uint32_t status = kr_plan_event(_engine, &event, &plan);
    if (status != KR_OK) {
      _inputStatus = KRInputEngineUnavailable;
      [self invalidate];
      [self noteStatus:status];
      return NO;
    }

    uint32_t outcome = KR_APPLIED_VERIFIED;
    if (plan.action == KR_MARK || plan.action == KR_FINISH)
      outcome = [self apply:plan client:client generation:generation];
    else if (plan.action != KR_PASS && plan.action != KR_UNAVAILABLE)
      outcome = KR_REFUSED_BEFORE_WRITE;

    status = kr_commit(_engine, plan.plan_id, outcome);
    if (outcome == KR_ATTEMPTED_UNKNOWN) {
      BOOL commit = _checkFailed && generation == _generation;
      [self invalidate];
      _stopped = YES;
      ++_unknownOutcomes;
      _rearmable = _verifiedWrite && _unknownOutcomes < 2;
      [self noteStatus:status];
      if (commit)
        [self commitReadbackIn:client generation:_generation];
      // A write that carried this key may already be in the document: consume it, never replay
      // it. A FINISH that passes or replans carried only the previous mark, so the key passes.
      return plan.disposition == KR_KEY_CONSUMED;
    }
    if (outcome == KR_REFUSED_BEFORE_WRITE || status != KR_OK) {
      // A nested callback resets Rust, so the stale acknowledgement is not an engine refusal
      if (generation != _generation)
        _inputStatus = KRInputInterrupted;
      else if (status != KR_OK)
        _inputStatus = KRInputEngineUnavailable;
      BOOL wrote =
          outcome == KR_APPLIED_VERIFIED && plan.action != KR_PASS && plan.action != KR_UNAVAILABLE;
      [self invalidate];
      [self noteStatus:status];
      // A verified write that Rust then refused to acknowledge is a contract failure
      if (wrote) {
        _stopped = YES;
        _rearmable = NO;
      }
      return wrote && plan.disposition == KR_KEY_CONSUMED;
    }
    _finishVerified = plan.action == KR_FINISH;
    if (plan.action == KR_MARK || plan.action == KR_FINISH)
      _verifiedWrite = YES;
    if (_owner)
      _ownerMode = event.mode;
    bool changed = plan.mark_len != plan.expected_len ||
                   std::memcmp(plan.expected, plan.mark, plan.expected_len * sizeof(uint16_t)) != 0;
    if (event.role == KR_TEXT && event.len == 1 && event.units[0] == 32 && plan.action == KR_MARK &&
        plan.mark_len == plan.expected_len + 1 &&
        std::memcmp(plan.expected, plan.mark, plan.expected_len * sizeof(uint16_t)) != 0)
      _automaticSwitchMode = event.mode == KR_EN ? KR_RU : KR_EN;
    // Undoing a correction in the mode Keyrill switched to also undoes the switch
    if (kept && ((event.role == KR_REVERSE && plan.action == KR_MARK) ||
                 (event.role == KR_ESCAPE && plan.action == KR_FINISH && changed)))
      _automaticSwitchMode = event.mode;
    if ((plan.disposition == KR_KEY_REPLAN || finishFirst) && plan.action == KR_FINISH)
      continue;
    if (plan.disposition == KR_KEY_PASS)
      return NO;
    if (plan.disposition == KR_KEY_CONSUMED)
      return YES;
    break;
  }
  // A second REPLAN or an unknown disposition: nothing was written for this pass
  [self invalidate];
  return NO;
}

- (BOOL)clientAllowed:(id<IMKTextInput>)client generation:(uint64_t)generation {
  @try {
    NSString *bundle = [client bundleIdentifier];
    BOOL allowed = [bundle isKindOfClass:NSString.class] && bundle.length != 0 &&
                   [_allowedClients containsObject:bundle];
    if (generation == _generation)
      _inputStatus = allowed ? KRInputReady : KRInputExcluded;
    return generation == _generation && allowed && ![self secureAfterQuery];
  } @catch (NSException *) {
    _inputStatus = KRInputClientException;
    return NO;
  }
}

- (uint32_t)contextIn:(id<IMKTextInput>)client generation:(uint64_t)generation {
  _contextLocation = NSNotFound;
  _inputStatus = KRInputContextUnavailable;
  @try {
    NSRange selected = [client selectedRange];
    if (generation != _generation)
      return KR_CTX_INELIGIBLE;
    NSRange marked = [client markedRange];
    if (generation != _generation || selected.location == NSNotFound || selected.length ||
        selected.location > kMaxStart || hasMark(marked))
      return KR_CTX_INELIGIBLE;
    if (selected.location == 0) {
      _contextLocation = 0;
      _inputStatus = KRInputReady;
      return KR_CTX_BOUNDARY;
    }
    if ([self secureAfterQuery])
      return KR_CTX_INELIGIBLE;
    NSRange wanted = NSMakeRange(selected.location - 1, 1);
    NSRange actual = {NSNotFound, 0};
    NSString *before = [client stringFromRange:wanted actualRange:&actual];
    if (generation == _generation && !before)
      _inputStatus = KRInputTextUnavailable;
    if (generation != _generation || before.length != 1 || !NSEqualRanges(actual, wanted))
      return KR_CTX_INELIGIBLE;
    _contextLocation = selected.location;
    bool boundary = isWhitespace([before characterAtIndex:0]);
    _inputStatus = boundary ? KRInputReady : KRInputNotAtBoundary;
    return boundary ? KR_CTX_BOUNDARY : KR_CTX_INELIGIBLE;
  } @catch (NSException *) {
    _inputStatus = KRInputClientException;
    return KR_CTX_INELIGIBLE;
  }
}

// Requires the exact mark, caret and text; reads the client three times and checks the
// generation after each read. A caret of NSNotFound skips the caret read, and readText NO the
// text read
- (BOOL)verifyMark:(NSString *)text
                at:(NSUInteger)start
             caret:(NSUInteger)caret
          readText:(BOOL)readText
            client:(id<IMKTextInput>)client
        generation:(uint64_t)generation {
  NSRange expected = NSMakeRange(start, text.length);
  NSRange marked = [client markedRange];
  if (generation != _generation)
    return NO;
  if (!NSEqualRanges(marked, expected)) {
    _inputStatus = KRInputMarkMismatch;
    return NO;
  }
  if (caret != NSNotFound) {
    NSRange selected = [client selectedRange];
    if (generation != _generation)
      return NO;
    if (!NSEqualRanges(selected, NSMakeRange(caret, 0))) {
      _inputStatus = KRInputCaretMismatch;
      return NO;
    }
  }
  if ([self secureAfterQuery])
    return NO;
  if (!readText) {
    _inputStatus = KRInputReady;
    return YES;
  }
  NSRange actual = {NSNotFound, 0};
  NSString *observed = [client stringFromRange:expected actualRange:&actual];
  if (generation != _generation)
    return NO;
  if (!observed) {
    _inputStatus = KRInputTextUnavailable;
    return NO;
  }
  if (!NSEqualRanges(actual, expected) || ![observed isEqualToString:text]) {
    _inputStatus = KRInputTextMismatch;
    return NO;
  }
  _inputStatus = KRInputReady;
  return YES;
}

- (uint32_t)apply:(const kr_plan &)plan
           client:(id<IMKTextInput>)client
       generation:(uint64_t)generation {
  if (plan.expected_len > KR_TEXT_CAPACITY || plan.mark_len > KR_TEXT_CAPACITY)
    return KR_REFUSED_BEFORE_WRITE;
  NSString *expected = string(plan.expected, plan.expected_len);
  NSString *mark = string(plan.mark, plan.mark_len);
  _checkFailed = NO;
  // Committing the mark unchanged replaces its exact range with the same text, so the caret
  // doesn't matter; clients such as Firefox move it on focus loss before the commit arrives
  bool unchanged =
      plan.action == KR_FINISH && plan.expected_len != 0 && [mark isEqualToString:expected];
  // A commit IMK requests on focus loss writes back the text last verified in the mark: only
  // Keyrill changes its own mark, and Firefox's readback is invalid at that moment
  bool trusted = unchanged && _lifecycleFinish;

  NSUInteger start;
  @try {
    if (plan.expected_len == 0) {
      if (_owner || plan.action != KR_MARK || plan.mark_len == 0)
        return KR_REFUSED_BEFORE_WRITE;
      NSRange selected = [client selectedRange];
      if (generation != _generation)
        return KR_REFUSED_BEFORE_WRITE;
      NSRange marked = [client markedRange];
      if (generation != _generation || hasMark(marked) || selected.length ||
          selected.location == NSNotFound || selected.location != _contextLocation)
        return KR_REFUSED_BEFORE_WRITE;
      start = selected.location;
    } else {
      if (client != _owner || ![expected isEqualToString:_text] ||
          ![self verifyMark:expected
                         at:_start
                      caret:unchanged ? NSNotFound : _start + _text.length
                   readText:!trusted
                     client:client
                 generation:generation])
        return KR_REFUSED_BEFORE_WRITE;
      start = _start;
    }
    if ([self secureAfterQuery])
      return KR_REFUSED_BEFORE_WRITE;
  } @catch (NSException *) {
    _inputStatus = KRInputClientException;
    return KR_REFUSED_BEFORE_WRITE;
  }

  // From here on the outcome is either verified or unknown; local authority is dropped first so
  // that no path can reuse it after the write
  _owner = nil;
  _text = nil;
  @try {
    if (plan.action == KR_MARK)
      [client setMarkedText:[[NSAttributedString alloc] initWithString:mark]
             selectionRange:NSMakeRange(plan.mark_len, 0)
           replacementRange:kCurrentSelection];
    else
      [client insertText:mark
          replacementRange:unchanged ? NSMakeRange(start, mark.length) : kCurrentSelection];
    if (generation != _generation)
      return KR_ATTEMPTED_UNKNOWN;

    if (plan.action == KR_MARK && mark.length) {
      if (![self verifyMark:mark
                         at:start
                      caret:start + plan.mark_len
                   readText:YES
                     client:client
                 generation:generation]) {
        _checkFailed = generation == _generation && _inputStatus != KRInputSecure;
        return KR_ATTEMPTED_UNKNOWN;
      }
      _owner = client;
      _start = start;
      _text = mark;
      return KR_APPLIED_VERIFIED;
    }

    NSRange marked = [client markedRange];
    if (generation != _generation)
      return KR_ATTEMPTED_UNKNOWN;
    if (hasMark(marked)) {
      _inputStatus = KRInputMarkMismatch;
      _checkFailed = YES;
      return KR_ATTEMPTED_UNKNOWN;
    }
    if (!unchanged) {
      NSRange selected = [client selectedRange];
      if (generation != _generation)
        return KR_ATTEMPTED_UNKNOWN;
      if (!NSEqualRanges(selected, NSMakeRange(start + mark.length, 0))) {
        _inputStatus = KRInputCaretMismatch;
        _checkFailed = YES;
        return KR_ATTEMPTED_UNKNOWN;
      }
    }
    if (mark.length && !trusted) {
      if ([self secureAfterQuery])
        return KR_ATTEMPTED_UNKNOWN;
      NSRange expectedRange = NSMakeRange(start, mark.length);
      NSRange actual = {NSNotFound, 0};
      NSString *observed = [client stringFromRange:expectedRange actualRange:&actual];
      if (generation != _generation)
        return KR_ATTEMPTED_UNKNOWN;
      _checkFailed = YES;
      if (!observed) {
        _inputStatus = KRInputTextUnavailable;
        return KR_ATTEMPTED_UNKNOWN;
      }
      if (!NSEqualRanges(actual, expectedRange) || ![observed isEqualToString:mark]) {
        _inputStatus = KRInputTextMismatch;
        return KR_ATTEMPTED_UNKNOWN;
      }
      _checkFailed = NO;
    }
    _inputStatus = KRInputReady;
    return KR_APPLIED_VERIFIED;
  } @catch (NSException *) {
    _checkFailed = NO;
    _inputStatus = KRInputClientException;
    return KR_ATTEMPTED_UNKNOWN;
  }
}

// After a failed readback check, a client such as NSTextView replaces its marked text with the
// next passed key. Committing the client's own readback of the marked range once keeps what the
// user sees; nothing is written when the mark or its exact readback is unavailable.
- (void)commitReadbackIn:(id<IMKTextInput>)client generation:(uint64_t)generation {
  @try {
    NSRange marked = [client markedRange];
    if (generation != _generation || !hasMark(marked) || [self secureAfterQuery])
      return;
    NSRange actual = {NSNotFound, 0};
    NSString *readback = [client stringFromRange:marked actualRange:&actual];
    if (generation != _generation || ![readback isKindOfClass:NSString.class] ||
        readback.length != marked.length || !NSEqualRanges(actual, marked) ||
        [self secureAfterQuery])
      return;
    [client insertText:readback replacementRange:marked];
  } @catch (NSException *) {
    _inputStatus = KRInputClientException;
  }
}

@end
