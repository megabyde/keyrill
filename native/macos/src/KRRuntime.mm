#import "KRRuntime.h"

#import "KRSession.h"

#include <array>
#include <atomic>
#include <string>

namespace {

enum class PathSetting { Default, Explicit, Unusable };

// The two Vim languages, then the list macOS keeps for Learn Spelling
constexpr int kSources = 3, kLearned = 2;

struct Sources {
  KRSourceStatus status[kSources];
  bool observed[kSources];
};

constexpr uint32_t kTags[kSources] = {KR_EN, KR_RU, KR_LEARNED};
NSString *const kPathKeys[2] = {@"EnglishDictionaryPath", @"RussianDictionaryPath"};

KRSourceStatus &source(KRRuntimeStatus &status, int index) {
  return index == kLearned ? status.learned : index ? status.russian : status.english;
}

bool flag(id value) { return [value isKindOfClass:NSNumber.class] && [value boolValue]; }

NSString *const kTierNames[] = {@"off", @"composition", @"manual", @"automatic"};

// Tested composition clients, matched exactly
NSSet<NSString *> *compositionBundles() {
  return [NSSet
      setWithObjects:@"com.apple.TextEdit", @"com.apple.Safari", @"org.mozilla.firefox", nil];
}

// Lowercased exact identifiers, then lowercased prefixes ending in a dot
NSDictionary<NSString *, NSNumber *> *builtInTiers() {
  NSNumber *manual = @(KRTierManual), *off = @(KRTierOff);
  return @{
    // Terminals: Backspace and keycodes reach the shell or TUI
    @"com.apple.terminal" : manual,
    @"com.googlecode.iterm2" : manual,
    @"com.mitchellh.ghostty" : manual,
    @"com.github.wez.wezterm" : manual,
    @"org.alacritty" : manual,
    @"net.kovidgoyal.kitty" : manual,
    @"dev.warp.warp-stable" : manual,
    @"dev.warp.warp-preview" : manual,
    @"co.zeit.hyper" : manual,
    @"com.umputun.agterm" : manual,
    @"com.umputun.agterm.debug" : manual,
    // Editors with completion, which can consume Space or Backspace
    @"com.apple.dt.xcode" : manual,
    @"com.microsoft.vscode" : manual,
    @"com.microsoft.vscodeinsiders" : manual,
    @"com.jetbrains." : manual,
    @"com.sublimetext.3" : manual,
    @"com.sublimetext.4" : manual,
    @"dev.zed.zed" : manual,
    @"dev.zed.zed-preview" : manual,
    @"org.vim.macvim" : manual,
    @"org.gnu.emacs" : manual,
    @"com.microsoft.word" : manual,
    @"com.microsoft.excel" : manual,
    @"com.microsoft.powerpoint" : manual,
    @"com.microsoft.outlook" : manual,
    // Password managers, remote desktops and virtual machines
    @"com.1password.1password" : off,
    @"com.agilebits.onepassword7" : off,
    @"com.apple.passwords" : off,
    @"com.apple.keychainaccess" : off,
    @"com.apple.screensharing" : off,
    @"com.microsoft.rdc.macos" : off,
    @"com.parallels.desktop.console" : off,
    @"com.utmapp.utm" : off,
    @"com.vmware.fusion" : off,
  };
}

bool isTierName(id value, KRTier &tier) {
  for (uint8_t i = 0; i < 4; ++i) {
    if ([value isEqual:kTierNames[i]]) {
      tier = static_cast<KRTier>(i);
      return true;
    }
  }
  return false;
}

bool isKeyrill(NSString *bundle) {
  return [bundle caseInsensitiveCompare:@KR_BUNDLE_IDENTIFIER] == NSOrderedSame;
}

// Raw file-system bytes for kr_vocabulary_configure; empty when the value is unusable
std::string pathBytes(id path) {
  char bytes[KR_MAX_PATH_BYTES + 1];
  if (![path isKindOfClass:NSString.class] ||
      ![path getFileSystemRepresentation:bytes maxLength:sizeof bytes] || bytes[0] != '/')
    return {};
  return bytes;
}

} // namespace

@implementation KRRuntime {
  KRRuntimeStatus _status;
  uint32_t _enginePolicy;
  kr_vocabulary *_vocabulary;
  PathSetting _paths[2];
  std::string _explicitPaths[2];
  NSString *_Nullable (^_homeResolver)(void);
  dispatch_queue_t _queue;
  void (^_mainExecutor)(dispatch_block_t);
  uint64_t _interval;
  dispatch_source_t _timer;
  bool _configured[kSources];
  bool _learnedWordsEnabled;
  bool _started;
  __weak id<KRVocabularyAdopting> _target;
  // Written on main; read by observation rounds to keep retrying a refused adoption
  std::atomic<bool> _needsAdoption;
  std::atomic<bool> _stopped;
  NSDictionary<NSString *, NSNumber *> *_builtIn;
  // Work-queue confined once observation begins
  bool _observing;
  Sources _sources;
  bool _active[kSources];
}

+ (uint32_t)policyFromPreferences:(NSDictionary *)preferences {
  return flag(preferences[@"AutomaticCorrectionEnabled"]) ? KR_POLICY_MODEL : KR_POLICY_OFF;
}

+ (NSString *)learnedWordsPathInHome:(NSString *)home {
  if (![home isKindOfClass:NSString.class])
    return nil;
  return [NSString pathWithComponents:@[
    home, @"Library", @"Group Containers", @"group.com.apple.AppleSpell", @"Library", @"Spelling",
    @"LocalDictionary"
  ]];
}

- (instancetype)initWithPreferences:(NSDictionary *)preferences
                       homeResolver:(NSString *_Nullable (^)(void))homeResolver
                          workQueue:(dispatch_queue_t)workQueue
                       mainExecutor:(void (^)(dispatch_block_t))mainExecutor
                observationInterval:(uint64_t)observationInterval {
  if (!NSThread.isMainThread)
    return nil;
  if ((self = [super init])) {
    _homeResolver = [homeResolver copy];
    _queue = workQueue;
    _mainExecutor = [mainExecutor copy];
    _interval = observationInterval;
    _learnedWordsEnabled = flag(preferences[@"LearnedWordsEnabled"]);
    id compatibility = preferences[@"CompatibilityEnabled"];
    _compatibilityEnabled = !compatibility || flag(compatibility);
    _unlistedAutomaticCorrection = flag(preferences[@"UnlistedAutomaticCorrectionEnabled"]);
    _builtIn = builtInTiers();
    NSMutableDictionary<NSString *, NSString *> *overrides = [NSMutableDictionary dictionary];
    id stored = preferences[@"ApplicationTiers"];
    if ([stored isKindOfClass:NSDictionary.class]) {
      for (id bundle in stored) {
        KRTier tier;
        if ([bundle isKindOfClass:NSString.class] && [bundle length] && !isKeyrill(bundle) &&
            isTierName(stored[bundle], tier))
          overrides[bundle] = kTierNames[tier];
      }
    }
    _tierOverrides = [overrides copy];
    NSMutableSet<NSString *> *composition = [NSMutableSet set];
    for (NSString *bundle in compositionBundles()) {
      if (!_tierOverrides[bundle])
        [composition addObject:bundle];
    }
    for (NSString *bundle in _tierOverrides) {
      if ([_tierOverrides[bundle] isEqualToString:kTierNames[KRTierComposition]])
        [composition addObject:bundle];
    }
    _compositionClients = [composition copy];
    id switching = preferences[@"AutomaticInputSourceSwitchingEnabled"];
    _automaticInputSourceSwitching = !switching || flag(switching);
    _enginePolicy = [KRRuntime policyFromPreferences:preferences];
    _status.policy = KRPolicyStatusOff;
    if (_enginePolicy == KR_POLICY_MODEL) {
      // Validates the embedded asset once per process here, so no input callback pays for it
      kr_engine *probe = nullptr;
      bool valid = kr_engine_new(KR_ABI_VERSION, &probe) == KR_OK &&
                   kr_set_automatic_policy(probe, KR_POLICY_MODEL) == KR_OK;
      kr_engine_free(probe);
      if (!valid || kr_vocabulary_new(KR_ABI_VERSION, &_vocabulary) != KR_OK) {
        _vocabulary = nullptr;
        _enginePolicy = KR_POLICY_OFF;
        _status.policy = KRPolicyStatusModelUnavailable;
      } else {
        _status.policy = KRPolicyStatusModel;
      }
    }
    for (int i = 0; i < 2 && _vocabulary; ++i) {
      id value = preferences[kPathKeys[i]];
      _explicitPaths[i] = value ? pathBytes(value) : std::string();
      _paths[i] = !value                      ? PathSetting::Default
                  : _explicitPaths[i].empty() ? PathSetting::Unusable
                                              : PathSetting::Explicit;
      source(_status, i) =
          _paths[i] == PathSetting::Unusable ? KRSourceStatusMissing : KRSourceStatusPending;
    }
    if (_vocabulary && _learnedWordsEnabled)
      _status.learned = KRSourceStatusPending;
  }
  return self;
}

- (void)dealloc {
  if (_timer)
    dispatch_source_cancel(_timer);
  kr_vocabulary_free(_vocabulary);
}

- (KRRuntimeStatus)status {
  return _status;
}

+ (NSString *)nameForTier:(KRTier)tier {
  return tier <= KRTierAutomatic ? kTierNames[tier] : kTierNames[KRTierOff];
}

- (KRTier)defaultTierForBundle:(NSString *)bundle unlisted:(BOOL *)unlisted {
  if (unlisted)
    *unlisted = NO;
  if ([compositionBundles() containsObject:bundle])
    return KRTierComposition;
  NSString *lower = bundle.lowercaseString;
  NSNumber *tier = _builtIn[lower];
  for (NSString *key in _builtIn) {
    if (!tier && [key hasSuffix:@"."] && [lower hasPrefix:key])
      tier = _builtIn[key];
  }
  if (tier)
    return static_cast<KRTier>(tier.unsignedCharValue);
  if (unlisted)
    *unlisted = YES;
  return _unlistedAutomaticCorrection ? KRTierAutomatic : KRTierManual;
}

- (KRTier)tierForBundle:(id)bundle {
  if (![bundle isKindOfClass:NSString.class] || ![bundle length] || isKeyrill(bundle))
    return KRTierOff;
  KRTier tier;
  if (!isTierName(_tierOverrides[bundle], tier))
    tier = [self defaultTierForBundle:bundle unlisted:nullptr];
  if (!_compatibilityEnabled && (tier == KRTierManual || tier == KRTierAutomatic))
    return KRTierOff;
  return tier;
}

- (uint32_t)configureEngine:(kr_engine *)engine {
  if (!NSThread.isMainThread)
    return KR_STATE;
  return kr_set_automatic_policy(engine, _enginePolicy);
}

// For a language's default setting `path` is the home directory, and Rust names the spell file
// under it. Only a default location may be absent: a configured file that is missing stays missing
- (bool)configure:(int)index path:(const std::string &)path {
  auto bytes = reinterpret_cast<const uint8_t *>(path.data());
  auto len = static_cast<uint32_t>(path.size());
  uint32_t status = KR_PATH;
  if (!path.empty()) {
    if (index == kLearned)
      status =
          kr_vocabulary_configure(_vocabulary, KR_LEARNED, bytes, len, KR_SOURCE_MISSING_IS_EMPTY);
    else if (_paths[index] == PathSetting::Default)
      status = kr_vocabulary_configure_default(_vocabulary, kTags[index], bytes, len);
    else
      status = kr_vocabulary_configure(_vocabulary, kTags[index], bytes, len, 0);
  }
  bool configured = status == KR_OK;
  if (!configured)
    source(_status, index) = KRSourceStatusMissing;
  _configured[index] = configured;
  return configured;
}

- (void)start {
  if (!NSThread.isMainThread || !_vocabulary || _started || _stopped)
    return;
  _started = true;
  for (int i = 0; i < 2; ++i) {
    if (_paths[i] == PathSetting::Explicit)
      [self configure:i path:_explicitPaths[i]];
  }
  // Resolve the home off main for default paths and the optional learned list
  dispatch_async(_queue, ^{
    if (self->_stopped)
      return;
    NSString *home = self->_homeResolver();
    self->_mainExecutor(^{
      [self configureDefaultsInHome:home];
    });
  });
}

- (void)configureDefaultsInHome:(NSString *)home {
  if (_stopped)
    return;
  for (int i = 0; i < 2; ++i) {
    if (_paths[i] == PathSetting::Default)
      [self configure:i path:pathBytes(home)];
  }
  if (_learnedWordsEnabled)
    [self configure:kLearned path:pathBytes([KRRuntime learnedWordsPathInHome:home])];
  [self beginObserving];
}

// Hands the configured state to the work queue, then observes immediately and periodically
- (void)beginObserving {
  Sources sources = {{_status.english, _status.russian, _status.learned}, {}};
  std::array<bool, kSources> configured = {_configured[0], _configured[1], _configured[kLearned]};
  dispatch_async(_queue, ^{
    self->_observing = true;
    self->_sources = sources;
    for (int i = 0; i < kSources; ++i)
      self->_active[i] = configured[i];
    [self observe];
  });
  if (!_interval)
    return;
  _timer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, _queue);
  __weak KRRuntime *weakSelf = self;
  dispatch_source_set_event_handler(_timer, ^{
    [weakSelf observe];
  });
  dispatch_source_set_timer(_timer, dispatch_time(DISPATCH_TIME_NOW, (int64_t)_interval), _interval,
                            _interval / 2);
  dispatch_resume(_timer);
}

- (void)tick {
  dispatch_async(_queue, ^{
    [self observe];
  });
}

// Work queue only. Wakes the main thread only for a publication, a status change or a pending
// adoption retry.
- (void)observe {
  if (_stopped || !_observing)
    return;
  Sources next = _sources;
  bool published = false;
  for (int i = 0; i < kSources; ++i) {
    if (!_active[i])
      continue;
    uint32_t observation = KR_SOURCE_PENDING;
    if (kr_vocabulary_observe(_vocabulary, kTags[i], &observation) != KR_OK) {
      next.status[i] = next.observed[i] ? KRSourceStatusRetained : KRSourceStatusMissing;
      continue;
    }
    if (observation != KR_SOURCE_PENDING)
      next.observed[i] = true;
    published = published || observation == KR_SOURCE_PUBLISHED;
    next.status[i] = next.observed[i] ? KRSourceStatusReady : KRSourceStatusPending;
  }
  bool changed = false;
  for (int i = 0; i < kSources; ++i)
    changed = changed || next.status[i] != _sources.status[i];
  _sources = next;
  if (!published && !changed && !_needsAdoption)
    return;
  _mainExecutor(^{
    if (self->_stopped)
      return;
    for (int i = 0; i < kSources; ++i)
      source(self->_status, i) = next.status[i];
    if (published)
      self->_needsAdoption = true;
    [self adoptIfNeeded];
  });
}

- (void)requestAdoptionForSession:(id<KRVocabularyAdopting>)session {
  if (!NSThread.isMainThread || !_vocabulary || _stopped)
    return;
  _target = session;
  _needsAdoption = true;
  _mainExecutor(^{
    [self adoptIfNeeded];
  });
}

- (void)adoptIfNeeded {
  if (_stopped || !_needsAdoption || [KRSession inputCallbackActive])
    return;
  id<KRVocabularyAdopting> target = _target;
  // STATE means a callback, a busy session or a pending plan; the next observation retries
  if (!target || [target adoptVocabulary:_vocabulary] != KR_STATE)
    _needsAdoption = false;
}

- (void)stop {
  if (!NSThread.isMainThread)
    return;
  _stopped = true;
  if (_timer)
    dispatch_source_cancel(_timer);
}

@end
