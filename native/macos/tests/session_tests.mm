// Controlled native tests: KRSession, KRCompatibilitySession and the controller against the real
// Rust engine, a scripted in-process IMKTextInput client and an offscreen NSTextView. No window,
// input source, event tap or host application is involved, so these results are controlled-native
// evidence only, never system IMK evidence.
#import <Carbon/Carbon.h>
#import <Foundation/Foundation.h>

#include <cstdio>
#include <cstring>
#include <functional>
#include <sys/stat.h>
#include <utility>
#include <vector>

#import "../src/KRCompatibility.h"
#import "../src/KRController.h"
#import "../src/KRKeys.h"
#import "../src/KRRuntime.h"
#import "../src/KRSession.h"

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

static const NSString *kTestBundle = @"org.keyrill.test.client";

@interface FakeClient : NSObject <IMKTextInput>
@property(nonatomic) NSMutableString *document;
@property(nonatomic) NSRange selection;
@property(nonatomic) NSRange marked;
@property(nonatomic, copy) NSString *bundle;
@property(nonatomic) NSUInteger calls;
@property(nonatomic) NSUInteger writes;
@property(nonatomic) NSUInteger markWrites;
@property(nonatomic) BOOL throwAfterInsert;
@property(nonatomic) BOOL noTextReadback;
/// Returns object replacement characters over the exact requested range, as Firefox does for
/// a composition while it loses focus.
@property(nonatomic) BOOL garbleReadback;
/// Misbehaving hosts: keep the mark after insertText, insert altered same-length text, or leave
/// the caret at the document start.
@property(nonatomic) BOOL keepMarkOnInsert;
@property(nonatomic) BOOL alterInsert;
@property(nonatomic) BOOL misplaceCaretOnInsert;
@property(nonatomic) NSUInteger emptyInserts;
@property(nonatomic) NSUInteger emptyMarks;
@property(nonatomic) BOOL ignoreWrites;
/// Reports the caret at the document start after a marked-text write, as a misbehaving host.
@property(nonatomic) BOOL misplaceCaretOnMark;
/// Input modes requested through selectInputMode, in order.
@property(nonatomic) NSMutableArray<NSString *> *selectedModes;
/// Invoked at the entry of every client call (and after each write's effect) with its name.
@property(nonatomic, copy) void (^hook)(NSString *call);
- (instancetype)initWithText:(NSString *)text caret:(NSUInteger)caret;
- (void)enter:(NSString *)call;
- (void)pressKey:(NSString *)text;
@end

@implementation FakeClient

- (instancetype)initWithText:(NSString *)text caret:(NSUInteger)caret {
  if ((self = [super init])) {
    _document = [text mutableCopy];
    _selection = NSMakeRange(caret, 0);
    _marked = NSMakeRange(NSNotFound, 0);
    _bundle = [kTestBundle copy];
    _selectedModes = [NSMutableArray array];
  }
  return self;
}

// A key the input method passed, applied as NSTextView applies it: it replaces marked text
- (void)pressKey:(NSString *)text {
  NSRange target = [self target];
  [_document replaceCharactersInRange:target withString:text];
  _marked = NSMakeRange(NSNotFound, 0);
  _selection = NSMakeRange(target.location + text.length, 0);
}

- (void)enter:(NSString *)call {
  ++_calls;
  if (_hook)
    _hook(call);
}

- (NSRange)target {
  return _marked.location != NSNotFound && _marked.length ? _marked : _selection;
}

- (void)setMarkedText:(id)string
       selectionRange:(NSRange)selectionRange
     replacementRange:(NSRange)r {
  (void)r;
  // A write counts as attempted from its entry, before any effect is visible
  ++_writes;
  ++_markWrites;
  NSString *text = [string isKindOfClass:NSAttributedString.class] ? [string string] : string;
  if (!text.length)
    ++_emptyMarks;
  [self enter:@"setMarkedText"];
  if (!_ignoreWrites) {
    NSRange target = [self target];
    [_document replaceCharactersInRange:target withString:text];
    _marked = text.length ? NSMakeRange(target.location, text.length) : NSMakeRange(NSNotFound, 0);
    _selection =
        NSMakeRange(_misplaceCaretOnMark ? 0 : target.location + selectionRange.location, 0);
  }
  [self enter:@"setMarkedText+after"];
}

- (void)insertText:(id)string replacementRange:(NSRange)r {
  ++_writes;
  NSString *text = [string isKindOfClass:NSAttributedString.class] ? [string string] : string;
  if (!text.length)
    ++_emptyInserts;
  [self enter:@"insertText"];
  if (!_ignoreWrites) {
    NSRange target = r.location == NSNotFound ? [self target] : r;
    if (_alterInsert && text.length)
      text = [[text substringToIndex:text.length - 1] stringByAppendingString:@"#"];
    [_document replaceCharactersInRange:target withString:text];
    _marked =
        _keepMarkOnInsert ? NSMakeRange(target.location, text.length) : NSMakeRange(NSNotFound, 0);
    _selection = NSMakeRange(_misplaceCaretOnInsert ? 0 : target.location + text.length, 0);
  }
  [self enter:@"insertText+after"];
  if (_throwAfterInsert)
    [NSException raise:@"KRTestFault" format:@"after insert"];
}

- (NSRange)selectedRange {
  [self enter:@"selectedRange"];
  return _selection;
}

- (NSRange)markedRange {
  [self enter:@"markedRange"];
  return _marked;
}

- (NSString *)stringFromRange:(NSRange)range actualRange:(NSRangePointer)actual {
  [self enter:@"stringFromRange"];
  if (_noTextReadback) {
    if (actual)
      *actual = NSMakeRange(NSNotFound, 0);
    return nil;
  }
  if (range.location > _document.length) {
    if (actual)
      *actual = NSMakeRange(NSNotFound, 0);
    return nil;
  }
  NSRange clamped =
      NSMakeRange(range.location, MIN(range.length, _document.length - range.location));
  if (actual)
    *actual = clamped;
  if (_garbleReadback)
    return [@"" stringByPaddingToLength:clamped.length withString:@"\uFFFC" startingAtIndex:0];
  return [_document substringWithRange:clamped];
}

- (NSAttributedString *)attributedSubstringFromRange:(NSRange)range {
  NSString *text = [self stringFromRange:range actualRange:nullptr];
  return text ? [[NSAttributedString alloc] initWithString:text] : nil;
}

- (NSString *)bundleIdentifier {
  [self enter:@"bundleIdentifier"];
  return _bundle;
}

- (NSInteger)length {
  return static_cast<NSInteger>(_document.length);
}
- (NSInteger)characterIndexForPoint:(NSPoint)point
                           tracking:(IMKLocationToOffsetMappingMode)mode
                      inMarkedRange:(BOOL *)inMarkedRange {
  (void)point, (void)mode, (void)inMarkedRange;
  return NSNotFound;
}
- (NSDictionary *)attributesForCharacterIndex:(NSUInteger)index lineHeightRectangle:(NSRect *)rect {
  (void)index, (void)rect;
  return @{};
}
- (NSArray *)validAttributesForMarkedText {
  return @[];
}
- (void)overrideKeyboardWithKeyboardNamed:(NSString *)name {
  (void)name;
}
- (void)selectInputMode:(NSString *)mode {
  [_selectedModes addObject:mode];
  [self enter:@"selectInputMode"];
}
- (BOOL)supportsUnicode {
  return YES;
}
- (CGWindowLevel)windowLevel {
  return 0;
}
- (BOOL)supportsProperty:(TSMDocumentPropertyTag)property {
  (void)property;
  return NO;
}
- (NSString *)uniqueClientIdentifierString {
  return @"fake";
}
- (NSRect)firstRectForCharacterRange:(NSRange)range actualRange:(NSRangePointer)actual {
  (void)range, (void)actual;
  return NSZeroRect;
}

@end

/// IMKTextInput over a real offscreen NSTextView. Writes and reads go to the text view; the only
/// scripted behavior is an optional fault in the client's report after a marked-text write.
@interface TextViewClient : FakeClient
@property(nonatomic, readonly) NSTextView *view;
/// After the next marked-text write, report the caret at the document start once.
@property(nonatomic) BOOL misreportCaretOnce;
/// After the next marked-text write, return no text for any range.
@property(nonatomic) BOOL dropReadbackAfterMark;
@end

@implementation TextViewClient {
  BOOL _misreport;
  BOOL _noReadback;
}

- (instancetype)initWithText:(NSString *)text caret:(NSUInteger)caret {
  if ((self = [super initWithText:text caret:caret])) {
    _view = [[NSTextView alloc] initWithFrame:NSMakeRect(0, 0, 400, 100)];
    _view.string = text;
    _view.selectedRange = NSMakeRange(caret, 0);
    self.bundle = @"com.apple.TextEdit";
  }
  return self;
}

- (void)setMarkedText:(id)string
       selectionRange:(NSRange)selectionRange
     replacementRange:(NSRange)r {
  ++self.writes;
  [self enter:@"setMarkedText"];
  [_view setMarkedText:string selectedRange:selectionRange replacementRange:r];
  _misreport = self.misreportCaretOnce;
  _noReadback = self.dropReadbackAfterMark;
  self.misreportCaretOnce = NO;
  self.dropReadbackAfterMark = NO;
}

- (void)insertText:(id)string replacementRange:(NSRange)r {
  ++self.writes;
  [self enter:@"insertText"];
  [_view insertText:string replacementRange:r];
}

- (NSRange)selectedRange {
  [self enter:@"selectedRange"];
  if (_misreport) {
    _misreport = NO;
    return NSMakeRange(0, 0);
  }
  return _view.selectedRange;
}

- (NSRange)markedRange {
  [self enter:@"markedRange"];
  return _view.hasMarkedText ? _view.markedRange : NSMakeRange(NSNotFound, 0);
}

- (NSString *)stringFromRange:(NSRange)range actualRange:(NSRangePointer)actual {
  [self enter:@"stringFromRange"];
  if (_noReadback)
    return nil;
  return [_view attributedSubstringForProposedRange:range actualRange:actual].string;
}

@end

/// Forwards to a key event and counts reads of its characters.
@interface EventSpy : NSProxy
@property(nonatomic, readonly) NSUInteger characterReads;
- (instancetype)initWithEvent:(NSEvent *)event;
@end

@implementation EventSpy {
  NSEvent *_event;
}

- (instancetype)initWithEvent:(NSEvent *)event {
  _event = event;
  return self;
}

- (NSString *)characters {
  ++_characterReads;
  return _event.characters;
}

- (NSMethodSignature *)methodSignatureForSelector:(SEL)selector {
  return [_event methodSignatureForSelector:selector];
}

- (void)forwardInvocation:(NSInvocation *)invocation {
  [invocation invokeWithTarget:_event];
}

@end

namespace {

KRSession *adopt(kr_engine *engine, uint32_t mode, NSSet<NSString *> *allowed = nil) {
  KRSession *session =
      [[KRSession alloc] initWithEngine:engine
                         allowedClients:allowed ? allowed : [NSSet setWithObject:kTestBundle]];
  session.secureInputEnabled = ^BOOL {
    return NO;
  };
  session.mode = mode;
  [session activate];
  return session;
}

KRSession *makeSession(bool fixtures, uint32_t mode = KR_EN, NSSet<NSString *> *allowed = nil) {
  kr_engine *engine = nullptr;
  if (kr_engine_new(KR_ABI_VERSION, &engine) != KR_OK || !engine)
    return nil;
  if (kr_set_automatic_policy(engine, fixtures ? KR_POLICY_FIXTURE : KR_POLICY_OFF) != KR_OK) {
    kr_engine_free(engine);
    return nil;
  }
  return adopt(engine, mode, allowed);
}

// The session owns the engine; `engine` stays a borrowed handle for configuration calls
KRSession *makeModelSession(kr_engine *&engine, uint32_t mode = KR_EN,
                            NSSet<NSString *> *allowed = nil) {
  engine = nullptr;
  if (kr_engine_new(KR_ABI_VERSION, &engine) != KR_OK || !engine)
    return nil;
  if (kr_set_automatic_policy(engine, KR_POLICY_MODEL) != KR_OK) {
    kr_engine_free(engine);
    engine = nullptr;
    return nil;
  }
  return adopt(engine, mode, allowed);
}

// Policy OFF does no source work, so the executor and queue never run
KRRuntime *offRuntime(NSDictionary *preferences) {
  return [[KRRuntime alloc] initWithPreferences:preferences
      homeResolver:^NSString * {
        return nil;
      }
      workQueue:dispatch_queue_create("org.keyrill.test.off", nullptr)
      mainExecutor:^(dispatch_block_t block) {
        (void)block;
      }
      observationInterval:0];
}

uint32_t publish(kr_engine *engine, uint32_t language, const char *source) {
  return kr_set_dictionary_utf8(engine, language, reinterpret_cast<const uint8_t *>(source),
                                static_cast<uint32_t>(std::strlen(source)));
}

bool publishEmpty(kr_engine *engine) {
  return kr_set_dictionary_utf8(engine, KR_EN, nullptr, 0) == KR_OK &&
         kr_set_dictionary_utf8(engine, KR_RU, nullptr, 0) == KR_OK;
}

kr_event textEvent(NSString *text, uint32_t mode) {
  kr_event event = {KR_TEXT, mode, KR_CTX_INELIGIBLE, static_cast<uint32_t>(text.length), {}};
  [text getCharacters:event.units range:NSMakeRange(0, text.length)];
  return event;
}

kr_event roleEvent(uint32_t role, uint32_t mode) { return {role, mode, KR_CTX_INELIGIBLE, 0, {}}; }

BOOL key(KRSession *session, FakeClient *client, uint32_t role) {
  kr_event event = roleEvent(role, session.mode);
  return [session handleEvent:&event client:client];
}

// Feeds each UTF-16 unit as one text key and requires every key to be consumed
bool type(KRSession *session, FakeClient *client, NSString *text) {
  bool all = true;
  for (NSUInteger i = 0; i < text.length; ++i) {
    kr_event event = textEvent([text substringWithRange:NSMakeRange(i, 1)], session.mode);
    all = [session handleEvent:&event client:client] && all;
  }
  return all;
}

bool documentIs(FakeClient *client, NSString *text, NSUInteger caret, NSRange marked) {
  bool markMatches = (marked.location == NSNotFound && client.marked.location == NSNotFound) ||
                     NSEqualRanges(marked, client.marked);
  return [client.document isEqualToString:text] &&
         NSEqualRanges(client.selection, NSMakeRange(caret, 0)) && markMatches;
}

constexpr NSRange kNoMark = {NSNotFound, 0};
NSString *const kPrefix = @"pre ";
NSString *const kSuffix = @"\nsuf";

FakeClient *fixtureClient() {
  return [[FakeClient alloc] initWithText:[kPrefix stringByAppendingString:kSuffix]
                                    caret:kPrefix.length];
}

NSString *withAffixes(NSString *middle) {
  return [NSString stringWithFormat:@"%@%@%@", kPrefix, middle, kSuffix];
}

NSUInteger after(NSString *middle) { return kPrefix.length + middle.length; }

void testFixtureBothDirections() {
  currentTest = "fixture_both_directions";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "),
                     NSMakeRange(kPrefix.length, 7)));
    CHECK(type(session, client, @"hello ghbdtn "));
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"привет hello привет "), after(@"привет hello привет "),
                     kNoMark));
    CHECK(!session.owned);
  }
  {
    KRSession *session = makeSession(true, KR_RU);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"руддщ "));
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"hello "), after(@"hello "), kNoMark));
  }
  {
    KRSession *session = makeSession(false);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), kNoMark));
  }
}

void testManualReversalAndExpiry() {
  currentTest = "manual_reversal_expiry";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  CHECK(type(session, client, @"ghbdtn"));
  CHECK(key(session, client, KR_MANUAL));
  CHECK(documentIs(client, withAffixes(@"привет"), after(@"привет"), NSMakeRange(4, 6)));
  CHECK(type(session, client, @" "));
  CHECK(key(session, client, KR_REVERSE));
  CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), NSMakeRange(4, 7)));
  CHECK(type(session, client, @"hello "));
  [session finishInClient:client];
  NSUInteger writes = client.writes;
  // Reversal authority expired with the finished mark: consumed as unavailable, zero writes
  CHECK(key(session, client, KR_REVERSE));
  CHECK(client.writes == writes);
  CHECK(documentIs(client, withAffixes(@"ghbdtn hello "), after(@"ghbdtn hello "), kNoMark));
}

void testAutomaticSwitchRequiresVerifiedCorrectionAndFinish() {
  currentTest = "automatic_switch_verified_correction_and_finish";
  for (uint32_t mode : {KR_EN, KR_RU}) {
    KRSession *session = makeSession(true, mode);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, mode == KR_EN ? @"ghbdtn" : @"руддщ"));
    CHECK(session.automaticSwitchMode == 0);
    CHECK(type(session, client, @" "));
    CHECK(session.automaticSwitchMode == (mode == KR_EN ? KR_RU : KR_EN));
    CHECK([session finishInClient:client]);
    NSString *text = mode == KR_EN ? @"привет " : @"hello ";
    CHECK(documentIs(client, withAffixes(text), after(text), kNoMark));
    CHECK(!session.owned && session.automaticSwitchMode == 0);
    CHECK(![session finishInClient:client]);
  }
  KRSession *manual = makeSession(true);
  FakeClient *client = fixtureClient();
  CHECK(type(manual, client, @"ghbdtn"));
  CHECK(key(manual, client, KR_MANUAL));
  CHECK(type(manual, client, @" "));
  CHECK(manual.automaticSwitchMode == 0);

  KRSession *unknown = makeSession(true);
  FakeClient *ignored = fixtureClient();
  CHECK(type(unknown, ignored, @"ghbdtn"));
  ignored.ignoreWrites = YES;
  CHECK(type(unknown, ignored, @" "));
  CHECK(unknown.stopped && unknown.automaticSwitchMode == 0);
  CHECK(![unknown finishInClient:ignored]);

  KRSession *failedFinish = makeSession(true);
  FakeClient *kept = fixtureClient();
  CHECK(type(failedFinish, kept, @"ghbdtn "));
  CHECK(failedFinish.automaticSwitchMode == KR_RU);
  kept.keepMarkOnInsert = YES;
  CHECK(![failedFinish finishInClient:kept]);
  CHECK(failedFinish.stopped && failedFinish.automaticSwitchMode == 0);
}

void testEscapeRestoresOriginal() {
  currentTest = "escape_restores_original";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  CHECK(type(session, client, @"ghbdtn "));
  CHECK(key(session, client, KR_ESCAPE));
  CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), kNoMark));
  CHECK(!session.owned);
}

void testBoundaryThenPunctuationPassesOnce() {
  currentTest = "boundary_punctuation";
  for (NSString *punctuation in @[ @"!", @"(", @"-", @"?" ]) {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    NSUInteger writes = client.writes;
    kr_event event = textEvent(punctuation, KR_EN);
    CHECK(![session handleEvent:&event client:client]);
    CHECK(client.writes == writes + 1 && !session.owned);
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), kNoMark));
  }
  currentTest = "boundary_provisional_letter";
  {
    // In English mode `,` types б: after a retained mark it finishes the mark and starts a word
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    kr_event comma = textEvent(@",", KR_EN);
    CHECK([session handleEvent:&comma client:client]);
    CHECK(session.owned);
    CHECK(documentIs(client, withAffixes(@"привет ,"), after(@"привет ,"),
                     NSMakeRange(after(@"привет "), 1)));
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"привет ,"), after(@"привет ,"), kNoMark));
  }
}

// The English-mode keys that type Russian letters start and continue a word, so a word typed
// in the wrong layout converts as a whole
void testProvisionalLettersConvert() {
  currentTest = "provisional_letters_convert";
  for (NSString *raw in @[ @"[jhjij ", @";bpym " ]) {
    kr_engine *engine;
    KRSession *session = makeModelSession(engine);
    CHECK(publishEmpty(engine));
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, raw));
    NSString *converted = [raw hasPrefix:@"["] ? @"хорошо " : @"жизнь ";
    CHECK(documentIs(client, withAffixes(converted), after(converted),
                     NSMakeRange(kPrefix.length, converted.length)));
    CHECK(key(session, client, KR_REVERSE));
    CHECK(
        documentIs(client, withAffixes(raw), after(raw), NSMakeRange(kPrefix.length, raw.length)));
    CHECK([session finishInClient:client]);
    CHECK(documentIs(client, withAffixes(raw), after(raw), kNoMark));
  }
}

void testEmptyComposition() {
  currentTest = "empty_composition";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  CHECK(type(session, client, @"x"));
  CHECK(key(session, client, KR_BACKSPACE));
  CHECK(client.emptyMarks == 1 && client.emptyInserts == 0);
  CHECK(documentIs(client, withAffixes(@""), after(@""), kNoMark));
  CHECK(!session.owned);
  CHECK(type(session, client, @"hello "));
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(@"hello "), after(@"hello "), kNoMark));
}

void testForeignMarkAndMovedCaret() {
  currentTest = "foreign_mark";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    client.marked = NSMakeRange(0, 3);
    kr_event event = textEvent(@"g", KR_EN);
    CHECK(![session handleEvent:&event client:client]);
    CHECK(client.writes == 0);
    CHECK(documentIs(client, withAffixes(@""), after(@""), NSMakeRange(0, 3)));
  }
  currentTest = "moved_caret";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"g"));
    client.selection = NSMakeRange(0, 0);
    NSUInteger writes = client.writes;
    kr_event event = textEvent(@"h", KR_EN);
    CHECK(![session handleEvent:&event client:client]);
    CHECK(client.writes == writes);
    CHECK(documentIs(client, withAffixes(@"g"), 0, NSMakeRange(4, 1)));
    CHECK(!session.owned);
  }
}

void testCaretMovedAfterContextRead() {
  currentTest = "caret_moved_after_context";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  __weak FakeClient *weakClient = client;
  client.hook = ^(NSString *call) {
    // The boundary context was read at "pre |"; the caret then moves inside "pre"
    if ([call isEqualToString:@"stringFromRange"])
      weakClient.selection = NSMakeRange(1, 0);
  };
  kr_event event = textEvent(@"g", KR_EN);
  CHECK(![session handleEvent:&event client:client]);
  CHECK(client.writes == 0);
  CHECK(documentIs(client, withAffixes(@""), 1, kNoMark));
}

void testClientChangeLeavesOldClientUntouched() {
  currentTest = "client_change";
  KRSession *session = makeSession(true);
  FakeClient *first = fixtureClient();
  FakeClient *second = fixtureClient();
  CHECK(type(session, first, @"g"));
  NSUInteger calls = first.calls;
  CHECK(type(session, second, @"h"));
  [session finishInClient:second];
  CHECK(first.calls == calls);
  CHECK(documentIs(first, withAffixes(@"g"), after(@"g"), NSMakeRange(4, 1)));
  CHECK(documentIs(second, withAffixes(@"h"), after(@"h"), kNoMark));
}

void testModeMismatchMakesNoWrite() {
  currentTest = "mode_mismatch";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  CHECK(type(session, client, @"g"));
  NSUInteger writes = client.writes;
  session.mode = KR_RU;
  kr_event event = textEvent(@"р", KR_RU);
  CHECK(![session handleEvent:&event client:client]);
  [session finishInClient:client];
  CHECK(client.writes == writes);
  CHECK(documentIs(client, withAffixes(@"g"), after(@"g"), NSMakeRange(4, 1)));
}

void testIgnoredWriteIsUnknownAndStops() {
  currentTest = "ignored_write";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  client.ignoreWrites = YES;
  kr_event event = textEvent(@"g", KR_EN);
  CHECK([session handleEvent:&event client:client]);
  CHECK(session.stopped);
  NSUInteger calls = client.calls;
  CHECK(![session handleEvent:&event client:client]);
  CHECK(key(session, client, KR_MANUAL));
  CHECK(client.calls == calls);
  [session activate];
  CHECK(!session.stopped);
}

void testReconfigurationFinishesOrKeepsTheStop() {
  currentTest = "reconfigure_verified_finish";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    CHECK([session finishForReconfigurationInClient:client]);
    CHECK(!session.owned && !session.stopped);
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), kNoMark));
    session = makeSession(false);
    CHECK(type(session, client, @"ghbdtn "));
    CHECK(key(session, client, KR_MANUAL));
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"привет привет "), after(@"привет привет "), kNoMark));
  }
  currentTest = "reconfigure_unknown_finish";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.throwAfterInsert = YES;
    CHECK(![session finishForReconfigurationInClient:client]);
    CHECK(session.stopped && !session.owned);
    // The word had verified writes, so the next one starts from fresh boundary checks
    kr_event event = textEvent(@"x", KR_EN);
    CHECK([session handleEvent:&event client:client]);
    CHECK(!session.stopped && session.owned);
    CHECK(documentIs(client, withAffixes(@"привет x"), after(@"привет x"),
                     NSMakeRange(after(@"привет "), 1)));
  }
  currentTest = "reconfigure_reentrant_finish";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    __weak KRSession *weakSession = session;
    __weak FakeClient *weakClient = client;
    __block BOOL fired = NO;
    client.hook = ^(NSString *call) {
      if (fired || ![call isEqualToString:@"insertText"])
        return;
      fired = YES;
      NSUInteger calls = weakClient.calls;
      CHECK(![weakSession finishForReconfigurationInClient:weakClient]);
      CHECK(weakClient.calls == calls);
    };
    CHECK(![session finishForReconfigurationInClient:client]);
    CHECK(fired && session.stopped && !session.owned);
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), kNoMark));
    client.hook = nil;
  }
}

void testUnknownFinishPassesFreshKeyOnce() {
  currentTest = "unknown_finish_then_letter";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.throwAfterInsert = YES;
    NSUInteger marks = client.markWrites;
    kr_event letter = textEvent(@"h", KR_EN);
    // FINISH never carried "h": it passes unhandled once and is never replanned or replayed
    CHECK(![session handleEvent:&letter client:client]);
    CHECK(session.stopped);
    CHECK(client.markWrites == marks);
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), kNoMark));
  }
  currentTest = "unknown_finish_then_shortcut";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn"));
    client.throwAfterInsert = YES;
    CHECK(!key(session, client, KR_FINISH_REQUEST));
    CHECK(session.stopped);
    NSUInteger calls = client.calls;
    CHECK(!key(session, client, KR_FINISH_REQUEST));
    CHECK(client.calls == calls);
  }
  currentTest = "unknown_escape_consumed";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.throwAfterInsert = YES;
    CHECK(key(session, client, KR_ESCAPE));
    CHECK(session.stopped);
  }
}

void testMisbehavingFinishIsUnknown() {
  const char *names[] = {"finish_kept_mark", "finish_altered_text"};
  for (int variant = 0; variant < 2; ++variant) {
    currentTest = names[variant];
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.keepMarkOnInsert = variant == 0;
    client.alterInsert = variant == 1;
    NSUInteger marks = client.markWrites;
    kr_event letter = textEvent(@"h", KR_EN);
    CHECK(![session handleEvent:&letter client:client]);
    CHECK(session.stopped);
    CHECK(client.markWrites == marks);
  }
  currentTest = "lifecycle_commit_unreadable_text";
  {
    // A commit IMK requests on focus loss writes back the text last verified in the mark and
    // ends the composition, so the next word starts normally
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.garbleReadback = YES;
    client.selection = NSMakeRange(0, 0);
    NSUInteger writes = client.writes;
    CHECK([session finishInClient:client]);
    CHECK(client.writes == writes + 1 && !session.stopped && !session.owned);
    CHECK([client.document isEqualToString:withAffixes(@"привет ")]);
    CHECK(client.marked.location == NSNotFound);
    client.garbleReadback = NO;
    client.selection = NSMakeRange(after(@"привет "), 0);
    CHECK(type(session, client, @"ghbdtn "));
    CHECK(documentIs(client, withAffixes(@"привет привет "), after(@"привет привет "),
                     NSMakeRange(after(@"привет "), 7)));
  }
  currentTest = "typed_finish_unreadable_text";
  {
    // A typed key's finish still reads the text back: the same readback refuses it
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.garbleReadback = YES;
    NSUInteger writes = client.writes;
    kr_event letter = textEvent(@"h", KR_EN);
    CHECK(![session handleEvent:&letter client:client]);
    CHECK(client.writes == writes && !session.owned);
    CHECK(session.inputStatus == KRInputTextMismatch);
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "),
                     NSMakeRange(kPrefix.length, 7)));
  }
  {
    // A lifecycle commit still needs the exact marked range
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.garbleReadback = YES;
    client.marked = NSMakeRange(kPrefix.length, 6);
    NSUInteger writes = client.writes;
    CHECK(![session finishInClient:client]);
    CHECK(client.writes == writes && session.inputStatus == KRInputMarkMismatch);
  }
  currentTest = "finish_unchanged_moved_caret";
  {
    // Firefox moves the caret on focus loss before the commit: the unchanged mark is replaced
    // by range and verified by its text, so no mark is left behind
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.selection = NSMakeRange(0, 0);
    client.misplaceCaretOnInsert = YES;
    NSUInteger writes = client.writes;
    CHECK([session finishInClient:client]);
    CHECK(client.writes == writes + 1 && !session.stopped && !session.owned);
    CHECK(documentIs(client, withAffixes(@"привет "), 0, kNoMark));
  }
  currentTest = "finish_changed_misplaced_caret";
  {
    // Restoring the typed word changes the text, so the caret is still required
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.misplaceCaretOnInsert = YES;
    CHECK(key(session, client, KR_ESCAPE));
    CHECK(session.stopped && !session.owned);
    CHECK([client.document isEqualToString:withAffixes(@"ghbdtn ")]);
  }
  currentTest = "finish_changed_moved_caret";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.selection = NSMakeRange(0, 0);
    NSUInteger writes = client.writes;
    CHECK(!key(session, client, KR_ESCAPE));
    CHECK(client.writes == writes && !session.stopped && !session.owned);
    CHECK(documentIs(client, withAffixes(@"привет "), 0, NSMakeRange(kPrefix.length, 7)));
  }
}

void testForeignMarkAfterContextRead() {
  currentTest = "foreign_mark_after_context";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  __weak FakeClient *weakClient = client;
  client.hook = ^(NSString *call) {
    // Another composition appears at the caret after the boundary context was read
    if ([call isEqualToString:@"stringFromRange"])
      weakClient.marked = NSMakeRange(1, 2);
  };
  kr_event event = textEvent(@"g", KR_EN);
  CHECK(![session handleEvent:&event client:client]);
  CHECK(client.writes == 0);
  CHECK(documentIs(client, withAffixes(@""), after(@""), NSMakeRange(1, 2)));
}

void testSecureAndUnlistedClientsAreUntouched() {
  currentTest = "secure_input";
  {
    KRSession *session = makeSession(true);
    session.secureInputEnabled = ^BOOL {
      return YES;
    };
    FakeClient *client = fixtureClient();
    kr_event event = textEvent(@"g", KR_EN);
    CHECK(![session handleEvent:&event client:client]);
    CHECK(client.calls == 0);
  }
  currentTest = "unlisted_client";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    client.bundle = @"com.example.other";
    kr_event event = textEvent(@"g", KR_EN);
    CHECK(![session handleEvent:&event client:client]);
    CHECK(!key(session, client, KR_MANUAL));
    CHECK(client.writes == 0);
  }
}

void testCompositionClientsAreExactAndFailClosed() {
  currentTest = "composition_client_scope";
  NSSet *allowed = offRuntime(@{}).compositionClients;
  NSSet *expected = [NSSet
      setWithObjects:@"com.apple.TextEdit", @"com.apple.Safari", @"org.mozilla.firefox", nil];
  CHECK([allowed isEqualToSet:expected]);
  NSArray *clients = @[
    @"com.apple.TextEdit", @"com.apple.Safari", @"org.mozilla.firefox", @"com.apple.Safari.Foo",
    @"com.apple.SafariTechnologyPreview", @"org.mozilla.firefoxdeveloperedition",
    @"com.apple.Safari ", @" com.apple.Safari", @"com.example.other"
  ];
  for (NSString *bundle in clients) {
    kr_engine *engine = nullptr;
    CHECK(kr_engine_new(KR_ABI_VERSION, &engine) == KR_OK);
    CHECK(kr_set_automatic_policy(engine, KR_POLICY_FIXTURE) == KR_OK);
    KRSession *session = [[KRSession alloc] initWithEngine:engine allowedClients:allowed];
    session.secureInputEnabled = ^BOOL {
      return NO;
    };
    [session activate];
    FakeClient *client = fixtureClient();
    client.bundle = bundle;
    if ([allowed containsObject:bundle]) {
      CHECK(type(session, client, @"ghbdtn "));
      [session finishInClient:client];
      CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), kNoMark));
    } else {
      client.hook = ^(NSString *call) {
        CHECK([call isEqualToString:@"bundleIdentifier"]);
      };
      kr_event event = textEvent(@"g", KR_EN);
      CHECK(![session handleEvent:&event client:client]);
      CHECK(client.calls == 1 && client.writes == 0 && !session.owned);
      CHECK(documentIs(client, withAffixes(@""), after(@""), kNoMark));
    }
  }
  for (BOOL secure : {NO, YES}) {
    kr_engine *engine = nullptr;
    CHECK(kr_engine_new(KR_ABI_VERSION, &engine) == KR_OK);
    KRSession *session = [[KRSession alloc] initWithEngine:engine allowedClients:allowed];
    session.secureInputEnabled = ^BOOL {
      return secure;
    };
    [session activate];
    FakeClient *client = fixtureClient();
    client.bundle = @"com.apple.Safari";
    client.hook = ^(NSString *call) {
      CHECK([call isEqualToString:@"bundleIdentifier"]);
      [NSException raise:@"KRTestFault" format:@"bundle lookup failed"];
    };
    kr_event event = textEvent(@"g", KR_EN);
    CHECK(![session handleEvent:&event client:client]);
    CHECK(client.calls == (secure ? 0u : 1u) && client.writes == 0 && !session.owned);
    CHECK(documentIs(client, withAffixes(@""), after(@""), kNoMark));
  }
}

// An application the user moved to composition gets the same checks as the built-in ones
void testOverrideAdmissionKeepsAuthorityChecks() {
  currentTest = "override_application_admission";
  NSSet<NSString *> *editor = [NSSet setWithObject:@"com.example.editor"];
  {
    kr_engine *engine = nullptr;
    KRSession *session = makeModelSession(engine, KR_EN, editor);
    CHECK(session && publishEmpty(engine));
    FakeClient *client = fixtureClient();
    client.bundle = @"com.example.editor";
    CHECK(type(session, client, @"ghbdtn "));
    CHECK(key(session, client, KR_REVERSE));
    CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), NSMakeRange(4, 7)));
    CHECK(type(session, client, @"ghbdtn"));
    CHECK(key(session, client, KR_MANUAL));
    CHECK(type(session, client, @" "));
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"ghbdtn привет "), after(@"ghbdtn привет "), kNoMark));
  }
  for (id bundle in @[
         @"", NSNull.null, @42, @KR_BUNDLE_IDENTIFIER, @"com.umputun.agterm", @"com.example.EDITOR"
       ]) {
    KRSession *session = makeSession(true, KR_EN, editor);
    FakeClient *client = fixtureClient();
    client.bundle = bundle == NSNull.null ? nil : bundle;
    client.hook = ^(NSString *call) {
      CHECK([call isEqualToString:@"bundleIdentifier"]);
    };
    kr_event event = textEvent(@"g", KR_EN);
    CHECK(![session handleEvent:&event client:client]);
    CHECK(!key(session, client, KR_MANUAL));
    CHECK(!key(session, client, KR_REVERSE));
    CHECK(client.calls == 3 && client.writes == 0 && !session.owned);
    CHECK(documentIs(client, withAffixes(@""), after(@""), kNoMark));
  }
  for (BOOL secure : {NO, YES}) {
    KRSession *session = makeSession(true, KR_EN, editor);
    session.secureInputEnabled = ^BOOL {
      return secure;
    };
    FakeClient *client = fixtureClient();
    client.bundle = @"com.example.editor";
    client.hook = ^(NSString *call) {
      CHECK([call isEqualToString:@"bundleIdentifier"]);
      [NSException raise:@"KRTestFault" format:@"bundle lookup failed"];
    };
    kr_event event = textEvent(@"g", KR_EN);
    CHECK(![session handleEvent:&event client:client]);
    CHECK(client.calls == (secure ? 0u : 1u) && client.writes == 0 && !session.owned);
  }
  {
    KRSession *session = makeSession(true, KR_EN, editor);
    FakeClient *client = fixtureClient();
    client.bundle = @"com.example.editor";
    client.marked = NSMakeRange(0, 3);
    kr_event event = textEvent(@"g", KR_EN);
    CHECK(![session handleEvent:&event client:client]);
    CHECK(client.writes == 0 && !session.owned);
    CHECK(documentIs(client, withAffixes(@""), after(@""), NSMakeRange(0, 3)));
  }
  {
    KRSession *session = makeSession(true, KR_EN, editor);
    FakeClient *client = fixtureClient();
    client.bundle = @"com.example.editor";
    client.ignoreWrites = YES;
    kr_event event = textEvent(@"g", KR_EN);
    CHECK([session handleEvent:&event client:client]);
    CHECK(session.stopped && client.writes == 1);
    NSUInteger calls = client.calls;
    CHECK(![session handleEvent:&event client:client]);
    CHECK(client.calls == calls && client.writes == 1);
    CHECK(documentIs(client, withAffixes(@""), after(@""), kNoMark));
  }
}

void testLongWordFinishesWithoutSuffixCorrection() {
  currentTest = "long_word";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  NSString *word = [@"" stringByPaddingToLength:64 withString:@"a" startingAtIndex:0];
  CHECK(type(session, client, word));
  kr_event extra = textEvent(@"b", KR_EN);
  CHECK(![session handleEvent:&extra client:client]);
  CHECK(documentIs(client, withAffixes(word), after(word), kNoMark));
  // The host inserts the passed key; a following letter is mid-token and must not compose
  [client.document insertString:@"b" atIndex:after(word)];
  client.selection = NSMakeRange(after(word) + 1, 0);
  NSUInteger writes = client.writes;
  CHECK(![session handleEvent:&extra client:client]);
  CHECK(client.writes == writes);
}

void testOwnerReleaseDuringCallback() {
  currentTest = "owner_release_during_callback";
  for (int action : {0, 1, 2}) {
    __block KRSession *owner;
    @autoreleasepool {
      owner = makeSession(true);
    }
    __weak KRSession *weakSession = owner;
    FakeClient *client = fixtureClient();
    if (action != 0)
      CHECK(type(owner, client, @"x"));
    __block BOOL released = NO;
    client.hook = ^(NSString *) {
      if (!released) {
        released = YES;
        owner = nil;
        CHECK(weakSession != nil);
      }
    };
    __unsafe_unretained KRSession *borrowed = owner;
    @autoreleasepool {
      if (action == 2)
        CHECK([borrowed finishForReconfigurationInClient:client]);
      else if (action == 1)
        [borrowed finishInClient:client];
      else {
        kr_event event = textEvent(@"x", KR_EN);
        CHECK([borrowed handleEvent:&event client:client]);
      }
    }
    CHECK(released);
    CHECK(weakSession == nil);
    CHECK(documentIs(client, withAffixes(@"x"), after(@"x"),
                     action != 0 ? kNoMark : NSMakeRange(kPrefix.length, 1)));
    client.hook = nil;
  }
}

void testPunctuationTokensNeverAcquireSuffix() {
  currentTest = "punctuation_tokens_no_suffix";
  for (NSString *raw in @[ @"(ghbdtn", @"b(hf", @"g-hbdtn" ]) {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    BOOL afterPunctuation = NO;
    NSUInteger suffixWrites = 0;
    for (NSUInteger i = 0; i < raw.length; ++i) {
      NSString *unit = [raw substringWithRange:NSMakeRange(i, 1)];
      kr_event event = textEvent(unit, KR_EN);
      if (![session handleEvent:&event client:client]) {
        [client.document insertString:unit atIndex:client.selection.location];
        client.selection = NSMakeRange(client.selection.location + 1, 0);
      }
      if ([unit isEqualToString:@"("] || [unit isEqualToString:@"-"]) {
        afterPunctuation = YES;
        suffixWrites = client.writes;
      }
      if (afterPunctuation) {
        CHECK(!session.owned);
        CHECK(client.writes == suffixWrites);
      }
    }
    NSUInteger writes = client.writes;
    CHECK(key(session, client, KR_MANUAL));
    CHECK(client.writes == writes);
    CHECK(documentIs(client, withAffixes(raw), after(raw), kNoMark));
  }
}

void testModelWholeDocument() {
  currentTest = "model_whole_document_en";
  {
    kr_engine *engine;
    KRSession *session = makeModelSession(engine);
    CHECK(publishEmpty(engine));
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn rfr ltkf hello "));
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"привет как дела hello "),
                     after(@"привет как дела hello "), kNoMark));
    CHECK(!session.stopped && !session.owned);
  }
  currentTest = "model_whole_document_ru";
  {
    kr_engine *engine;
    KRSession *session = makeModelSession(engine, KR_RU);
    CHECK(publishEmpty(engine));
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"руддщ цщкдв привет "));
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"hello world привет "), after(@"hello world привет "),
                     kNoMark));
  }
}

// Correction requires both sources; the explicit command needs neither
void testModelMissingDictionaries() {
  currentTest = "model_missing_dictionaries";
  kr_engine *engine;
  KRSession *session = makeModelSession(engine);
  FakeClient *client = fixtureClient();
  CHECK(type(session, client, @"ghbdtn "));
  CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), NSMakeRange(4, 7)));
  CHECK(type(session, client, @"ghbdtn"));
  CHECK(key(session, client, KR_MANUAL));
  CHECK(type(session, client, @" "));
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(@"ghbdtn привет "), after(@"ghbdtn привет "), kNoMark));
  CHECK(kr_set_dictionary_utf8(engine, KR_EN, nullptr, 0) == KR_OK);
  CHECK(type(session, client, @"ghbdtn "));
  CHECK(kr_set_dictionary_utf8(engine, KR_RU, reinterpret_cast<const uint8_t *>(""), 0) == KR_OK);
  CHECK(type(session, client, @"ghbdtn "));
  [session finishInClient:client];
  CHECK(kr_clear_dictionary(engine, KR_RU) == KR_OK);
  CHECK(type(session, client, @"ghbdtn "));
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(@"ghbdtn привет ghbdtn привет ghbdtn "),
                   after(@"ghbdtn привет ghbdtn привет ghbdtn "), kNoMark));
}

void testModelDictionaryProtectionAndLastGood() {
  currentTest = "model_dictionary_last_good";
  kr_engine *engine;
  KRSession *session = makeModelSession(engine);
  CHECK(publish(engine, KR_EN, "ghbdtn\n") == KR_OK && publish(engine, KR_RU, "") == KR_OK);
  FakeClient *client = fixtureClient();
  CHECK(type(session, client, @"ghbdtn "));
  // Rejected candidates leave the published protection in force
  CHECK(publish(engine, KR_EN, "/encoding=latin1\n") == KR_DICTIONARY_DATA);
  CHECK(publish(engine, KR_EN, "/encoding=koi8-r\nghbdtn\n") == KR_DICTIONARY_DATA);
  // Still ready, not missing: an unlisted word converts while the listed one stays protected
  CHECK(type(session, client, @"ckjdj ghbdtn "));
  CHECK(publish(engine, KR_EN, "") == KR_OK);
  CHECK(type(session, client, @"ghbdtn "));
  // A forbidden target blocks the conversion from the other language's source
  CHECK(publish(engine, KR_RU, "привет/!\n") == KR_OK);
  CHECK(type(session, client, @"ghbdtn "));
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(@"ghbdtn слово ghbdtn привет ghbdtn "),
                   after(@"ghbdtn слово ghbdtn привет ghbdtn "), kNoMark));
}

void testModelReversalSurvivesDictionaryChange() {
  currentTest = "model_reversal_dictionary_change";
  kr_engine *engine;
  KRSession *session = makeModelSession(engine);
  CHECK(publishEmpty(engine));
  FakeClient *client = fixtureClient();
  CHECK(type(session, client, @"ghbdtn "));
  CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), NSMakeRange(4, 7)));
  // Publication between plans is allowed while owned and does not revisit the shown decision
  CHECK(publish(engine, KR_EN, "ghbdtn\n") == KR_OK);
  CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), NSMakeRange(4, 7)));
  CHECK(key(session, client, KR_REVERSE));
  CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), NSMakeRange(4, 7)));
  CHECK(type(session, client, @"ghbdtn "));
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(@"ghbdtn ghbdtn "), after(@"ghbdtn ghbdtn "), kNoMark));
}

// Configuration attempted from a client callback while a proposal awaits acknowledgement is
// refused without disturbing the token, the receipt or the published sources
void testConfigurationDuringPendingPlan() {
  currentTest = "configuration_during_pending_plan";
  kr_engine *engine;
  KRSession *session = makeModelSession(engine);
  kr_vocabulary *vocabulary = nullptr;
  CHECK(kr_vocabulary_new(KR_ABI_VERSION, &vocabulary) == KR_OK && vocabulary);
  CHECK(publishEmpty(engine));
  FakeClient *client = fixtureClient();
  CHECK(type(session, client, @"ghbdtn"));
  NSUInteger writes = client.writes;
  __block NSUInteger attempts = 0, refusals = 0;
  __weak FakeClient *weakClient = client;
  client.hook = ^(NSString *call) {
    (void)call;
    if (weakClient.writes == writes)
      return;
    ++attempts;
    refusals += publish(engine, KR_EN, "ghbdtn\n") == KR_STATE;
    refusals += kr_clear_dictionary(engine, KR_RU) == KR_STATE;
    refusals += kr_set_automatic_policy(engine, KR_POLICY_OFF) == KR_STATE;
    refusals += kr_set_automatic_policy(engine, KR_POLICY_FIXTURE) == KR_STATE;
    refusals += [session adoptVocabulary:vocabulary] == KR_STATE;
  };
  CHECK(type(session, client, @" "));
  client.hook = nil;
  CHECK(attempts > 0 && refusals == 5 * attempts);
  CHECK(!session.stopped && session.owned);
  CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), NSMakeRange(4, 7)));
  CHECK(key(session, client, KR_REVERSE));
  CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), NSMakeRange(4, 7)));
  CHECK(type(session, client, @"ghbdtn "));
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(@"ghbdtn привет "), after(@"ghbdtn привет "), kNoMark));
  kr_vocabulary_free(vocabulary);
}

void testAdoptionOutsideHostCallbacks() {
  currentTest = "adoption_outside_host_callbacks";
  kr_engine *engine;
  KRSession *session = makeModelSession(engine);
  CHECK(publishEmpty(engine));
  kr_vocabulary *vocabulary = nullptr;
  CHECK(kr_vocabulary_new(KR_ABI_VERSION, &vocabulary) == KR_OK && vocabulary);
  FakeClient *client = fixtureClient();
  NSUInteger calls = client.calls;
  [KRSession endInputCallback];
  CHECK(![KRSession inputCallbackActive]);
  [KRSession beginInputCallback];
  [KRSession beginInputCallback];
  CHECK([KRSession inputCallbackActive]);
  CHECK([session adoptVocabulary:vocabulary] == KR_STATE);
  [KRSession endInputCallback];
  CHECK([KRSession inputCallbackActive]);
  CHECK([session adoptVocabulary:vocabulary] == KR_STATE);
  [KRSession endInputCallback];
  CHECK(![KRSession inputCallbackActive]);
  CHECK(client.calls == calls);

  __block uint32_t offMainStatus = KR_OK;
  __block BOOL offMainActive = NO;
  dispatch_semaphore_t done = dispatch_semaphore_create(0);
  // No owning-thread session access occurs until this refused probe signals completion
  dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{
    [KRSession beginInputCallback];
    offMainActive = [KRSession inputCallbackActive];
    offMainStatus = [session adoptVocabulary:vocabulary];
    [KRSession endInputCallback];
    dispatch_semaphore_signal(done);
  });
  dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
  CHECK(offMainActive && offMainStatus == KR_STATE);
  CHECK(![KRSession inputCallbackActive] && client.calls == calls);
  __block NSUInteger attempts = 0, refusals = 0;
  client.hook = ^(NSString *call) {
    (void)call;
    ++attempts;
    refusals += [session adoptVocabulary:vocabulary] == KR_STATE;
  };
  CHECK(type(session, client, @"ghbdtn "));
  client.hook = nil;
  CHECK(attempts > 0 && refusals == attempts);
  CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), NSMakeRange(4, 7)));

  calls = client.calls;
  CHECK([session adoptVocabulary:vocabulary] == KR_OK);
  CHECK(client.calls == calls && session.owned && !session.stopped);
  kr_vocabulary_free(vocabulary);
  CHECK(key(session, client, KR_REVERSE));
  CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), NSMakeRange(4, 7)));
  CHECK(type(session, client, @"ghbdtn "));
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(@"ghbdtn ghbdtn "), after(@"ghbdtn ghbdtn "), kNoMark));
}

// Observes on a serial queue off the controller thread, as the host contract requires, and returns
// the status and observation
std::pair<uint32_t, uint32_t> observe(dispatch_queue_t queue, kr_vocabulary *vocabulary,
                                      uint32_t language) {
  __block uint32_t status = UINT32_MAX, observation = UINT32_MAX;
  __block BOOL offMain = NO;
  dispatch_semaphore_t done = dispatch_semaphore_create(0);
  dispatch_async(queue, ^{
    offMain = !NSThread.isMainThread;
    status = kr_vocabulary_observe(vocabulary, language, &observation);
    dispatch_semaphore_signal(done);
  });
  dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
  CHECK(offMain);
  return {status, observation};
}

bool configure(kr_vocabulary *vocabulary, uint32_t language, NSString *path) {
  const char *bytes = path.fileSystemRepresentation;
  return kr_vocabulary_configure(vocabulary, language, reinterpret_cast<const uint8_t *>(bytes),
                                 static_cast<uint32_t>(std::strlen(bytes)), 0) == KR_OK;
}

// Settled EN and RU sources are adopted after reset; the source handle is freed before the engine
// uses them, and each file visibly decides one word
void testAdoptedSourcesDecideWholeDocument() {
  currentTest = "adopted_sources_whole_document";
  NSString *root = [NSTemporaryDirectory()
      stringByAppendingPathComponent:[NSString stringWithFormat:@"keyrill-native-source-%d",
                                                                NSProcessInfo.processInfo
                                                                    .processIdentifier]];
  if (mkdir(root.fileSystemRepresentation, 0700) != 0) {
    CHECK(false);
    return;
  }
  NSString *en = [root stringByAppendingPathComponent:@"en.utf-8.add"];
  NSString *ru = [root stringByAppendingPathComponent:@"ru.utf-8.add"];
  CHECK([@"ghbdtn\n" writeToFile:en atomically:NO encoding:NSUTF8StringEncoding error:nil]);
  CHECK([@"слово/!\n" writeToFile:ru atomically:NO encoding:NSUTF8StringEncoding error:nil]);
  kr_vocabulary *vocabulary = nullptr;
  CHECK(kr_vocabulary_new(KR_ABI_VERSION, &vocabulary) == KR_OK && vocabulary);
  CHECK(configure(vocabulary, KR_EN, en) && configure(vocabulary, KR_RU, ru));
  dispatch_queue_t queue = dispatch_queue_create("org.keyrill.test.acquisition", nullptr);
  for (uint32_t language : {KR_EN, KR_RU}) {
    CHECK(observe(queue, vocabulary, language) == std::make_pair(KR_OK, +KR_SOURCE_PENDING));
    CHECK(observe(queue, vocabulary, language) == std::make_pair(KR_OK, +KR_SOURCE_PUBLISHED));
    CHECK(observe(queue, vocabulary, language) == std::make_pair(KR_OK, +KR_SOURCE_UNCHANGED));
  }
  kr_engine *engine;
  KRSession *session = makeModelSession(engine);
  FakeClient *client = fixtureClient();
  [session activate];
  CHECK([session adoptVocabulary:vocabulary] == KR_OK);
  kr_vocabulary_free(vocabulary);
  CHECK(type(session, client, @"cgbcjr "));
  CHECK(documentIs(client, withAffixes(@"список "), after(@"список "), NSMakeRange(4, 7)));
  // Russian context now favors conversion; EN protection and the RU forbidden target refuse it
  CHECK(type(session, client, @"ghbdtn ckjdj "));
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(@"список ghbdtn ckjdj "), after(@"список ghbdtn ckjdj "),
                   kNoMark));
  CHECK(!session.stopped && !session.owned);
  CHECK([NSFileManager.defaultManager removeItemAtPath:root error:nil]);
}

void testRuntimeReloadPreservesWholeDocumentAndReceipt() {
  currentTest = "runtime_reload_whole_document_and_receipt";
  NSString *root = [NSTemporaryDirectory()
      stringByAppendingPathComponent:[NSString stringWithFormat:@"keyrill-native-runtime-%d",
                                                                NSProcessInfo.processInfo
                                                                    .processIdentifier]];
  if (mkdir(root.fileSystemRepresentation, 0700) != 0) {
    CHECK(false);
    return;
  }
  NSString *en = [root stringByAppendingPathComponent:@"en"];
  NSString *ru = [root stringByAppendingPathComponent:@"ru"];
  CHECK([@"" writeToFile:en atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  CHECK([@"" writeToFile:ru atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  dispatch_queue_t queue = dispatch_queue_create("org.keyrill.test.runtime-document", nullptr);
  NSMutableArray<dispatch_block_t> *pending = [NSMutableArray array];
  KRRuntime *runtime = [[KRRuntime alloc] initWithPreferences:@{
    @"AutomaticCorrectionEnabled" : @YES,
    @"EnglishDictionaryPath" : en,
    @"RussianDictionaryPath" : ru
  }
      homeResolver:^NSString * {
        CHECK(false);
        return nil;
      }
      workQueue:queue
      mainExecutor:^(dispatch_block_t block) {
        @synchronized(pending) {
          [pending addObject:block];
        }
      }
      observationInterval:0];
  void (^drain)(void) = ^{
    for (;;) {
      dispatch_sync(queue, ^{
                    });
      NSArray<dispatch_block_t> *blocks;
      @synchronized(pending) {
        blocks = [pending copy];
        [pending removeAllObjects];
      }
      if (!blocks.count)
        return;
      for (dispatch_block_t block in blocks)
        block();
    }
  };
  kr_engine *engine = nullptr;
  CHECK(kr_engine_new(KR_ABI_VERSION, &engine) == KR_OK && engine);
  CHECK([runtime configureEngine:engine] == KR_OK);
  KRSession *session = adopt(engine, KR_EN);
  FakeClient *client = fixtureClient();
  [runtime requestAdoptionForSession:session];
  [runtime start];
  drain();
  CHECK(runtime.status.english == KRSourceStatusPending);
  CHECK(type(session, client, @"ghbdtn "));
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), kNoMark));
  [runtime tick];
  drain();
  CHECK(runtime.status.english == KRSourceStatusReady &&
        runtime.status.russian == KRSourceStatusReady);
  CHECK(type(session, client, @"cgbcjr "));
  NSString *shown = @"ghbdtn список ";
  CHECK(documentIs(client, withAffixes(shown), after(shown), NSMakeRange(after(@"ghbdtn "), 7)));

  CHECK([@"ghbdtn\n" writeToFile:en atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  CHECK([@"слово/!\n" writeToFile:ru atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  NSUInteger calls = client.calls, writes = client.writes;
  [KRSession beginInputCallback];
  [runtime tick];
  [runtime tick];
  drain();
  [KRSession endInputCallback];
  [runtime tick];
  drain();
  CHECK(client.calls == calls && client.writes == writes && session.owned && !session.stopped);
  CHECK(documentIs(client, withAffixes(shown), after(shown), NSMakeRange(after(@"ghbdtn "), 7)));
  CHECK(key(session, client, KR_REVERSE));
  CHECK(type(session, client, @"cgbcjr ghbdtn ckjdj "));
  [session finishInClient:client];
  NSString *protectedText = @"ghbdtn cgbcjr список ghbdtn ckjdj ";
  CHECK(documentIs(client, withAffixes(protectedText), after(protectedText), kNoMark));

  const uint8_t invalid[] = {0};
  CHECK([[NSData dataWithBytes:invalid length:sizeof invalid] writeToFile:en atomically:YES]);
  [runtime tick];
  drain();
  CHECK(runtime.status.english == KRSourceStatusRetained);
  CHECK(type(session, client, @"ghbdtn "));
  [session finishInClient:client];
  NSString *retainedText = [protectedText stringByAppendingString:@"ghbdtn "];
  CHECK(documentIs(client, withAffixes(retainedText), after(retainedText), kNoMark));

  CHECK([@"" writeToFile:en atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  [runtime tick];
  [runtime tick];
  drain();
  CHECK(type(session, client, @"ghbdtn "));
  [session finishInClient:client];
  NSString *finalText = [retainedText stringByAppendingString:@"привет "];
  CHECK(documentIs(client, withAffixes(finalText), after(finalText), kNoMark));
  CHECK(!session.stopped && !session.owned);
  [runtime stop];
  drain();
  CHECK([NSFileManager.defaultManager removeItemAtPath:root error:nil]);
}

enum class Fault { Reenter, Throw };

// Runs the faulted key once to count client calls, then once per call index k with a fault
// injected at call k. Each faulted run must make zero writes after the fault and preserve the
// prefix/suffix. The key is consumed only when the fault follows the attempt of a write that
// carried it (setMarkedText for every swept key); otherwise it passes unhandled once.
void sweep(const char *name, NSString *setup, NSString *faultedKey, Fault fault) {
  currentTest = name;
  NSUInteger total = 0;
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    type(session, client, setup);
    NSUInteger before = client.calls;
    kr_event event = textEvent(faultedKey, KR_EN);
    [session handleEvent:&event client:client];
    total = client.calls - before;
  }
  CHECK(total > 0);
  for (NSUInteger k = 0; k < total; ++k) {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    FakeClient *other = fixtureClient();
    type(session, client, setup);
    NSString *setupDocument = [client.document copy];
    NSUInteger base = client.calls, setupWrites = client.writes,
               setupMarkWrites = client.markWrites;
    __block NSUInteger writesAtFault = NSNotFound, markWritesAtFault = NSNotFound;
    __block BOOL nested = NO;
    __block BOOL fired = NO;
    __weak FakeClient *weakClient = client;
    client.hook = ^(NSString *call) {
      FakeClient *strong = weakClient;
      if (fired || strong.calls - base - 1 != k)
        return;
      (void)call;
      fired = YES;
      writesAtFault = strong.writes;
      markWritesAtFault = strong.markWrites;
      if (fault == Fault::Throw)
        [NSException raise:@"KRTestFault" format:@"injected"];
      kr_event reentrant = textEvent(@"q", KR_EN);
      nested = [session handleEvent:&reentrant client:other];
    };
    kr_event event = textEvent(faultedKey, KR_EN);
    BOOL handled = [session handleEvent:&event client:client];
    CHECK(fired);
    CHECK(!nested);
    CHECK(other.calls == 0);
    CHECK(client.writes == writesAtFault);
    CHECK([client.document hasPrefix:kPrefix] && [client.document hasSuffix:kSuffix]);
    CHECK(handled == (markWritesAtFault > setupMarkWrites));
    // No write attempted for this key at all: refused, unchanged document, session not stopped
    if (writesAtFault == setupWrites) {
      CHECK([client.document isEqualToString:setupDocument]);
      CHECK(!session.stopped);
    }
    CHECK(!session.owned);
    CHECK(session.inputStatus ==
          (fault == Fault::Reenter ? KRInputInterrupted : KRInputClientException));
    client.hook = nil;
    NSUInteger writes = client.writes;
    // Lost authority is never resumed: a now-foreign mark, or a session that never verified a
    // write, gets no write. A re-armed session writes only a new mark at a whitespace boundary
    bool frozen = client.marked.location != NSNotFound || (session.stopped && setupWrites == 0);
    NSString *document = [client.document copy];
    NSUInteger caret = client.selection.location;
    kr_event next = textEvent(@"z", KR_EN);
    [session handleEvent:&next client:client];
    if (frozen)
      CHECK(client.writes == writes);
    if (client.writes != writes) {
      CHECK(client.writes == writes + 1 && caret > 0 && caret <= document.length);
      CHECK([document characterAtIndex:caret - 1] == ' ');
      NSString *expected = [document stringByReplacingCharactersInRange:NSMakeRange(caret, 0)
                                                             withString:@"z"];
      CHECK(documentIs(client, expected, caret + 1, NSMakeRange(caret, 1)));
    }
  }
}

void testKeyMapping() {
  currentTest = "key_mapping";
  const NSEventModifierFlags shift = NSEventModifierFlagShift, ctrl = NSEventModifierFlagControl,
                             opt = NSEventModifierFlagOption, cmd = NSEventModifierFlagCommand,
                             caps = NSEventModifierFlagCapsLock;
  CHECK(KREventForKey(kVK_ANSI_A, 0, @"a", KR_EN).role == KR_TEXT);
  kr_event upper = KREventForKey(kVK_ANSI_A, shift | caps, @"A", KR_RU);
  CHECK(upper.role == KR_TEXT && upper.len == 1 && upper.units[0] == u'A' && upper.mode == KR_RU);
  kr_event pair = KREventForKey(kVK_ANSI_A, 0, @"😀", KR_EN);
  CHECK(pair.role == KR_TEXT && pair.len == 2 && pair.units[0] == 0xD83D &&
        pair.units[1] == 0xDE00);
  CHECK(KREventForKey(kVK_Delete, 0, @"\x7f", KR_EN).role == KR_BACKSPACE);
  CHECK(KREventForKey(kVK_Delete, shift, @"\x7f", KR_EN).role == KR_FINISH_REQUEST);
  CHECK(KREventForKey(kVK_Escape, 0, @"\x1b", KR_EN).role == KR_ESCAPE);
  CHECK(KREventForKey(kVK_ANSI_K, ctrl | opt, @"\x0b", KR_EN).role == KR_MANUAL);
  CHECK(KREventForKey(kVK_ANSI_K, ctrl | opt | caps, @"л", KR_RU).role == KR_MANUAL);
  CHECK(KREventForKey(kVK_ANSI_Z, ctrl | opt, @"\x1a", KR_EN).role == KR_REVERSE);
  CHECK(KREventForKey(kVK_ANSI_K, ctrl | opt | shift, @"K", KR_EN).role == KR_FINISH_REQUEST);
  CHECK(KREventForKey(kVK_ANSI_K, ctrl, @"\x0b", KR_EN).role == KR_FINISH_REQUEST);
  CHECK(KREventForKey(kVK_ANSI_A, cmd, @"a", KR_EN).role == KR_FINISH_REQUEST);
  CHECK(KREventForKey(kVK_ANSI_A, opt, @"å", KR_EN).role == KR_FINISH_REQUEST);
  CHECK(KREventForKey(kVK_Return, 0, @"\r", KR_EN).role == KR_FINISH_REQUEST);
  CHECK(KREventForKey(kVK_Tab, 0, @"\t", KR_EN).role == KR_FINISH_REQUEST);
  CHECK(KREventForKey(kVK_LeftArrow, NSEventModifierFlagFunction, @"", KR_EN).role ==
        KR_FINISH_REQUEST);
  CHECK(KREventForKey(kVK_ANSI_A, 0, @"", KR_EN).role == KR_FINISH_REQUEST);
  CHECK(KREventForKey(kVK_ANSI_A, 0, @"abcdefghi", KR_EN).role == KR_FINISH_REQUEST);
  kr_event other = KREventForKey(kVK_Return, 0, @"\r", KR_EN);
  CHECK(other.len == 0);
}

void testInputStatusIsContentFreeAndDoesNotRetry() {
  currentTest = "input_status";
  KRSession *session = makeSession(true, KR_EN, [NSSet setWithObject:@"com.example.editor"]);
  FakeClient *client = [FakeClient new];
  client.bundle = @"com.example.editor";
  client.document = [NSMutableString stringWithString:@"prefix "];
  client.selection = NSMakeRange(client.document.length, 0);
  client.marked = NSMakeRange(NSNotFound, 0);
  client.noTextReadback = YES;
  kr_event input = textEvent(@"g", KR_EN);
  CHECK(![session handleEvent:&input client:client]);
  CHECK(session.inputStatus == KRInputTextUnavailable);
  CHECK(!session.stopped && client.writes == 0);
  CHECK([client.document isEqualToString:@"prefix "]);

  // A position-zero client can accept preedit but provide no verification of it
  client.document = [NSMutableString string];
  client.selection = NSMakeRange(0, 0);
  CHECK([session handleEvent:&input client:client]);
  CHECK(session.stopped && !session.owned && client.writes == 1);
  CHECK(session.inputStatus == KRInputTextUnavailable);
  NSUInteger calls = client.calls;
  CHECK(![session handleEvent:&input client:client]);
  CHECK(client.calls == calls && client.writes == 1);
  CHECK(session.inputStatus == KRInputTextUnavailable);

  [session activate];
  CHECK(session.inputStatus == KRInputNotObserved);
  session.secureInputEnabled = ^BOOL {
    return YES;
  };
  CHECK(![session handleEvent:&input client:client]);
  CHECK(session.inputStatus == KRInputSecure && client.calls == calls);
  session.secureInputEnabled = ^BOOL {
    return NO;
  };
  client.bundle = @"com.umputun.agterm";
  CHECK(![session handleEvent:&input client:client]);
  CHECK(session.inputStatus == KRInputExcluded && client.calls == calls + 1);
  CHECK(client.writes == 1);
}

void testLifecycleInterruptionStatus() {
  currentTest = "lifecycle_interruption_status";
  KRSession *session = makeSession(true);
  FakeClient *client = fixtureClient();
  __block NSUInteger callsAtInterruption = 0;
  __weak FakeClient *weakClient = client;
  client.hook = ^(NSString *call) {
    if ([call isEqualToString:@"setMarkedText+after"]) {
      FakeClient *strong = weakClient;
      callsAtInterruption = strong.calls;
      [session finishInClient:strong];
    }
  };
  kr_event input = textEvent(@"g", KR_EN);
  CHECK([session handleEvent:&input client:client]);
  CHECK(callsAtInterruption > 0 && client.calls == callsAtInterruption);
  CHECK(client.writes == 1 && session.stopped && !session.owned);
  CHECK(session.inputStatus == KRInputInterrupted);
  NSString *expected = [kPrefix stringByAppendingFormat:@"g%@", kSuffix];
  CHECK([client.document isEqualToString:expected]);
  client.hook = nil;
}

struct CompatibilityHarness {
  KRCompatibilitySession *session;
  FakeClient *client = fixtureClient();
  KRCompatibilityTarget target = {12345, 1, YES, 0};
  NSUInteger batches = 0;
  NSUInteger posts = 0;
  NSUInteger interruptAfter = NSUIntegerMax;
  BOOL secure = NO;
  BOOL interleave = NO;
  BOOL stripTags = NO;
  BOOL deferDelivery = NO;
  BOOL rawKeyFallback = NO;
  BOOL reletterEchoes = NO;
  uint64_t observedKeys = 0;
  NSUInteger maximumBatches = NSUIntegerMax;
  std::vector<CGEventRef> queued;

  explicit CompatibilityHarness(bool automatic = false) {
    kr_engine *engine = nullptr;
    CHECK(kr_engine_new(KR_ABI_VERSION, &engine) == KR_OK);
    CHECK(kr_set_automatic_policy(engine, KR_POLICY_FIXTURE) == KR_OK);
    client.bundle = @"com.example.editor";
    session = [[KRCompatibilitySession alloc] initWithEngine:engine
                                            bundleIdentifier:@"com.example.editor"
                                                   automatic:automatic];
    session.secureInputEnabled = ^BOOL {
      return secure;
    };
    session.observedKeyCount = ^uint64_t {
      return observedKeys;
    };
    session.targetSnapshot = ^KRCompatibilityTarget(NSString *bundle, id<IMKTextInput> supplied) {
      if (!target.available)
        return target;
      CHECK([[supplied bundleIdentifier] isEqualToString:bundle]);
      return target;
    };
    session.dispatchRewrite = ^KRCompatibilityDispatch(pid_t pid, const kr_compatibility_plan *plan,
                                                       BOOL (^current)(void)) {
      ++batches;
      if (batches > maximumBatches)
        return KRCompatibilityNotStarted;
      return [KRCompatibilitySession dispatchPlan:plan
                                        toProcess:pid
                                     stillCurrent:current
                                        postEvent:^(pid_t destination, CGEventRef event) {
                                          CHECK(destination == target.pid);
                                          CHECK([KRCompatibilitySession isSyntheticEvent:event]);
                                          CHECK(CGEventGetFlags(event) == 0 ||
                                                CGEventGetFlags(event) == kCGEventFlagMaskShift);
                                          CGEventRef copy = CGEventCreateCopy(event);
                                          CHECK(copy != nullptr);
                                          queued.push_back(copy);
                                          ++posts;
                                          if (posts == interruptAfter)
                                            ++target.generation;
                                        }];
    };
    client.hook = ^(NSString *call) {
      CHECK([call isEqualToString:@"bundleIdentifier"]);
    };
  }

  void insert(NSString *text) {
    [client.document insertString:text atIndex:client.selection.location];
    client.selection = NSMakeRange(client.selection.location + text.length, 0);
  }

  BOOL key(uint32_t role, NSString *text = @"") {
    ++observedKeys;
    constexpr CGKeyCode letterCodes[] = {0,  11, 8,  2,  14, 3, 5,  4,  34, 38, 40, 37, 46,
                                         45, 31, 35, 12, 15, 1, 17, 32, 9,  13, 7,  16, 6};
    CGKeyCode code = kVK_Space;
    switch (role) {
    case KR_MANUAL:
      code = kVK_ANSI_K;
      break;
    case KR_REVERSE:
      code = kVK_ANSI_Z;
      break;
    case KR_BACKSPACE:
      code = kVK_Delete;
      break;
    case KR_ESCAPE:
      code = kVK_Escape;
      break;
    }
    NSRange letter = [@"abcdefghijklmnopqrstuvwxyz" rangeOfString:text.lowercaseString];
    if (role == KR_TEXT && text.length == 1 && letter.location != NSNotFound)
      code = letterCodes[letter.location];
    CGEventRef physical = CGEventCreateKeyboardEvent(nullptr, code, true);
    CHECK(physical != nullptr);
    CGEventSetFlags(physical, role == KR_MANUAL || role == KR_REVERSE
                                  ? kCGEventFlagMaskControl | kCGEventFlagMaskAlternate
                              : text.length && ![text isEqualToString:text.lowercaseString]
                                  ? kCGEventFlagMaskShift
                                  : 0);
    if (text.length) {
      UniChar unit = [text characterAtIndex:0];
      CGEventKeyboardSetUnicodeString(physical, 1, &unit);
    }
    kr_event event =
        role == KR_TEXT ? textEvent(text, session.mode) : roleEvent(role, session.mode);
    BOOL handled = [session shouldPassPostedEvent:physical]
                       ? (!secure && (role == KR_MANUAL || role == KR_REVERSE))
                       : [session handleEvent:&event client:client];
    CFRelease(physical);
    if ((!handled || rawKeyFallback) && role == KR_TEXT)
      insert(text);
    // The client finishes handling the original key before its queued posts are delivered
    NSUInteger delivered = 0;
    for (CGEventRef posted : queued) {
      if (CGEventGetType(posted) == kCGEventKeyDown) {
        if (!deferDelivery) {
          // Re-create host echoes without carrying the tagged event source
          CGEventRef echo =
              stripTags
                  ? CGEventCreateKeyboardEvent(
                        nullptr, CGEventGetIntegerValueField(posted, kCGKeyboardEventKeycode), true)
                  : CGEventCreateCopy(posted);
          CHECK(echo != nullptr);
          CGEventSetFlags(echo, CGEventGetFlags(posted));
          UniChar echoUnit = 0;
          UniCharCount echoCount = 0;
          CGEventKeyboardGetUnicodeString(posted, 1, &echoCount, &echoUnit);
          if (echoCount == 1)
            CGEventKeyboardSetUnicodeString(echo, 1, &echoUnit);
          if (stripTags)
            CHECK(![KRCompatibilitySession isSyntheticEvent:echo]);
          if (reletterEchoes) {
            UniChar unit = 0;
            UniCharCount count = 0;
            CGEventKeyboardGetUnicodeString(echo, 1, &count, &unit);
            if (count == 1) {
              NSRange letter = [@"привет" rangeOfString:[NSString stringWithCharacters:&unit
                                                                                length:1]];
              if (letter.location != NSNotFound) {
                unit = [@"ghbdtn" characterAtIndex:letter.location];
                CGEventKeyboardSetUnicodeString(echo, 1, &unit);
              }
            }
          }
          BOOL passEcho = [session shouldPassPostedEvent:echo];
          if (!passEcho) {
            CHECK(reletterEchoes);
            UniChar unit = 0;
            UniCharCount count = 0;
            CGEventKeyboardGetUnicodeString(echo, 1, &count, &unit);
            NSString *text = count ? [NSString stringWithCharacters:&unit length:1] : @"";
            kr_event input =
                KREventForKey(CGEventGetIntegerValueField(echo, kCGKeyboardEventKeycode),
                              CGEventGetFlags(echo), text, session.mode);
            CHECK(![session handleEvent:&input client:client]);
          }
          CFRelease(echo);
        }
        int64_t code = CGEventGetIntegerValueField(posted, kCGKeyboardEventKeycode);
        if (code == kVK_Delete) {
          CHECK(client.selection.location > kPrefix.length);
          [client.document deleteCharactersInRange:NSMakeRange(client.selection.location - 1, 1)];
          client.selection = NSMakeRange(client.selection.location - 1, 0);
        } else {
          UniChar unit = 0;
          UniCharCount length = 0;
          CGEventKeyboardGetUnicodeString(posted, 1, &length, &unit);
          CHECK(length == 1);
          if (unit == 32)
            CHECK(code == kVK_Space);
          if (unit == u'п' || unit == u'П')
            CHECK(code == kVK_ANSI_G);
          if (unit == u'П')
            CHECK(CGEventGetFlags(posted) == kCGEventFlagMaskShift);
          insert([NSString stringWithCharacters:&unit length:1]);
        }
      }
      if (interleave && ++delivered == 1)
        insert(@"x");
      CFRelease(posted);
    }
    queued.clear();
    return handled;
  }

  void type(NSString *text) {
    for (NSUInteger i = 0; i < text.length; ++i)
      key(KR_TEXT, [text substringWithRange:NSMakeRange(i, 1)]);
  }

  BOOL document(NSString *middle) {
    NSString *expected = [[kPrefix stringByAppendingString:middle] stringByAppendingString:kSuffix];
    return [client.document isEqualToString:expected] &&
           NSEqualRanges(client.selection, NSMakeRange(kPrefix.length + middle.length, 0)) &&
           client.marked.location == NSNotFound && client.writes == 0;
  }
};

void testCompatibilityDispatchAndTracking() {
  currentTest = "compatibility_dispatch_and_tracking";
  CompatibilityHarness manual;
  manual.type(@"ghbdtn");
  CHECK(manual.batches == 0 && manual.document(@"ghbdtn"));
  CHECK(manual.key(KR_MANUAL));
  if (!manual.posts)
    std::fprintf(stderr, "compatibility dispatch: batches=%lu status=%s\n",
                 static_cast<unsigned long>(manual.batches), manual.session.statusTitle.UTF8String);
  CHECK(manual.batches == 1 && manual.posts == 24 && manual.document(@"привет"));
  CHECK(!manual.key(KR_TEXT, @" "));
  CHECK(manual.key(KR_REVERSE));
  CHECK(manual.document(@"ghbdtn "));
  manual.type(@"hello ");
  NSUInteger batches = manual.batches;
  CHECK(manual.key(KR_REVERSE));
  CHECK(manual.batches == batches && manual.document(@"ghbdtn hello "));

  CompatibilityHarness automatic(true);
  automatic.type(@" ghbdtn ");
  CHECK(automatic.batches == 1 && automatic.document(@" привет "));
  CHECK(automatic.session.automaticSwitchMode == KR_RU);
  CHECK([automatic.session.statusTitle isEqualToString:@"Compatibility: ready"]);
  CompatibilityHarness disabled;
  disabled.type(@"ghbdtn ");
  CHECK(disabled.batches == 0 && disabled.document(@"ghbdtn "));

  CompatibilityHarness untagged;
  untagged.stripTags = YES;
  untagged.type(@"Ghbdtn");
  CHECK(untagged.key(KR_MANUAL));
  CHECK(untagged.document(@"Привет"));
  CHECK(!untagged.key(KR_TEXT, @" "));
  CHECK(untagged.key(KR_REVERSE));
  CHECK(untagged.document(@"Ghbdtn "));
}

void testCompatibilityRefusalInterruptionAndSecureInput() {
  currentTest = "compatibility_refusal_interruption_secure";
  CompatibilityHarness secure;
  secure.secure = YES;
  CHECK(!secure.key(KR_TEXT, @"g"));
  CHECK(secure.client.calls == 0 && secure.batches == 0 && secure.document(@"g"));

  CompatibilityHarness unavailable;
  unavailable.target.available = NO;
  unavailable.type(@"ghbdtn");
  CHECK(unavailable.key(KR_MANUAL));
  CHECK(unavailable.client.calls == 0 && unavailable.batches == 0);

  CompatibilityHarness moved;
  moved.type(@"ghbdtn");
  ++moved.target.generation;
  CHECK(moved.key(KR_MANUAL));
  CHECK(moved.batches == 0 && moved.document(@"ghbdtn"));

  CompatibilityHarness interrupted;
  interrupted.type(@"ghbdtn");
  interrupted.interruptAfter = 1;
  CHECK(interrupted.key(KR_MANUAL));
  CHECK(interrupted.session.stopped && interrupted.posts == 1);
  CHECK(interrupted.document(@"ghbdt"));
  NSUInteger calls = interrupted.client.calls;
  CHECK(!interrupted.key(KR_TEXT, @"q"));
  CHECK(interrupted.client.calls == calls && interrupted.posts == 1);
  CHECK(interrupted.document(@"ghbdtq"));
}

void testCompatibilityKnownInterleavingHazard() {
  currentTest = "compatibility_known_interleaving_hazard";
  CompatibilityHarness h;
  h.type(@"ghbdtn");
  h.interleave = YES;
  CHECK(h.key(KR_MANUAL));
  // The counterexample is retained, not classified as safe input preservation
  CHECK(h.document(@"gпривет"));
  CHECK(!h.session.stopped);
  CHECK([h.session.statusTitle isEqualToString:@"Compatibility: ready"]);
}

void testCompatibilityContinuesWithoutPostedEchoes() {
  currentTest = "compatibility_continues_without_posted_echoes";
  CompatibilityHarness h(true);
  h.deferDelivery = YES;
  h.type(@" ghbdtn ");
  CHECK(h.batches == 1 && h.document(@" привет "));
  for (NSUInteger i = 0; i < 3; ++i)
    h.type(@"ghbdtn ");
  CHECK(!h.session.stopped && h.batches == 4);
  CHECK(h.document(@" привет привет привет привет "));

  CompatibilityHarness terminal(true);
  terminal.deferDelivery = YES;
  terminal.rawKeyFallback = YES;
  terminal.type(@" ghbdtn");
  // A raw-key host forwards Space even if IMK claims it consumed the event
  CHECK(!terminal.key(KR_TEXT, @" "));
  CHECK(terminal.document(@" привет "));

  CompatibilityHarness altered(true);
  altered.stripTags = YES;
  altered.reletterEchoes = YES;
  altered.maximumBatches = 1;
  altered.type(@" ghbdtn ");
  CHECK(altered.batches == 1 && !altered.session.stopped);
  CHECK([altered.session.statusTitle containsString:@"new observed typing"]);
}

void testCompatibilityBoundaryAcrossAutomaticModeEcho() {
  currentTest = "compatibility_boundary_after_automatic_mode_echo";
  {
    CompatibilityHarness h(true);
    h.deferDelivery = YES;
    h.type(@" ghbdtn ");
    CHECK(h.session.automaticSwitchMode == KR_RU);
    NSUInteger calls = h.client.calls;
    [h.session keepAcrossAutomaticSwitchToMode:KR_RU];
    CHECK(h.session.mode == KR_RU && h.client.calls == calls && h.batches == 1);
    // The kept word reverses in its own mode, and the switch is undone with it
    CHECK(h.key(KR_REVERSE));
    CHECK(h.batches == 2 && h.document(@" ghbdtn "));
    CHECK(h.session.automaticSwitchMode == KR_EN);
    [h.session keepAcrossAutomaticSwitchToMode:KR_EN];
    h.type(@"hello ");
    CHECK(h.batches == 2 && h.document(@" ghbdtn hello ") && !h.session.stopped);
  }
  {
    // Typing in the new mode drops the kept word but keeps the observed Space as a boundary
    CompatibilityHarness h(true);
    h.deferDelivery = YES;
    h.type(@" ghbdtn ");
    [h.session keepAcrossAutomaticSwitchToMode:KR_RU];
    h.type(@"р");
    ++h.target.generation;
    ++h.target.sourceGeneration;
    h.type(@"уддщ ");
    CHECK(h.batches == 2 && h.document(@" привет hello "));
    CHECK(h.session.automaticSwitchMode == KR_EN);
    NSUInteger batches = h.batches;
    CHECK(h.key(KR_REVERSE));
    CHECK(h.batches == batches + 1 && h.document(@" привет руддщ "));
    // Reversal in the word's own mode doesn't ask for another switch
    CHECK(h.session.automaticSwitchMode == 0);
  }

  CompatibilityHarness moved(true);
  moved.deferDelivery = YES;
  moved.type(@" ghbdtn ");
  [moved.session keepAcrossAutomaticSwitchToMode:KR_RU];
  ++moved.target.generation;
  // A guard event other than the source change expires the word and its reversal
  CHECK(moved.key(KR_REVERSE));
  CHECK(moved.batches == 1 && moved.document(@" привет "));
  moved.type(@"руддщ ");
  CHECK(moved.batches == 1 && moved.document(@" привет руддщ "));
  moved.type(@"руддщ ");
  CHECK(moved.batches == 2 && moved.document(@" привет руддщ hello "));

  CompatibilityHarness stopped(true);
  stopped.interruptAfter = 1;
  stopped.type(@" ghbdtn ");
  CHECK(stopped.session.stopped);
  [stopped.session keepAcrossAutomaticSwitchToMode:KR_RU];
  CHECK(stopped.session.stopped);
}

// A failed readback check after a write commits the client's own readback of the mark once, so
// a host that replaces marked text with the next passed key can't lose the word
void testReadbackCommitAfterFailedCheck() {
  currentTest = "readback_commit_after_failed_check";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn"));
    client.misplaceCaretOnMark = YES;
    NSUInteger writes = client.writes;
    kr_event space = textEvent(@" ", KR_EN);
    // The Space was carried by the unknown write and may be in the document: consumed
    CHECK([session handleEvent:&space client:client]);
    CHECK(session.stopped && !session.owned && session.inputStatus == KRInputCaretMismatch);
    CHECK(client.writes == writes + 2 && client.markWrites == writes + 1);
    CHECK([client.document isEqualToString:withAffixes(@"привет ")]);
    CHECK(client.marked.location == NSNotFound);
    // The next word re-arms once from fresh checks. This client misplaces the caret again, so
    // the second unknown outcome stops the session until activation
    NSUInteger calls = client.calls;
    kr_event letter = textEvent(@"x", KR_EN);
    CHECK([session handleEvent:&letter client:client]);
    CHECK(client.calls > calls && session.stopped && !session.owned);
    CHECK([client.document isEqualToString:withAffixes(@"привет x")]);
    CHECK(client.marked.location == NSNotFound);
    calls = client.calls;
    CHECK(![session handleEvent:&space client:client]);
    CHECK(![session handleEvent:&letter client:client]);
    CHECK(client.calls == calls);
    [session activate];
    client.misplaceCaretOnMark = NO;
    [client pressKey:@" "];
    CHECK(type(session, client, @"ghbdtn "));
    CHECK(!session.stopped && session.owned);
    CHECK([client.document isEqualToString:withAffixes(@"привет x привет ")]);
  }
  {
    // A finish whose readback differs commits what the client shows, then the key passes once
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    client.keepMarkOnInsert = YES;
    client.alterInsert = YES;
    NSUInteger writes = client.writes;
    kr_event letter = textEvent(@"h", KR_EN);
    CHECK(![session handleEvent:&letter client:client]);
    CHECK(session.stopped && client.writes == writes + 2);
    CHECK([client.document isEqualToString:withAffixes(@"привет#")]);
  }
  {
    // Without an exact readback of the mark nothing is written, and the host loses the word
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn"));
    __weak FakeClient *weakClient = client;
    client.hook = ^(NSString *call) {
      if ([call isEqualToString:@"setMarkedText+after"])
        weakClient.noTextReadback = YES;
    };
    NSUInteger writes = client.writes;
    kr_event space = textEvent(@" ", KR_EN);
    CHECK([session handleEvent:&space client:client]);
    CHECK(session.stopped && client.writes == writes + 1);
    CHECK(session.inputStatus == KRInputTextUnavailable);
    client.hook = nil;
    [client pressKey:@"x"];
    CHECK([client.document isEqualToString:withAffixes(@"x")]);
  }
  {
    // An ignored write leaves no mark to commit
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    client.ignoreWrites = YES;
    kr_event letter = textEvent(@"g", KR_EN);
    CHECK([session handleEvent:&letter client:client]);
    CHECK(session.stopped && client.writes == 1);
  }
}

void testReadbackCommitKeepsWordInTextView() {
  currentTest = "readback_commit_text_view";
  NSString *initial = [kPrefix stringByAppendingString:kSuffix];
  {
    KRSession *session = makeSession(true, KR_EN, [NSSet setWithObject:@"com.apple.TextEdit"]);
    TextViewClient *client = [[TextViewClient alloc] initWithText:initial caret:kPrefix.length];
    CHECK(type(session, client, @"ghbdtn "));
    CHECK(!session.stopped && client.view.hasMarkedText);
    CHECK(NSEqualRanges(client.view.markedRange, NSMakeRange(kPrefix.length, 7)));
    CHECK([client.view.string isEqualToString:withAffixes(@"привет ")]);
    CHECK(key(session, client, KR_REVERSE));
    CHECK([client.view.string isEqualToString:withAffixes(@"ghbdtn ")]);
    CHECK([session finishInClient:client]);
    CHECK(!client.view.hasMarkedText);
    CHECK([client.view.string isEqualToString:withAffixes(@"ghbdtn ")]);
  }
  {
    KRSession *session = makeSession(true, KR_EN, [NSSet setWithObject:@"com.apple.TextEdit"]);
    TextViewClient *client = [[TextViewClient alloc] initWithText:initial caret:kPrefix.length];
    CHECK(type(session, client, @"ghbdtn"));
    client.misreportCaretOnce = YES;
    CHECK(type(session, client, @" "));
    CHECK(session.stopped && !client.view.hasMarkedText);
    CHECK([client.view.string isEqualToString:withAffixes(@"привет ")]);
    // The next word re-arms the session from the text view's own boundary
    kr_event letter = textEvent(@"x", KR_EN);
    CHECK([session handleEvent:&letter client:client]);
    CHECK(!session.stopped && session.owned && client.view.hasMarkedText);
    CHECK(NSEqualRanges(client.view.markedRange, NSMakeRange(after(@"привет "), 1)));
    CHECK([client.view.string isEqualToString:withAffixes(@"привет x")]);
  }
  {
    // The measured loss this prevents: with no readback the mark stays and the key replaces it
    KRSession *session = makeSession(true, KR_EN, [NSSet setWithObject:@"com.apple.TextEdit"]);
    TextViewClient *client = [[TextViewClient alloc] initWithText:initial caret:kPrefix.length];
    CHECK(type(session, client, @"ghbdtn"));
    client.dropReadbackAfterMark = YES;
    NSUInteger writes = client.writes;
    CHECK(type(session, client, @" "));
    CHECK(session.stopped && client.writes == writes + 1 && client.view.hasMarkedText);
    [client.view insertText:@"x" replacementRange:NSMakeRange(NSNotFound, 0)];
    CHECK([client.view.string isEqualToString:withAffixes(@"x")]);
  }
}

// Secure Input that starts during a client call stops inspection and writes at the next step
void testSecureInputDuringClientCalls() {
  currentTest = "secure_input_during_client_calls";
  for (NSString *trigger in @[ @"bundleIdentifier", @"markedRange" ]) {
    KRSession *session = makeSession(true);
    __block BOOL secure = NO;
    session.secureInputEnabled = ^BOOL {
      return secure;
    };
    FakeClient *client = fixtureClient();
    __block NSUInteger reads = 0;
    client.hook = ^(NSString *call) {
      if ([call isEqualToString:trigger])
        secure = YES;
      if ([call isEqualToString:@"stringFromRange"])
        ++reads;
    };
    kr_event letter = textEvent(@"g", KR_EN);
    CHECK(![session handleEvent:&letter client:client]);
    CHECK(reads == 0 && client.writes == 0 && session.inputStatus == KRInputSecure);
    CHECK(!session.stopped && !session.owned);
  }
  {
    // Before a write: the pre-write verification read is the last query
    KRSession *session = makeSession(true);
    __block BOOL secure = NO;
    session.secureInputEnabled = ^BOOL {
      return secure;
    };
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"g"));
    NSUInteger writes = client.writes;
    client.hook = ^(NSString *call) {
      if ([call isEqualToString:@"stringFromRange"])
        secure = YES;
    };
    kr_event letter = textEvent(@"h", KR_EN);
    CHECK(![session handleEvent:&letter client:client]);
    CHECK(client.writes == writes && session.inputStatus == KRInputSecure);
    CHECK(documentIs(client, withAffixes(@"g"), after(@"g"), NSMakeRange(kPrefix.length, 1)));
  }
  {
    // After a write: no readback and no readback commit
    KRSession *session = makeSession(true);
    __block BOOL secure = NO;
    session.secureInputEnabled = ^BOOL {
      return secure;
    };
    FakeClient *client = fixtureClient();
    __block NSUInteger reads = 0;
    client.hook = ^(NSString *call) {
      if ([call isEqualToString:@"setMarkedText+after"])
        secure = YES;
      if (secure && [call isEqualToString:@"stringFromRange"])
        ++reads;
    };
    kr_event letter = textEvent(@"g", KR_EN);
    CHECK([session handleEvent:&letter client:client]);
    CHECK(session.stopped && client.writes == 1 && reads == 0);
    CHECK(session.inputStatus == KRInputSecure);
  }
}

// A mark kept across a mode change is planned in its own mode until it is finished
void testKeptMarkAcrossModeChange() {
  currentTest = "kept_mark_across_mode_change";
  {
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    CHECK(session.automaticSwitchMode == KR_RU && session.boundaryRetained);
    session.mode = KR_RU;
    CHECK(key(session, client, KR_REVERSE));
    CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "),
                     NSMakeRange(kPrefix.length, 7)));
    CHECK(session.automaticSwitchMode == KR_EN);
    session.mode = KR_EN;
    CHECK(type(session, client, @"hello "));
    CHECK(session.automaticSwitchMode == 0);
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"ghbdtn hello "), after(@"ghbdtn hello "), kNoMark));
  }
  {
    // A letter in the new mode finishes the kept mark and starts its own word in that mode
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    session.mode = KR_RU;
    CHECK(type(session, client, @"руддщ "));
    CHECK(session.automaticSwitchMode == KR_EN);
    CHECK(key(session, client, KR_REVERSE));
    CHECK(session.automaticSwitchMode == 0);
    [session finishInClient:client];
    CHECK(documentIs(client, withAffixes(@"привет руддщ "), after(@"привет руддщ "), kNoMark));
    CHECK(!session.stopped);
  }
  {
    // Escape restores in the kept mode and asks to switch back; Space just finishes
    KRSession *session = makeSession(true);
    FakeClient *client = fixtureClient();
    CHECK(type(session, client, @"ghbdtn "));
    session.mode = KR_RU;
    CHECK(key(session, client, KR_ESCAPE));
    CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "), kNoMark));
    CHECK(session.automaticSwitchMode == KR_EN);
    KRSession *other = makeSession(true);
    FakeClient *spaced = fixtureClient();
    CHECK(type(other, spaced, @"ghbdtn "));
    other.mode = KR_RU;
    kr_event space = textEvent(@" ", KR_RU);
    CHECK(![other handleEvent:&space client:spaced]);
    CHECK(other.automaticSwitchMode == 0 && !other.owned);
    CHECK(documentIs(spaced, withAffixes(@"привет "), after(@"привет "), kNoMark));
  }
}

// Runs the controller's deferred main-queue work. Zero-timeout passes service only what is
// already queued, so no wall-clock wait is involved
void runMainQueue() {
  for (int i = 0; i < 4; ++i)
    CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0, false);
}

NSEvent *keyDown(NSString *characters, unsigned short code, NSEventModifierFlags flags = 0) {
  return [NSEvent keyEventWithType:NSEventTypeKeyDown
                          location:NSZeroPoint
                     modifierFlags:flags
                         timestamp:0
                      windowNumber:0
                           context:nil
                        characters:characters
       charactersIgnoringModifiers:characters
                         isARepeat:NO
                           keyCode:code];
}

NSEvent *command(unsigned short code) {
  return keyDown(code == kVK_ANSI_K ? @"\x0b" : @"\x1a", code,
                 NSEventModifierFlagControl | NSEventModifierFlagOption);
}

// A controller with test seams and a runtime whose work runs only when drained
struct ControllerHarness {
  dispatch_queue_t queue = dispatch_queue_create("org.keyrill.test.controller", nullptr);
  NSMutableArray<dispatch_block_t> *pending = [NSMutableArray array];
  NSMutableDictionary *preferences;
  KRRuntime *runtime;
  KeyrillAlphaController *controller;
  BOOL secure = NO;
  NSUInteger guardStarts = 0;
  NSMutableArray<NSDictionary *> *updates = [NSMutableArray array];

  explicit ControllerHarness(NSDictionary *initial) {
    preferences = [initial mutableCopy];
    install();
    controller = [[KeyrillAlphaController alloc] initWithServer:nil delegate:nil client:nil];
    CHECK(controller != nil);
    controller.secureInputEnabled = ^BOOL {
      return secure;
    };
    controller.startGuard = ^{
      ++guardStarts;
    };
    KeyrillAlphaController.preferencesUpdater = ^BOOL(NSDictionary *changes) {
      [updates addObject:changes];
      for (NSString *key in changes) {
        if (changes[key] == NSNull.null)
          [preferences removeObjectForKey:key];
        else
          preferences[key] = changes[key];
      }
      return install();
    };
  }
  ControllerHarness(const ControllerHarness &) = delete;
  ControllerHarness &operator=(const ControllerHarness &) = delete;
  ~ControllerHarness() {
    KeyrillAlphaController.preferencesUpdater = nil;
    [runtime stop];
    drain();
  }

  BOOL install() {
    NSMutableArray<dispatch_block_t> *queued = pending;
    runtime = [[KRRuntime alloc] initWithPreferences:preferences
        homeResolver:^NSString * {
          return nil;
        }
        workQueue:queue
        mainExecutor:^(dispatch_block_t block) {
          @synchronized(queued) {
            [queued addObject:block];
          }
        }
        observationInterval:0];
    if (![KeyrillAlphaController setRuntime:runtime])
      return NO;
    [runtime start];
    return YES;
  }

  void drain() {
    for (;;) {
      dispatch_sync(queue, ^{
                    });
      NSArray<dispatch_block_t> *blocks;
      @synchronized(pending) {
        blocks = [pending copy];
        [pending removeAllObjects];
      }
      if (!blocks.count)
        return;
      for (dispatch_block_t block in blocks)
        block();
    }
  }

  // Two observation rounds publish settled sources; adoption follows on drain
  void settle() {
    [runtime tick];
    [runtime tick];
    drain();
  }

  BOOL type(FakeClient *client, NSString *text) {
    BOOL all = YES;
    for (NSUInteger i = 0; i < text.length; ++i) {
      NSString *unit = [text substringWithRange:NSMakeRange(i, 1)];
      all = [controller handleEvent:keyDown(unit, [unit isEqualToString:@" "] ? kVK_Space : 0)
                             client:client] &&
            all;
    }
    return all;
  }

  void echo(uint32_t mode, FakeClient *client) {
    [controller setValue:[KeyrillAlphaController inputModeForMode:mode]
                  forTag:kTextServiceInputModePropertyTag
                  client:client];
  }

  NSArray<NSString *> *menuTitles() {
    NSMutableArray<NSString *> *titles = [NSMutableArray array];
    for (NSMenuItem *item in controller.menu.itemArray)
      [titles addObject:item.title];
    return titles;
  }

  NSString *checkedTierChoice() {
    for (NSMenuItem *item in controller.menu.itemArray) {
      for (NSMenuItem *choice in item.submenu.itemArray) {
        if (choice.state == NSControlStateValueOn)
          return choice.title;
      }
    }
    return nil;
  }
};

NSString *temporaryDirectory(NSString *name) {
  NSString *root = [NSTemporaryDirectory()
      stringByAppendingPathComponent:[NSString stringWithFormat:@"keyrill-%@-%d", name,
                                                                NSProcessInfo.processInfo
                                                                    .processIdentifier]];
  CHECK(mkdir(root.fileSystemRepresentation, 0700) == 0);
  return root;
}

// Automatic correction, the requested switch and its echo, and reversal through the controller
void testControllerCorrectionSwitchAndReversal() {
  currentTest = "controller_correction_switch_reversal";
  NSString *root = temporaryDirectory(@"controller");
  NSString *en = [root stringByAppendingPathComponent:@"en"];
  NSString *ru = [root stringByAppendingPathComponent:@"ru"];
  CHECK([@"" writeToFile:en atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  CHECK([@"" writeToFile:ru atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  NSDictionary *preferences = @{
    @"AutomaticCorrectionEnabled" : @YES,
    @"EnglishDictionaryPath" : en,
    @"RussianDictionaryPath" : ru
  };
  NSString *russian = [KeyrillAlphaController inputModeForMode:KR_RU];
  NSString *english = [KeyrillAlphaController inputModeForMode:KR_EN];
  CHECK([english isEqualToString:@KR_BUNDLE_IDENTIFIER ".English"]);
  {
    ControllerHarness h(preferences);
    FakeClient *client = fixtureClient();
    client.bundle = @"com.apple.TextEdit";
    [h.controller activateServer:client];
    h.settle();
    CHECK(h.type(client, @"ghbdtn "));
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "),
                     NSMakeRange(kPrefix.length, 7)));
    runMainQueue();
    CHECK([client.selectedModes isEqualToArray:@[ russian ]]);
    // The echo of Keyrill's own request keeps the mark and its reversal
    NSUInteger writes = client.writes;
    h.echo(KR_RU, client);
    CHECK(client.writes == writes);
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "),
                     NSMakeRange(kPrefix.length, 7)));
    CHECK([h.controller handleEvent:command(kVK_ANSI_Z) client:client]);
    CHECK(documentIs(client, withAffixes(@"ghbdtn "), after(@"ghbdtn "),
                     NSMakeRange(kPrefix.length, 7)));
    runMainQueue();
    CHECK([client.selectedModes isEqualToArray:(@[ russian, english ])]);
    h.echo(KR_EN, client);
    CHECK(h.type(client, @"hello "));
    [h.controller commitComposition:client];
    CHECK(documentIs(client, withAffixes(@"ghbdtn hello "), after(@"ghbdtn hello "), kNoMark));
    CHECK(h.guardStarts == 0);
  }
  {
    // A mode change Keyrill didn't request finishes the mark; the queued request is dropped
    ControllerHarness h(preferences);
    FakeClient *client = fixtureClient();
    client.bundle = @"com.apple.TextEdit";
    [h.controller activateServer:client];
    h.settle();
    CHECK(h.type(client, @"ghbdtn "));
    h.echo(KR_RU, client);
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), kNoMark));
    runMainQueue();
    CHECK(client.selectedModes.count == 0);
    NSUInteger writes = client.writes;
    CHECK([h.controller handleEvent:command(kVK_ANSI_Z) client:client]);
    CHECK(client.writes == writes);
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), kNoMark));
  }
  {
    // A client that commits its mark during the switch: the readback check refuses the reversal
    ControllerHarness h(preferences);
    FakeClient *client = fixtureClient();
    client.bundle = @"com.apple.TextEdit";
    [h.controller activateServer:client];
    h.settle();
    CHECK(h.type(client, @"ghbdtn "));
    __weak FakeClient *weakClient = client;
    client.hook = ^(NSString *call) {
      if ([call isEqualToString:@"selectInputMode"])
        weakClient.marked = NSMakeRange(NSNotFound, 0);
    };
    runMainQueue();
    h.echo(KR_RU, client);
    client.hook = nil;
    NSUInteger writes = client.writes;
    [h.controller handleEvent:command(kVK_ANSI_Z) client:client];
    CHECK(client.writes == writes);
    CHECK(documentIs(client, withAffixes(@"привет "), after(@"привет "), kNoMark));
  }
  CHECK([NSFileManager.defaultManager removeItemAtPath:root error:nil]);
}

void testControllerTierRouting() {
  currentTest = "controller_tier_routing";
  struct Case {
    NSString *bundle;
    NSDictionary *preferences;
    KRTier tier;
    NSString *line;
  };
  NSDictionary *compatibilityOff = @{@"CompatibilityEnabled" : @NO};
  const Case cases[] = {
      {@"com.apple.TextEdit", @{}, KRTierComposition, @"This app: direct editing (built-in)"},
      {@"com.apple.Terminal", @{}, KRTierManual,
       @"This app: compatibility, manual only (built-in)"},
      {@"com.umputun.agterm", @{}, KRTierManual,
       @"This app: compatibility, manual only (built-in)"},
      {@"com.example.editor", @{}, KRTierManual,
       @"This app: compatibility, manual only (unlisted app)"},
      {@"com.example.editor", @{@"UnlistedAutomaticCorrectionEnabled" : @YES}, KRTierAutomatic,
       @"This app: compatibility, automatic and manual (unlisted app)"},
      {@"com.1password.1password", @{}, KRTierOff, @"This app: off (built-in)"},
      {@"com.example.editor", compatibilityOff, KRTierOff,
       @"This app: off (compatibility mode is off)"},
      {@"com.umputun.agterm", compatibilityOff, KRTierOff,
       @"This app: off (compatibility mode is off)"},
      {@"com.example.editor", @{@"ApplicationTiers" : @{@"com.example.editor" : @"composition"}},
       KRTierComposition, @"This app: direct editing (your setting)"},
      {@"com.apple.Safari", @{@"ApplicationTiers" : @{@"com.apple.Safari" : @"off"}}, KRTierOff,
       @"This app: off (your setting)"},
      {@KR_BUNDLE_IDENTIFIER, @{@"ApplicationTiers" : @{@KR_BUNDLE_IDENTIFIER : @"composition"}},
       KRTierOff, nil},
  };
  for (const Case &c : cases) {
    ControllerHarness h(c.preferences);
    FakeClient *client = fixtureClient();
    client.bundle = c.bundle;
    [h.controller activateServer:client];
    runMainQueue();
    CHECK(client.calls == 1);
    NSArray<NSString *> *titles = h.menuTitles();
    if (c.line)
      CHECK([titles containsObject:c.line]);
    else
      CHECK(![titles containsObject:@"Correction in This App"]);
    BOOL compatible = c.tier == KRTierManual || c.tier == KRTierAutomatic;
    CHECK(h.guardStarts == (compatible ? 1u : 0u));
    BOOL letter = [h.controller handleEvent:keyDown(@"g", kVK_ANSI_G) client:client];
    BOOL manual = [h.controller handleEvent:command(kVK_ANSI_K) client:client];
    switch (c.tier) {
    case KRTierComposition:
      CHECK(letter && manual);
      CHECK(documentIs(client, withAffixes(@"п"), after(@"п"), NSMakeRange(kPrefix.length, 1)));
      break;
    case KRTierManual:
    case KRTierAutomatic:
      // Without the guard tap compatibility refuses: letters pass, commands stay reserved
      CHECK(!letter && manual && client.writes == 0);
      break;
    case KRTierOff:
      CHECK(!letter && !manual && client.writes == 0 && client.calls == 1);
      break;
    }
  }
  {
    // Secure Input at activation: no client query until a key arrives with it off
    ControllerHarness h(@{});
    h.secure = YES;
    FakeClient *client = fixtureClient();
    client.bundle = @"com.apple.TextEdit";
    [h.controller activateServer:client];
    CHECK(![h.controller handleEvent:keyDown(@"g", kVK_ANSI_G) client:client]);
    CHECK(![h.controller handleEvent:command(kVK_ANSI_K) client:client]);
    CHECK(client.calls == 0 && ![h.menuTitles() containsObject:@"Correction in This App"]);
    h.secure = NO;
    CHECK([h.controller handleEvent:keyDown(@"g", kVK_ANSI_G) client:client]);
    CHECK(documentIs(client, withAffixes(@"g"), after(@"g"), NSMakeRange(kPrefix.length, 1)));
    // Secure Input mid-word passes before any character is read or the client is queried
    h.secure = YES;
    NSUInteger calls = client.calls;
    CHECK(![h.controller handleEvent:keyDown(@"h", kVK_ANSI_H) client:client]);
    CHECK(![h.controller handleEvent:command(kVK_ANSI_Z) client:client]);
    CHECK(client.calls == calls);
  }
  {
    ControllerHarness h(@{});
    h.secure = YES;
    FakeClient *client = fixtureClient();
    client.bundle = @"com.example.editor";
    [h.controller activateServer:client];
    h.secure = NO;
    CHECK([h.controller handleEvent:command(kVK_ANSI_K) client:client]);
    CHECK(client.calls == 1 && client.writes == 0);
    h.secure = YES;
    CHECK(![h.controller handleEvent:command(kVK_ANSI_K) client:client]);
    CHECK(client.calls == 1);
  }
}

// The input menu changes the current app's tier, stores it and reroutes the app
void testControllerTierOverride() {
  currentTest = "controller_tier_override";
  ControllerHarness h(@{@"UnrelatedKey" : @"kept"});
  FakeClient *client = fixtureClient();
  client.bundle = @"com.example.editor";
  [h.controller activateServer:client];
  CHECK([h.checkedTierChoice() isEqualToString:@"Default (compatibility, manual only)"]);
  CHECK(![h.controller handleEvent:keyDown(@"g", kVK_ANSI_G) client:client]);
  [h.controller performSelector:@selector(useCompositionTier:) withObject:nil];
  CHECK(h.updates.count == 0);
  runMainQueue();
  CHECK([h.updates isEqualToArray:@[
    @{@"ApplicationTiers" : @{@"com.example.editor" : @"composition"}}
  ]]);
  CHECK([h.preferences[@"UnrelatedKey"] isEqualToString:@"kept"]);
  CHECK([h.checkedTierChoice() isEqualToString:@"Direct Editing"]);
  CHECK([h.menuTitles() containsObject:@"This app: direct editing (your setting)"]);
  [client pressKey:@"g"];
  [client pressKey:@" "];
  CHECK([h.controller handleEvent:keyDown(@"g", kVK_ANSI_G) client:client]);
  CHECK(documentIs(client, withAffixes(@"g g"), after(@"g g"), NSMakeRange(after(@"g "), 1)));

  // Leaving composition finishes the owned mark through the verified path
  [h.controller performSelector:@selector(useOffTier:) withObject:nil];
  runMainQueue();
  CHECK(documentIs(client, withAffixes(@"g g"), after(@"g g"), kNoMark));
  NSUInteger calls = client.calls;
  CHECK(![h.controller handleEvent:command(kVK_ANSI_K) client:client]);
  CHECK(client.calls == calls);

  [h.controller performSelector:@selector(useDefaultTier:) withObject:nil];
  runMainQueue();
  CHECK(h.updates.count == 3 && h.updates[2][@"ApplicationTiers"] == NSNull.null);
  CHECK(!h.preferences[@"ApplicationTiers"]);
  CHECK([h.controller handleEvent:command(kVK_ANSI_K) client:client]);

  KeyrillAlphaController.preferencesUpdater = nil;
  [h.controller performSelector:@selector(useOffTier:) withObject:nil];
  runMainQueue();
  CHECK([h.menuTitles() containsObject:@"This app's setting couldn't be saved"]);
  CHECK([h.controller handleEvent:command(kVK_ANSI_K) client:client]);
}

// A reentrant lifecycle change during deferred routing leaves the newer activation in charge, and
// the stale key writes nothing into the document that lost focus
void testControllerReentrantDeferredRouting() {
  currentTest = "controller_reentrant_deferred_routing";
  // Terminal routes the newer activation to compatibility; Safari to composition, where a stale
  // key would otherwise start a fresh mark in the inactive document
  for (NSString *newer in @[ @"com.apple.Terminal", @"com.apple.Safari" ]) {
    ControllerHarness h(@{});
    ControllerHarness *harness = &h;
    h.secure = YES;
    FakeClient *first = fixtureClient();
    first.bundle = @"com.apple.TextEdit";
    FakeClient *second = fixtureClient();
    second.bundle = newer;
    [h.controller activateServer:first];
    CHECK(first.calls == 0);
    h.secure = NO;
    __block BOOL fired = NO;
    __weak FakeClient *weakFirst = first;
    first.hook = ^(NSString *call) {
      if (fired || ![call isEqualToString:@"bundleIdentifier"])
        return;
      fired = YES;
      [harness->controller deactivateServer:weakFirst];
      [harness->controller activateServer:second];
    };
    CHECK(![h.controller handleEvent:keyDown(@"g", kVK_ANSI_G) client:first]);
    CHECK(fired && first.writes == 0);
    CHECK(documentIs(first, withAffixes(@""), after(@""), kNoMark));
    first.hook = nil;
    if ([newer isEqualToString:@"com.apple.Safari"]) {
      CHECK([h.menuTitles() containsObject:@"This app: direct editing (built-in)"]);
      CHECK([h.controller handleEvent:keyDown(@"g", kVK_ANSI_G) client:second]);
      CHECK(documentIs(second, withAffixes(@"g"), after(@"g"), NSMakeRange(kPrefix.length, 1)));
    } else {
      CHECK([h.menuTitles() containsObject:@"This app: compatibility, manual only (built-in)"]);
      CHECK(![h.controller handleEvent:keyDown(@"g", kVK_ANSI_G) client:second]);
      CHECK([h.controller handleEvent:command(kVK_ANSI_K) client:second]);
      CHECK(second.writes == 0);
    }
    CHECK(first.writes == 0);
    CHECK(documentIs(first, withAffixes(@""), after(@""), kNoMark));
  }
}

// A key under Secure Input is newer input: it cancels a queued automatic mode switch
void testControllerSecureInputCancelsQueuedSwitch() {
  currentTest = "controller_secure_input_cancels_queued_switch";
  NSString *root = temporaryDirectory(@"controller-secure");
  NSString *en = [root stringByAppendingPathComponent:@"en"];
  NSString *ru = [root stringByAppendingPathComponent:@"ru"];
  CHECK([@"" writeToFile:en atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  CHECK([@"" writeToFile:ru atomically:YES encoding:NSUTF8StringEncoding error:nil]);
  {
    ControllerHarness h(@{
      @"AutomaticCorrectionEnabled" : @YES,
      @"EnglishDictionaryPath" : en,
      @"RussianDictionaryPath" : ru
    });
    FakeClient *client = fixtureClient();
    client.bundle = @"com.apple.TextEdit";
    [h.controller activateServer:client];
    h.settle();
    CHECK(h.type(client, @"ghbdtn "));
    h.secure = YES;
    NSUInteger calls = client.calls;
    CHECK(![h.controller handleEvent:keyDown(@"x", kVK_ANSI_X) client:client]);
    CHECK(client.calls == calls);
    h.secure = NO;
    runMainQueue();
    CHECK(client.selectedModes.count == 0);
  }
  CHECK([NSFileManager.defaultManager removeItemAtPath:root error:nil]);
}

// No key character is read while Secure Input is on, including when it starts during routing
void testControllerSecureInputBeforeCharacters() {
  currentTest = "controller_secure_input_before_characters";
  {
    ControllerHarness h(@{});
    ControllerHarness *harness = &h;
    h.secure = YES;
    FakeClient *client = fixtureClient();
    client.bundle = @"com.apple.TextEdit";
    [h.controller activateServer:client];
    h.secure = NO;
    client.hook = ^(NSString *call) {
      if ([call isEqualToString:@"bundleIdentifier"])
        harness->secure = YES;
    };
    EventSpy *spy = [[EventSpy alloc] initWithEvent:keyDown(@"g", kVK_ANSI_G)];
    CHECK(![h.controller handleEvent:(NSEvent *)spy client:client]);
    CHECK(spy.characterReads == 0 && client.writes == 0 && client.calls == 1);
    client.hook = nil;
    h.secure = NO;
    spy = [[EventSpy alloc] initWithEvent:keyDown(@"g", kVK_ANSI_G)];
    CHECK([h.controller handleEvent:(NSEvent *)spy client:client]);
    CHECK(spy.characterReads == 1);
    CHECK(documentIs(client, withAffixes(@"g"), after(@"g"), NSMakeRange(kPrefix.length, 1)));
  }
  {
    // A real Backspace under Secure Input that matches an outstanding posted echo still expires
    // tracking, so the later reversal can't delete a stale length
    ControllerHarness h(@{});
    __block NSUInteger batches = 0;
    h.controller.configureCompatibility = ^(KRCompatibilitySession *session) {
      session.targetSnapshot = ^KRCompatibilityTarget(NSString *, id<IMKTextInput>) {
        return {4242, 1, YES, 0};
      };
      session.observedKeyCount = ^uint64_t {
        return 0;
      };
      session.dispatchRewrite = ^KRCompatibilityDispatch(
          pid_t pid, const kr_compatibility_plan *plan, BOOL (^current)(void)) {
        ++batches;
        return [KRCompatibilitySession dispatchPlan:plan
                                          toProcess:pid
                                       stillCurrent:current
                                          postEvent:^(pid_t, CGEventRef){
                                          }];
      };
    };
    FakeClient *client = fixtureClient();
    client.bundle = @"com.apple.Terminal";
    [h.controller activateServer:client];
    for (NSString *unit in @[ @"g", @"h", @"b", @"d", @"t", @"n" ])
      CHECK(![h.controller handleEvent:keyDown(unit, kVK_ANSI_A) client:client]);
    CHECK([h.controller handleEvent:command(kVK_ANSI_K) client:client]);
    CHECK(batches == 1);
    h.secure = YES;
    EventSpy *backspace = [[EventSpy alloc] initWithEvent:keyDown(@"\x7f", kVK_Delete)];
    CHECK(![h.controller handleEvent:(NSEvent *)backspace client:client]);
    CHECK(backspace.characterReads == 0);
    h.secure = NO;
    [h.controller handleEvent:command(kVK_ANSI_Z) client:client];
    CHECK(batches == 1 && client.writes == 0);
  }
}

// After Keyrill switches RU to EN with the mark kept, the first English-mode key is planned in
// English: a provisional letter starts the next word instead of passing
void testKeptMarkThenProvisionalWord() {
  currentTest = "kept_mark_then_provisional_word";
  kr_engine *engine;
  KRSession *session = makeModelSession(engine, KR_RU);
  CHECK(publishEmpty(engine));
  FakeClient *client = fixtureClient();
  CHECK(type(session, client, @"руддщ "));
  CHECK(
      documentIs(client, withAffixes(@"hello "), after(@"hello "), NSMakeRange(kPrefix.length, 6)));
  CHECK(session.automaticSwitchMode == KR_EN && session.boundaryRetained);
  session.mode = KR_EN;
  CHECK(type(session, client, @"[jhjij "));
  NSString *text = @"hello хорошо ";
  CHECK(documentIs(client, withAffixes(text), after(text), NSMakeRange(after(@"hello "), 7)));
  CHECK(session.automaticSwitchMode == KR_RU);
  [session finishInClient:client];
  CHECK(documentIs(client, withAffixes(text), after(text), kNoMark));
  CHECK(!session.stopped);
}

} // namespace

int main() {
  @autoreleasepool {
    if (!NSThread.isMainThread || kr_abi_version() != KR_ABI_VERSION) {
      std::fprintf(stderr, "session_tests: wrong thread or ABI\n");
      return 2;
    }
    testKeyMapping();
    testInputStatusIsContentFreeAndDoesNotRetry();
    testLifecycleInterruptionStatus();
    testCompatibilityDispatchAndTracking();
    testCompatibilityRefusalInterruptionAndSecureInput();
    testCompatibilityKnownInterleavingHazard();
    testCompatibilityContinuesWithoutPostedEchoes();
    testCompatibilityBoundaryAcrossAutomaticModeEcho();
    testReadbackCommitAfterFailedCheck();
    testReadbackCommitKeepsWordInTextView();
    testSecureInputDuringClientCalls();
    testKeptMarkAcrossModeChange();
    testControllerCorrectionSwitchAndReversal();
    testControllerTierRouting();
    testControllerTierOverride();
    testControllerReentrantDeferredRouting();
    testControllerSecureInputCancelsQueuedSwitch();
    testControllerSecureInputBeforeCharacters();
    testKeptMarkThenProvisionalWord();
    testFixtureBothDirections();
    testManualReversalAndExpiry();
    testAutomaticSwitchRequiresVerifiedCorrectionAndFinish();
    testEscapeRestoresOriginal();
    testBoundaryThenPunctuationPassesOnce();
    testProvisionalLettersConvert();
    testEmptyComposition();
    testForeignMarkAndMovedCaret();
    testCaretMovedAfterContextRead();
    testClientChangeLeavesOldClientUntouched();
    testModeMismatchMakesNoWrite();
    testIgnoredWriteIsUnknownAndStops();
    testReconfigurationFinishesOrKeepsTheStop();
    testUnknownFinishPassesFreshKeyOnce();
    testMisbehavingFinishIsUnknown();
    testForeignMarkAfterContextRead();
    testSecureAndUnlistedClientsAreUntouched();
    testCompositionClientsAreExactAndFailClosed();
    testOverrideAdmissionKeepsAuthorityChecks();
    testLongWordFinishesWithoutSuffixCorrection();
    testOwnerReleaseDuringCallback();
    testPunctuationTokensNeverAcquireSuffix();
    testModelWholeDocument();
    testModelMissingDictionaries();
    testModelDictionaryProtectionAndLastGood();
    testModelReversalSurvivesDictionaryChange();
    testConfigurationDuringPendingPlan();
    testAdoptionOutsideHostCallbacks();
    testAdoptedSourcesDecideWholeDocument();
    testRuntimeReloadPreservesWholeDocumentAndReceipt();
    for (Fault fault : {Fault::Reenter, Fault::Throw}) {
      bool reenter = fault == Fault::Reenter;
      sweep(reenter ? "reenter_start" : "throw_start", @"", @"g", fault);
      sweep(reenter ? "reenter_continue" : "throw_continue", @"g", @"h", fault);
      sweep(reenter ? "reenter_boundary" : "throw_boundary", @"ghbdtn ", @"h", fault);
      sweep(reenter ? "reenter_space" : "throw_space", @"ghbdtn", @" ", fault);
    }
  }
  if (failures) {
    std::fprintf(stderr, "session_tests: %d failure(s)\n", failures);
    return 1;
  }
  std::printf("session_tests: passed\n");
  return 0;
}
