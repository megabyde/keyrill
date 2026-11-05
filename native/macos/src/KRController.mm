#import <Carbon/Carbon.h>
#import <InputMethodKit/InputMethodKit.h>

#import "KRCompatibility.h"
#import "KRController.h"
#import "KRKeys.h"
#import "KRRuntime.h"
#import "KRSession.h"

static __weak KeyrillAlphaController *activeController;
static KRRuntime *runtime;
static BOOL (^preferencesUpdater)(NSDictionary *);

namespace {

// Marks a host callback for the whole entry, including exception unwinding, so vocabulary
// adoption never runs inside one, even from a nested run loop serving another controller
struct CallbackScope {
  CallbackScope() { [KRSession beginInputCallback]; }
  ~CallbackScope() { [KRSession endInputCallback]; }
  CallbackScope(const CallbackScope &) = delete;
  CallbackScope &operator=(const CallbackScope &) = delete;
};

bool usesCompatibility(KRTier tier) { return tier == KRTierManual || tier == KRTierAutomatic; }

NSString *missingDictionaries(const KRRuntimeStatus &status) {
  bool english = status.english == KRSourceStatusMissing;
  bool russian = status.russian == KRSourceStatusMissing;
  if (english && russian)
    return @"English and Russian dictionaries";
  return english ? @"English dictionary" : russian ? @"Russian dictionary" : nil;
}

// Whether automatic correction will run on the next Space here, and if not the first reason.
// `compatibilityProblem` is nil when the compatibility backend can post and observe keys.
NSString *correctionTitle(const KRRuntimeStatus &status, KRTier tier,
                          NSString *compatibilityProblem, bool paused) {
  if (tier == KRTierOff)
    return @"Keyrill is off in this app";
  if (paused)
    return @"Correction paused in this field: check your recent text, then switch away and back";
  if (usesCompatibility(tier) && compatibilityProblem)
    return [@"Correction unavailable: " stringByAppendingString:compatibilityProblem];
  if (tier == KRTierManual)
    return @"Manual conversion only in this app";
  switch (status.policy) {
  case KRPolicyStatusOff:
    return @"Automatic correction: off";
  case KRPolicyStatusModelUnavailable:
    return @"Automatic correction unavailable: model failed to load";
  case KRPolicyStatusModel:
    break;
  }
  if (NSString *missing = missingDictionaries(status))
    return [NSString
        stringWithFormat:@"Automatic correction unavailable: %@ missing or invalid", missing];
  if (status.english == KRSourceStatusPending || status.russian == KRSourceStatusPending)
    return @"Automatic correction: loading dictionaries";
  return @"Automatic correction: ready";
}

NSString *sourceState(KRSourceStatus status) {
  switch (status) {
  case KRSourceStatusUnconfigured:
    return nil;
  case KRSourceStatusPending:
    return @"loading";
  case KRSourceStatusReady:
    return @"ready";
  case KRSourceStatusRetained:
    return @"update rejected, previous kept";
  case KRSourceStatusMissing:
    return @"missing or invalid";
  }
  return nil;
}

void addStatusItem(NSMenu *menu, NSString *title) {
  if (title)
    [menu addItemWithTitle:title action:nil keyEquivalent:@""].enabled = NO;
}

// Why the last word was left as typed, in the app's terms; content-free
NSString *inputTitle(KRInputStatus status) {
  switch (status) {
  case KRInputNotObserved:
  case KRInputReady:
    return nil;
  case KRInputExcluded:
    return @"Last word left alone: this field is excluded";
  case KRInputSecure:
    return @"Last word left alone: macOS Secure Input is on";
  case KRInputContextUnavailable:
    return @"Last word left alone: the app didn't show what precedes it";
  case KRInputNotAtBoundary:
    return @"Last word left alone: it continues other text";
  case KRInputMarkMismatch:
    return @"Last word left alone: the app changed the underlined word";
  case KRInputCaretMismatch:
    return @"Last word left alone: the app moved the caret";
  case KRInputTextUnavailable:
    return @"Last word left alone: the app doesn't report its text";
  case KRInputTextMismatch:
    return @"Last word left alone: the app's text didn't match";
  case KRInputInterrupted:
    return @"Last word left alone: the app interrupted the check";
  case KRInputEngineUnavailable:
    return @"Last word left alone: the correction engine refused it";
  case KRInputClientException:
    return @"Last word left alone: the app reported an error";
  }
  return nil;
}

NSString *tierName(KRTier tier) {
  switch (tier) {
  case KRTierOff:
    return @"off";
  case KRTierComposition:
    return @"direct editing";
  case KRTierManual:
    return @"compatibility, manual only";
  case KRTierAutomatic:
    return @"compatibility, automatic and manual";
  }
  return @"off";
}

bool isKeyrill(NSString *bundle) {
  return [bundle caseInsensitiveCompare:@KR_BUNDLE_IDENTIFIER] == NSOrderedSame;
}

} // namespace

static NSString *keyboardForMode(uint32_t mode) {
  return mode == KR_RU ? @"com.apple.keylayout.Russian" : @"com.apple.keylayout.US";
}

// IMK and shutdown callbacks keep self alive through dispatch: ARC does not retain self for a
// message send, and IMK may release the controller while a synchronous client call is on the stack
@implementation KeyrillAlphaController {
  KRSession *_session;
  KRRuntime *_configuredRuntime;
  uint32_t _mode;
  BOOL _reconfiguring;
  KRCompatibilitySession *_compatibility;
  NSString *_compatibilityBundle;
  // Routing happens at activation, or at the first key after Secure Input if it was on then
  BOOL _routed;
  KRTier _tier;
  NSString *_activeBundle;
  __weak id _routedClient;
  uint64_t _selectionGeneration;
  NSString *_layoutSwitchStatus;
  NSString *_tierStatus;
  uint32_t _requestedAutomaticMode;
  uint64_t _requestedContextGeneration;
  uint64_t _requestedSourceGeneration;
}

@synthesize secureInputEnabled = _secureInputEnabled;

+ (instancetype)activeController {
  return activeController;
}

+ (BOOL (^)(NSDictionary *))preferencesUpdater {
  return preferencesUpdater;
}

+ (void)setPreferencesUpdater:(BOOL (^)(NSDictionary *))updater {
  if (NSThread.isMainThread)
    preferencesUpdater = [updater copy];
}

+ (NSString *)inputModeForMode:(uint32_t)mode {
  return mode == KR_RU ? @KR_BUNDLE_IDENTIFIER ".Russian" : @KR_BUNDLE_IDENTIFIER ".English";
}

+ (BOOL)setRuntime:(KRRuntime *)value {
  if (!NSThread.isMainThread || [KRSession inputCallbackActive] || !value)
    return NO;
  if (runtime != value) {
    [runtime stop];
    runtime = value;
  }
  return YES;
}

- (void)setSecureInputEnabled:(BOOL (^)(void))block {
  _secureInputEnabled = [block copy];
  _session.secureInputEnabled = _secureInputEnabled;
  _compatibility.secureInputEnabled = _secureInputEnabled;
}

- (kr_engine *)newEngineForRuntime:(KRRuntime *)configuration {
  if (!configuration)
    return nullptr;
  kr_engine *engine = nullptr;
  if (kr_abi_version() != KR_ABI_VERSION || kr_engine_new(KR_ABI_VERSION, &engine) != KR_OK)
    return nullptr;
  if ([configuration configureEngine:engine] != KR_OK) {
    kr_engine_free(engine);
    return nullptr;
  }
  return engine;
}

- (KRSession *)newSessionForRuntime:(KRRuntime *)configuration {
  kr_engine *engine = [self newEngineForRuntime:configuration];
  if (!engine)
    return nil;
  KRSession *session = [[KRSession alloc] initWithEngine:engine
                                          allowedClients:configuration.compositionClients];
  session.secureInputEnabled = _secureInputEnabled;
  return session;
}

- (id)initWithServer:(IMKServer *)server delegate:(id)delegate client:(id)client {
  if (!NSThread.isMainThread)
    return nil;
  CallbackScope scope;
  if ((self = [super initWithServer:server delegate:delegate client:client])) {
    _mode = KR_EN;
    _secureInputEnabled = ^BOOL {
      return IsSecureEventInputEnabled();
    };
    _startGuard = ^{
      [KRCompatibilitySession startGuardIfPermitted];
    };
    if (runtime) {
      _session = [self newSessionForRuntime:runtime];
      _configuredRuntime = runtime;
    }
  }
  return self;
}

- (void)overrideKeyboardIn:(id)sender {
  @try {
    [sender overrideKeyboardWithKeyboardNamed:keyboardForMode(_mode)];
  } @catch (NSException *) {
    [_session invalidate];
  }
}

// Reads the client's bundle identifier only, never its text, and picks the backend for it
- (void)routeClient:(id)sender {
  _routed = NO;
  _tier = KRTierOff;
  _activeBundle = nil;
  if (!runtime || self.secureInputEnabled()) {
    [_compatibility invalidate];
    return;
  }
  // The query can reenter deactivation and another activation, which bump the generation; their
  // routing then stands
  uint64_t generation = _selectionGeneration;
  id candidate = nil;
  @try {
    candidate = [sender bundleIdentifier];
  } @catch (NSException *) {
  }
  if (generation != _selectionGeneration)
    return;
  _routed = YES;
  if ([candidate isKindOfClass:NSString.class] && [candidate length])
    _activeBundle = [candidate copy];
  _tier = [runtime tierForBundle:_activeBundle];
  NSString *bundle = _activeBundle;
  if (!usesCompatibility(_tier) || !bundle) {
    [_compatibility invalidate];
    _compatibility = nil;
    _compatibilityBundle = nil;
    if (_tier == KRTierComposition && _session)
      [runtime requestAdoptionForSession:_session];
    return;
  }
  BOOL automatic = _tier == KRTierAutomatic;
  if (!_compatibility || ![_compatibilityBundle isEqualToString:bundle] ||
      _compatibility.automatic != automatic) {
    kr_engine *engine = [self newEngineForRuntime:runtime];
    _compatibility = engine ? [[KRCompatibilitySession alloc] initWithEngine:engine
                                                            bundleIdentifier:bundle
                                                                   automatic:automatic]
                            : nil;
    _compatibility.secureInputEnabled = _secureInputEnabled;
    if (_compatibility && _configureCompatibility)
      _configureCompatibility(_compatibility);
    _compatibilityBundle = bundle;
  }
  _compatibility.mode = _mode;
  [_compatibility activate];
  if (_compatibility)
    [runtime requestAdoptionForSession:_compatibility];
  // Permissions granted after launch take effect here; tap creation can't run in a callback
  __weak KeyrillAlphaController *weakController = self;
  dispatch_async(dispatch_get_main_queue(), ^{
    KeyrillAlphaController *controller = weakController;
    if (controller && activeController == controller && usesCompatibility(controller->_tier))
      controller->_startGuard();
  });
}

- (void)activateServer:(id)sender {
  if (!NSThread.isMainThread)
    return;
  CallbackScope scope;
  __attribute__((objc_precise_lifetime)) KeyrillAlphaController *retained = self;
  activeController = self;
  ++_selectionGeneration;
  if (_reconfiguring) {
    [_session invalidate];
    [_compatibility invalidate];
    return;
  }
  if (_configuredRuntime != runtime) {
    _reconfiguring = YES;
    @try {
      if (_session && ![_session finishForReconfigurationInClient:sender]) {
        [self overrideKeyboardIn:sender];
        return;
      }
      _session = [self newSessionForRuntime:runtime];
      _configuredRuntime = runtime;
      _compatibility = nil;
      _compatibilityBundle = nil;
    } @finally {
      _reconfiguring = NO;
    }
  }
  [_session activate];
  _session.mode = _mode;
  _routedClient = sender;
  uint64_t generation = _selectionGeneration;
  [self routeClient:sender];
  if (generation == _selectionGeneration)
    [self overrideKeyboardIn:sender];
}

- (void)deactivateServer:(id)sender {
  if (!NSThread.isMainThread)
    return;
  CallbackScope scope;
  __attribute__((objc_precise_lifetime)) KeyrillAlphaController *retained = self;
  ++_selectionGeneration;
  _requestedAutomaticMode = 0;
  if (activeController == self)
    activeController = nil;
  [_session finishInClient:sender];
  [_session invalidate];
  [_compatibility invalidate];
}

- (void)commitComposition:(id)sender {
  CallbackScope scope;
  __attribute__((objc_precise_lifetime)) KeyrillAlphaController *retained = self;
  if (NSThread.isMainThread) {
    ++_selectionGeneration;
    _requestedAutomaticMode = 0;
    if (usesCompatibility(_tier))
      [_compatibility invalidate];
    else
      [_session finishInClient:sender];
  }
}

- (void)inputControllerWillClose {
  if (!NSThread.isMainThread)
    return;
  CallbackScope scope;
  __attribute__((objc_precise_lifetime)) KeyrillAlphaController *retained = self;
  ++_selectionGeneration;
  _requestedAutomaticMode = 0;
  if (activeController == self)
    activeController = nil;
  [_session finishInClient:self.client];
  [_session invalidate];
  [_compatibility invalidate];
}

- (void)setValue:(id)value forTag:(long)tag client:(id)sender {
  if (!NSThread.isMainThread)
    return;
  CallbackScope scope;
  __attribute__((objc_precise_lifetime)) KeyrillAlphaController *retained = self;
  if (tag != kTextServiceInputModePropertyTag || ![value isKindOfClass:NSString.class]) {
    [super setValue:value forTag:tag client:sender];
    return;
  }
  uint32_t mode = [value hasSuffix:@".Russian"] ? KR_RU : KR_EN;
  uint64_t context = [KRCompatibilitySession inputContextGeneration];
  uint64_t source = [KRCompatibilitySession inputSourceGeneration];
  // Only the echo of Keyrill's own request, with nothing but source changes since, keeps state
  BOOL automatic = _requestedAutomaticMode == mode && activeController == self &&
                   context >= _requestedContextGeneration && source >= _requestedSourceGeneration &&
                   context - _requestedContextGeneration == source - _requestedSourceGeneration;
  _requestedAutomaticMode = 0;
  ++_selectionGeneration;
  _layoutSwitchStatus = nil;
  // A kept mark stays owned with its reversal; its own mode still plans it, so the next key
  // reverses it or finishes it before typing in the new mode
  if (!automatic || !_session.boundaryRetained) {
    [_session finishInClient:sender];
    [_session invalidate];
  }
  if (automatic)
    [_compatibility keepAcrossAutomaticSwitchToMode:mode];
  else
    [_compatibility invalidate];
  _mode = mode;
  _session.mode = _mode;
  _compatibility.mode = _mode;
  [self overrideKeyboardIn:sender];
}

- (void)addTierItemsTo:(NSMenu *)menu {
  if (!_activeBundle || isKeyrill(_activeBundle))
    return;
  BOOL unlisted = NO;
  KRTier fallback = [runtime defaultTierForBundle:_activeBundle unlisted:&unlisted];
  NSString *chosen = runtime.tierOverrides[_activeBundle];
  NSString *origin = chosen ? @" (your setting)" : unlisted ? @" (unlisted app)" : @" (built-in)";
  if (_tier == KRTierOff && !runtime.compatibilityEnabled &&
      (usesCompatibility(fallback) || [chosen isEqualToString:@"manual"] ||
       [chosen isEqualToString:@"automatic"]))
    origin = @" (compatibility mode is off)";
  addStatusItem(menu, [NSString stringWithFormat:@"This app: %@%@", tierName(_tier), origin]);
  NSMenu *choices = [[NSMenu alloc] initWithTitle:@"Correction in This App"];
  struct Choice {
    NSString *title;
    SEL action;
    NSString *name;
  };
  const Choice items[] = {
      {[NSString stringWithFormat:@"Default (%@)", tierName(fallback)], @selector(useDefaultTier:),
       nil},
      {@"Direct Editing", @selector(useCompositionTier:), @"composition"},
      {@"Compatibility, Automatic and Manual", @selector(useAutomaticTier:), @"automatic"},
      {@"Compatibility, Manual Only", @selector(useManualTier:), @"manual"},
      {@"Off", @selector(useOffTier:), @"off"},
  };
  for (const Choice &choice : items) {
    NSMenuItem *item = [choices addItemWithTitle:choice.title
                                          action:choice.action
                                   keyEquivalent:@""];
    bool selected = choice.name ? [chosen isEqualToString:choice.name] : !chosen;
    item.state = selected ? NSControlStateValueOn : NSControlStateValueOff;
  }
  NSMenuItem *submenu = [menu addItemWithTitle:@"Correction in This App"
                                        action:nil
                                 keyEquivalent:@""];
  submenu.submenu = choices;
}

- (NSMenu *)menu {
  CallbackScope scope;
  NSMenu *menu = [[NSMenu alloc] initWithTitle:@"Keyrill"];
  if (!runtime)
    return menu;
  KRRuntimeStatus status = runtime.status;
  BOOL posting = usesCompatibility(_tier) && CGPreflightPostEventAccess();
  BOOL listening = usesCompatibility(_tier) && CGPreflightListenEventAccess();
  NSString *compatibilityProblem = !_compatibility          ? @"correction engine unavailable"
                                   : !posting || !listening ? @"permissions needed"
                                                            : nil;
  bool paused = _tier == KRTierComposition && _session.stopped;
  addStatusItem(menu, correctionTitle(status, _tier, compatibilityProblem, paused));
  if (_tier != KRTierOff) {
    addStatusItem(menu, @"Convert the current word: Control-Option-K");
    addStatusItem(menu, @"Undo a correction before the next key: Control-Option-Z");
  }
  [self addTierItemsTo:menu];
  if (_tierStatus)
    addStatusItem(menu, _tierStatus);
  addStatusItem(menu, _mode == KR_RU ? @"Typing mode: Russian" : @"Typing mode: English");
  addStatusItem(menu, runtime.automaticInputSourceSwitching ? @"Layout switching: on"
                                                            : @"Layout switching: off");
  if (_layoutSwitchStatus)
    addStatusItem(menu, _layoutSwitchStatus);
  if (usesCompatibility(_tier)) {
    if (!posting)
      addStatusItem(menu, @"Permission needed: Device Control and Data Access (Accessibility)");
    if (!listening)
      addStatusItem(menu, @"Permission needed: Input Monitoring");
    if (!compatibilityProblem)
      addStatusItem(menu, _compatibility.statusTitle);
  }
  if (_tier == KRTierComposition && _session)
    addStatusItem(menu, inputTitle(_session.inputStatus));
  NSString *english = sourceState(status.english), *russian = sourceState(status.russian);
  if (english)
    addStatusItem(menu, [@"English dictionary: " stringByAppendingString:english]);
  if (russian)
    addStatusItem(menu, [@"Russian dictionary: " stringByAppendingString:russian]);
  if (NSString *learned = sourceState(status.learned))
    addStatusItem(menu, [@"Learned words: " stringByAppendingString:learned]);
  [menu addItem:NSMenuItem.separatorItem];
  [menu addItemWithTitle:@"Settings…" action:@selector(showPreferences:) keyEquivalent:@""];
  return menu;
}

// The bundle is the one the menu showed; the change is stored and applied outside this callback
- (void)overrideTier:(nullable NSString *)name {
  CallbackScope scope;
  NSString *bundle = _activeBundle;
  if (!NSThread.isMainThread || !runtime || !bundle || isKeyrill(bundle))
    return;
  NSMutableDictionary *tiers = [runtime.tierOverrides mutableCopy];
  if (name)
    tiers[bundle] = name;
  else
    [tiers removeObjectForKey:bundle];
  NSDictionary *changes = @{@"ApplicationTiers" : tiers.count ? [tiers copy] : NSNull.null};
  __weak KeyrillAlphaController *weakController = self;
  dispatch_async(dispatch_get_main_queue(), ^{
    __attribute__((objc_precise_lifetime)) KeyrillAlphaController *controller = weakController;
    BOOL (^updater)(NSDictionary *) = preferencesUpdater;
    BOOL applied = updater && ![KRSession inputCallbackActive] && updater(changes);
    if (!controller)
      return;
    controller->_tierStatus = applied ? nil : @"This app's setting couldn't be saved";
    id client = controller->_routedClient;
    if (applied && activeController == controller && client)
      [controller activateServer:client];
  });
}

- (void)useDefaultTier:(id)sender {
  (void)sender;
  [self overrideTier:nil];
}

- (void)useCompositionTier:(id)sender {
  (void)sender;
  [self overrideTier:[KRRuntime nameForTier:KRTierComposition]];
}

- (void)useAutomaticTier:(id)sender {
  (void)sender;
  [self overrideTier:[KRRuntime nameForTier:KRTierAutomatic]];
}

- (void)useManualTier:(id)sender {
  (void)sender;
  [self overrideTier:[KRRuntime nameForTier:KRTierManual]];
}

- (void)useOffTier:(id)sender {
  (void)sender;
  [self overrideTier:[KRRuntime nameForTier:KRTierOff]];
}

- (void)showPreferences:(id)sender {
  (void)sender;
  CallbackScope scope;
  dispatch_async(dispatch_get_main_queue(), ^{
    [NSApp sendAction:@selector(showKeyrillPreferences:) to:NSApp.delegate from:nil];
  });
}

- (NSUInteger)recognizedEvents:(id)sender {
  (void)sender;
  // Left mouse down finishes an owned mark; the click itself is always returned unhandled
  return NSEventMaskKeyDown | NSEventMaskLeftMouseDown;
}

- (void)requestInputMode:(uint32_t)target client:(id<IMKTextInput>)client {
  if (!runtime.automaticInputSourceSwitching || target == _mode ||
      (target != KR_EN && target != KR_RU))
    return;
  uint64_t generation = ++_selectionGeneration;
  uint64_t contextGeneration = [KRCompatibilitySession inputContextGeneration];
  uint32_t sourceMode = _mode;
  __weak KeyrillAlphaController *weakController = self;
  id<IMKTextInput> requestedClient = client;
  // Let the original key return to the client before requesting its next input mode
  dispatch_async(dispatch_get_main_queue(), ^{
    __attribute__((objc_precise_lifetime)) KeyrillAlphaController *controller = weakController;
    __attribute__((objc_precise_lifetime)) id<IMKTextInput> currentClient = requestedClient;
    // A source change since the request shows in the generation and mode. TIS can't tell: in
    // this process it reports the overridden keyboard layout, not Keyrill's input mode
    if (!controller || !currentClient || activeController != controller ||
        controller->_selectionGeneration != generation || controller->_mode != sourceMode ||
        controller->_configuredRuntime != runtime || [KRSession inputCallbackActive] ||
        [KRCompatibilitySession inputContextGeneration] != contextGeneration ||
        !runtime.automaticInputSourceSwitching || controller->_secureInputEnabled())
      return;
    @try {
      controller->_requestedAutomaticMode = target;
      controller->_requestedContextGeneration = contextGeneration;
      controller->_requestedSourceGeneration = [KRCompatibilitySession inputSourceGeneration];
      [currentClient selectInputMode:[KeyrillAlphaController inputModeForMode:target]];
    } @catch (NSException *) {
      controller->_requestedAutomaticMode = 0;
      controller->_layoutSwitchStatus = @"Layout switch failed; select the layout manually";
    }
  });
}

- (BOOL)handleEvent:(NSEvent *)event client:(id)sender {
  if (!NSThread.isMainThread || !_session)
    return NO;
  CallbackScope scope;
  __attribute__((objc_precise_lifetime)) KeyrillAlphaController *retained = self;
  // Secure Input passes every key before echo matching reads its characters. Like any newer
  // input, it cancels a queued mode switch and expires tracking, so a reversal can't later
  // delete a stale length
  if (self.secureInputEnabled()) {
    ++_selectionGeneration;
    _requestedAutomaticMode = 0;
    [_session invalidate];
    [_compatibility invalidate];
    return NO;
  }
  BOOL synthetic = [KRCompatibilitySession isSyntheticEvent:event.CGEvent];
  if (usesCompatibility(_tier) && [_compatibility shouldPassPostedEvent:event.CGEvent]) {
    if (!synthetic) {
      kr_event command = KREventForKey(event.keyCode, event.modifierFlags, nil, _mode);
      return command.role == KR_MANUAL || command.role == KR_REVERSE;
    }
    return NO;
  }
  if (synthetic)
    return NO;
  if (event.type == NSEventTypeKeyDown) {
    ++_selectionGeneration;
    _requestedAutomaticMode = 0;
  }
  if (_configuredRuntime != runtime) {
    [_session finishInClient:sender];
    [_compatibility invalidate];
    return NO;
  }
  @try {
    if (!_routed) {
      uint64_t generation = _selectionGeneration;
      [self routeClient:sender];
      // Routing queried the client: a reentrant lifecycle change makes this key stale, and
      // Secure Input may have started
      if (generation != _selectionGeneration || !_routed || self.secureInputEnabled()) {
        if (generation == _selectionGeneration) {
          [_session invalidate];
          [_compatibility invalidate];
        }
        return NO;
      }
    }
    if (event.type == NSEventTypeLeftMouseDown) {
      if (usesCompatibility(_tier))
        [_compatibility invalidate];
      else
        [_session finishInClient:sender];
      return NO;
    }
    if (event.type != NSEventTypeKeyDown || _tier == KRTierOff)
      return NO;
    kr_event input = KREventForKey(event.keyCode, event.modifierFlags, event.characters, _mode);
    if (usesCompatibility(_tier)) {
      KRCompatibilitySession *session = _compatibility;
      BOOL handled = [session handleEvent:&input client:sender];
      if (session == _compatibility && session.automaticSwitchMode)
        [self requestInputMode:session.automaticSwitchMode client:sender];
      return handled;
    }
    // The mark stays owned while the mode switch is requested, so reversal survives the echo
    KRSession *session = _session;
    BOOL handled = [session handleEvent:&input client:sender];
    if (session == _session && session.automaticSwitchMode)
      [self requestInputMode:session.automaticSwitchMode client:sender];
    return handled;
  } @catch (NSException *) {
    [_session invalidate];
    [_compatibility invalidate];
    return NO;
  }
}

- (void)finishForShutdown {
  CallbackScope scope;
  __attribute__((objc_precise_lifetime)) KeyrillAlphaController *retained = self;
  ++_selectionGeneration;
  _requestedAutomaticMode = 0;
  if (activeController == self)
    activeController = nil;
  if (NSThread.isMainThread)
    [_session finishInClient:self.client];
  if (NSThread.isMainThread)
    [_compatibility invalidate];
}

@end
