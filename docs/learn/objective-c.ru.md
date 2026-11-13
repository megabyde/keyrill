<!-- translation of docs/learn/objective-c.md at e6aa5591ae3e9b85a73416271520d1473328d340 -->

# Objective-C

[Objective-C](https://en.wikipedia.org/wiki/Objective-C) представляет собой язык C, к которому
добавлено одно новое выражение — посылка сообщения, заимствованная из
[Smalltalk](https://en.wikipedia.org/wiki/Smalltalk), — и построенная на нём объектная модель. Всё,
что разрешено в C, разрешено и здесь, поэтому адаптер может хранить сырой указатель `kr_engine *`
внутри объекта и передавать `kr_event` по указателю. В этой главе рассматривается та половина
адаптера, которая написана на Objective-C: синтаксис, объекты, управление их памятью и передача
работы между потоками. Половине на C++ посвящена [следующая глава](objective-cpp.ru.md).

Справочником по языку служит документ Apple
[Programming with Objective-C](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/Introduction/Introduction.html).
Он помещён в архив, однако в тех частях, которыми пользуется этот код, язык не изменился.

## Сообщения

*Цель: научиться читать посылку сообщения в квадратных скобках и называть вызываемый ею метод.*

Квадратные скобки кажутся непривычными лишь первое время. Запись `[receiver message]` посылает
объекту **сообщение** (англ. message), причём метод, который будет выполнен, отыскивается *по имени
во время выполнения*
([динамическая диспетчеризация](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ObjCRuntimeGuide/Articles/ocrtHowMessagingWorks.html)).
Читателю, знакомому с Python или JavaScript, это знакомо: там `obj.method()` тоже отыскивается по
имени в момент выполнения строки. Исключением является C++ с его таблицей виртуальных функций,
которая фиксируется во время компиляции.

Аргументы записываются между частями имени метода. Выражение
`[session handleEvent:&input client:client]` вызывает один метод, полное имя которого, или
**селектор** (англ. selector), — `handleEvent:client:`, по одному двоеточию на каждый аргумент.
Метод, объявленный со знаком `-`, является **методом экземпляра** (англ. instance method) и
вызывается у объекта; метод, объявленный со знаком `+`, является **методом класса** (англ. class
method) и вызывается у самого класса, подобно `@classmethod` в Python или статической функции-члену
в C++.

| Objective-C | C++ | Python | JavaScript |
| -- | -- | -- | -- |
| `[client markedRange]` | `client->markedRange()` | `client.markedRange()` | `client.markedRange()` |
| `[s handleEvent:e client:c]` | `s->handleEvent(e, c)` | `s.handleEvent(e, client=c)` | `s.handleEvent(e, c)` |
| `@interface A : NSObject <P>` | `class A : public NSObject, public P` | `class A(NSObject, P)` | `class A extends NSObject` плюс интерфейс в TypeScript |
| `@property(nonatomic) uint32_t mode;` | геттер, сеттер и поле | `@property` с сеттером | `get mode()` и `set mode(v)` |
| `^BOOL { return YES; }` | лямбда | `lambda` или вложенный `def` | стрелочная функция |
| `@"text"`, `@[a, b]`, `@{k : v}`, `@(n)` | типы библиотеки | `"text"`, `[a, b]`, `{k: v}`, `n` | `"text"`, `[a, b]`, `{k: v}`, `n` |
| `nil`, и `[nil anything]` ничего не делает и даёт нуль для объектов и чисел | `nullptr`, но вызов на нём падает | `None`, и вызов на нём выбрасывает исключение | `null`, ближе всего `obj?.method()` |
| `YES`, `NO`, `BOOL` | `true`, `false`, `bool` | `True`, `False` | `true`, `false` |

> [!TIP]
> **Запомните.** Выражение `[object part1:a part2:b]` вызывает метод `part1:part2:`, который
> отыскивается по имени во время выполнения.

## Класс в двух файлах

*Цель: научиться читать публичное объявление класса и его закрытую реализацию.*

У класса есть открытая часть — `@interface` в заголовочном файле — и тело — `@implementation` в
исходном файле. Рассмотрим интерфейс сеанса с его методами класса и единственным инициализатором:

[`native/macos/src/KRSession.h:76-92`](../../native/macos/src/KRSession.h#L76-L92)

```objc
@interface KRSession : NSObject <KRVocabularyAdopting>

/// Process-wide depth of host input callbacks on the main thread. While it is nonzero,
/// vocabulary adoption waits and runtime replacement and deferred mode requests are refused,
/// because a nested run loop inside one controller's callback can run work queued for another.
/// Begin and end off main are no-ops, and the query returns YES off main. End at zero is a no-op.
/// Overflow saturates and defers adoption until process exit.
+ (void)beginInputCallback;
+ (void)endInputCallback;
+ (BOOL)inputCallbackActive;

/// Takes ownership of `engine` and frees it in dealloc. `allowedClients` lists the bundle
/// identifiers in which a composition may start; a nonempty client bundle identifier and every
/// secure-input, ownership and readback check remain required.
- (instancetype)initWithEngine:(kr_engine *)engine
                allowedClients:(NSSet<NSString *> *)allowedClients NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;
```

`NSObject` — корневой класс, предок всех объектов в этом коде. Тип `instancetype` означает
«экземпляр того класса, у которого вызван метод», так что инициализатор подкласса возвращает
подкласс. Два макроса в конце проверяются компилятором: `NS_DESIGNATED_INITIALIZER` отмечает
единственный инициализатор, который обязан вызывать любой другой, а `NS_UNAVAILABLE` убирает
унаследованный `init`, так что выражение `[[KRSession alloc] init]`, создающее сеанс без движка,
вызывает ошибку компиляции, а не аварийное завершение впоследствии. Запись `NSSet<NSString *> *`
является *облегчённым обобщённым типом* — подсказкой, которую компилятор проверяет в месте вызова и
забывает во время выполнения, подобно аннотации типа в Python, проверяемой mypy.

Закрытые поля, или **переменные экземпляра** (англ. instance variables), записываются в фигурных
скобках после `@implementation` в исходном файле, поэтому в заголовочном файле их нет вовсе.
Заголовочный файл C++ показывает все закрытые члены каждому, кто его подключает; здесь же
заголовочный файл ничего не сообщает о внутреннем представлении:

[`native/macos/src/KRSession.mm:26-40`](../../native/macos/src/KRSession.mm#L26-L40)

```obj-c++
@implementation KRSession {
  kr_engine *_engine;
  NSSet<NSString *> *_allowedClients;
  // Bumped by every invalidation; each client call is followed by a comparison with the value
  // taken before it
  uint64_t _generation;
  // A plan is in flight; a callback that finds it set came through a nested run loop
  BOOL _busy;
  BOOL _poisoned;
  BOOL _finishVerified;
  // Local authority: the client, document start and exact text of the verified mark; the caret
  // is at its end
  id<IMKTextInput> _owner;
  NSUInteger _start;
  NSString *_text;
```

Эти комментарии являются заметками о проекте, и остальная часть учебника на них опирается; поля
`_generation` и `_busy` рассматриваются в главе
[«Реентерабельность и тесты»](reentrancy-and-tests.ru.md). Обратите внимание, что дескриптор из
Rust — обычное поле-указатель языка C, стоящее рядом с указателями на объекты.

### Создание и уничтожение объекта

*Цель: научиться читать стандартный инициализатор и понимать, для чего нужен `dealloc`.*

Объект создаётся двумя сообщениями: `[[KRSession alloc] initWithEngine:... allowedClients:...]`.
Сообщение `alloc` выделяет память, заполненную нулями, а **инициализатор** (англ. initializer),
аналог конструктора или метода `__init__` в Python, заполняет её. Инициализация является
соглашением, а не средством языка; вот её стандартная форма:

[`native/macos/src/KRSession.mm:75-91`](../../native/macos/src/KRSession.mm#L75-L91)

```obj-c++
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
```

Присваивание `self = [super init]` позволяет родительскому классу инициализировать свою часть
объекта. Родитель может вернуть `nil` или даже другой объект, поэтому результат присваивается и
проверяется. Двойные скобки сообщают компилятору, что присваивание внутри `if` сделано намеренно.
Выражение `[allowedClients copy]` сохраняет собственную неизменяемую копию, поскольку вызывающий код
мог передать изменяемое множество и впоследствии изменить его. Блок, присваиваемый в конце,
представляет собой внедрение зависимости; к нему мы вернёмся в разделе «Блоки» ниже.

Метод `dealloc` является деструктором и вызывается, когда объект отпускает последний владелец. При
ARC (он описан ниже, в разделе «Память: ARC») освобождать поля и вызывать `[super dealloc]` не
нужно: компилятор делает и то, и другое. Метод `dealloc` пишут только ради того, чего ARC не видит,
и дескриптор C — именно такой случай. Эта строка является другим концом `Box::from_raw` из главы о
[границе C](boundary.ru.md#%D1%88%D0%B0%D0%B3-2-%D0%BF%D0%B5%D1%80%D0%B5%D0%B4%D0%B0%D1%82%D1%8C-%D0%B2%D0%BB%D0%B0%D0%B4%D0%B5%D0%BD%D0%B8%D0%B5-%D0%B8-%D0%B7%D0%B0%D0%B1%D1%80%D0%B0%D1%82%D1%8C-%D0%B5%D0%B3%D0%BE-%D0%BE%D0%B1%D1%80%D0%B0%D1%82%D0%BD%D0%BE):
когда исчезает последняя ссылка на сеанс, вместе с ним уничтожается и движок Rust.

> [!TIP]
> **Запомните.** Заголовочный файл показывает интерфейс и скрывает поля. Метод `dealloc` нужен здесь
> только для того, чтобы освободить ресурс, которым не управляет ARC, — движок Rust.

## Свойства

*Цель: научиться читать атрибуты свойства и понимать, что они порождают.*

[**Свойство**](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/EncapsulatingData/EncapsulatingData.html)
(англ. property) объявляет пару **методов доступа** (англ. accessors) — метод чтения (геттер) и
метод записи (сеттер); компилятор сам пишет эти методы и поле, имя которого начинается с
подчёркивания. В Python то же делает
[`@property`](https://docs.python.org/3/library/functions.html#property), в JavaScript —
[геттеры](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Functions/get), но там
методы пишутся вручную. Атрибуты в скобках уточняют, как именно устроено свойство:

[`native/macos/src/KRSession.h:94-104`](../../native/macos/src/KRSession.h#L94-L104)

```objc
/// Input mode (KR_EN or KR_RU) of a finish the session plans on its own; the controller keeps
/// it equal to its current mode. A key carries its mode in its event.
@property(nonatomic) uint32_t mode;
/// True after an unknown write outcome, a contract violation or a poisoned engine. Ordinary
/// keys then pass with zero client calls until `activate` (never again once poisoned). After a
/// verified write, the first unknown outcome in an activation stops only that word: the next
/// text key re-arms the session and starts from fresh client checks.
@property(nonatomic, readonly) BOOL stopped;
/// YES while the session holds local authority over a verified mark.
@property(nonatomic, readonly) BOOL owned;
@property(nonatomic, readonly) KRInputStatus inputStatus;
```

[`native/macos/src/KRSession.h:112-114`](../../native/macos/src/KRSession.h#L112-L114)

```objc
/// Replaceable for controlled tests; defaults to `IsSecureEventInputEnabled`. Checked at entry
/// and again after each client query that precedes a text read or a write.
@property(nonatomic, copy) BOOL (^secureInputEnabled)(void);
```

Атрибут `nonatomic` отменяет действующую по умолчанию гарантию того, что геттер, конкурирующий с
сеттером из другого потока, всё равно увидит значение целиком; здесь вся работа идёт в **главном
потоке** (англ. main thread) — единственном, в котором macOS доставляет события ввода, — так что эта
гарантия ничего бы не дала. Атрибут `readonly` открывает только геттер: класс записывает `_stopped`
непосредственно, а извне этого сделать нельзя. Атрибут `copy` заставляет сеттер копировать аргумент;
для блока это обязательно, поскольку блок, записанный в функции, может находиться в стеке, пока его
не скопируют в кучу. Выражения `self.mode = KR_RU` и `[self setMode:KR_RU]` означают одно и то же
сообщение: точечная запись — лишь форма записи, а не прямой доступ к полю.

Свойство может быть и вычисляемым. За свойством `owned` поля нет; реализация предоставляет геттер
сама:

[`native/macos/src/KRSession.mm:93-99`](../../native/macos/src/KRSession.mm#L93-L99)

```obj-c++
- (BOOL)owned {
  return _owner != nil;
}

- (BOOL)boundaryRetained {
  return _owner && [_text hasSuffix:@" "];
}
```

Если класс пишет собственный сеттер, он должен указать с помощью `@synthesize`, каким полем
пользуется свойство; контроллер делает это, чтобы передавать новый блок Secure Input принадлежащим
ему сеансам:

[`native/macos/src/KRController.mm:175`](../../native/macos/src/KRController.mm#L175)

```obj-c++
@synthesize secureInputEnabled = _secureInputEnabled;
```

[`native/macos/src/KRController.mm:204-208`](../../native/macos/src/KRController.mm#L204-L208)

```obj-c++
- (void)setSecureInputEnabled:(BOOL (^)(void))block {
  _secureInputEnabled = [block copy];
  _session.secureInputEnabled = _secureInputEnabled;
  _compatibility.secureInputEnabled = _secureInputEnabled;
}
```

Последняя строка может выполниться, когда `_compatibility` равно `nil`, и в этом нет ошибки:
сообщение, посланное `nil`, ничего не делает, а метод, возвращающий объект, число или указатель,
даёт `nil` или нуль (результат-структура заполняется нулями не всегда, поэтому на это полагаться
нельзя). Это гарантирует язык, и код **Cocoa** — фреймворков Apple на Objective-C для приложений
macOS — постоянно на это опирается, примерно так же, как код на JavaScript опирается на
[цепочку обращений с проверкой на `null`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Operators/Optional_chaining)
(англ. optional chaining) `obj?.method()`, с той разницей, что Objective-C делает это для каждого
сообщения без всякой просьбы. У этого свойства есть и обратная сторона: ошибочное предположение о
существовании объекта не приводит ни к аварийному завершению, ни к ошибке, а даёт только нули. Это
одна из причин, по которым адаптер проверяет объекты, полученные от приложения, прежде чем ими
пользоваться, как показано ниже, в разделе «Значения извне проверяются».

Тип блочного свойства требует пояснения. Запись `BOOL (^secureInputEnabled)(void)` объявляет блок с
именем `secureInputEnabled`, который ничего не принимает и возвращает `BOOL`. Она построена так же,
как указатель на функцию C `BOOL (*f)(void)`, но со знаком `^` вместо `*`.

> [!TIP]
> **Запомните.** Свойство — это геттер и сеттер, которые пишет компилятор. Посылать сообщение `nil`
> допустимо, и оно ничего не делает; результат-объект или число равен `nil` или нулю.

## Протоколы и предварительные объявления

*Цель: научиться читать протокол и тип, который его требует.*

[**Протокол**](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/WorkingwithProtocols/WorkingwithProtocols.html)
(англ. protocol) — это именованный список методов, который может принять любой класс; ему
соответствуют класс C++, содержащий только чисто виртуальные функции,
[`typing.Protocol`](https://docs.python.org/3/library/typing.html#typing.Protocol) в Python,
интерфейс TypeScript и типаж Rust. Оба режима правки принимают следующий протокол, так что загрузчик
словарей может передать словари активному режиму, не зная его класса:

[`native/macos/src/KRSession.h:8-14`](../../native/macos/src/KRSession.h#L8-L14)

```objc
/// A session that can take the runtime's published vocabulary snapshots into its engine. Both
/// backends implement it, so `KRRuntime` adopts into whichever session the active controller
/// routed to. Returns the `kr_adopt_vocabulary` status; STATE means "not now", and the runtime
/// retries after a later observation.
@protocol KRVocabularyAdopting <NSObject>
- (uint32_t)adoptVocabulary:(kr_vocabulary *)vocabulary;
@end
```

Запись `id<KRVocabularyAdopting>` означает «любой объект, принявший этот протокол». Однако самым
важным протоколом адаптера является не собственный протокол Keyrill. Протокол `IMKTextInput` из
фреймворка Apple [InputMethodKit](https://developer.apple.com/documentation/inputmethodkit)
(**IMK**) определяет, как метод ввода взаимодействует с приложением, в котором печатает:
`markedRange`, `selectedRange`, `stringFromRange:actualRange:`,
`setMarkedText:selectionRange:replacementRange:` и `insertText:replacementRange:` являются
сообщениями этого протокола. Каждое из них представляет собой
[межпроцессное взаимодействие](https://en.wikipedia.org/wiki/Inter-process_communication) (англ.
inter-process communication, **IPC**) — запрос к другой работающей программе, приложению, которое
отвечает, когда будет готово.

Если заголовочному файлу нужно лишь назвать класс или протокол, но не обращаться к нему,
используется предварительное объявление, как в C++ с `class X;`:

[`native/macos/src/KRRuntime.h:9-10`](../../native/macos/src/KRRuntime.h#L9-L10)

```objc
@class KRSession;
@protocol KRVocabularyAdopting;
```

> [!TIP]
> **Запомните.** Протокол — это интерфейс; `id<P>` обозначает любой объект, принявший протокол `P`.
> Каждое сообщение приложению является межпроцессным взаимодействием.

## Допустимость nil и перечисления

*Цель: научиться читать пометки, с которых начинаются заголовочные файлы Keyrill.*

Заголовочные файлы проекта начинаются с `NS_ASSUME_NONNULL_BEGIN`; в пределах этой области каждый
указатель на объект считается ненулевым, если он не помечен `nullable`. Где возможно, компилятор
проверяет эти пометки, а где невозможно, они документируют контракт
([допустимость nil](https://developer.apple.com/documentation/swift/designating-nullability-in-objective-c-apis)).

Макрос `NS_ENUM` объявляет перечисление C с фиксированным базовым типом, благодаря чему размер
перечисления становится частью объявления, а не выбором компилятора:

[`native/macos/src/KRSession.h:16-30`](../../native/macos/src/KRSession.h#L16-L30)

```objc
/// Most recent input-boundary observation, shown in the input menu as the reason the last word
/// was left alone. It contains no text, offsets or client identity.
typedef NS_ENUM(uint8_t, KRInputStatus) {
  KRInputNotObserved,
  KRInputReady,
  /// The client's bundle identifier is not in the composition set.
  KRInputExcluded,
  KRInputSecure,
  /// The selection was not a collapsed caret, the client already had a mark, or the unit before
  /// the caret came back with another length or range.
  KRInputContextUnavailable,
  /// The UTF-16 unit before the caret is not whitespace: the word continues other text.
  KRInputNotAtBoundary,
  KRInputMarkMismatch,
  KRInputCaretMismatch,
```

В комментарии видно то же правило приватности, что и на стороне Rust: состояние сообщает, *почему*
слово оставлено без изменений, но не содержит ничего из набранного текста.

> [!TIP]
> **Запомните.** Указатели являются ненулевыми, если не помечены `nullable`. Состояние называет
> причину и никогда не содержит текста.

## Значения извне проверяются

*Цель: увидеть, как адаптер рассматривает каждый ответ приложения как не заслуживающий доверия.*

Объявленный тип ответа приложения — всего лишь обещание заголовочного файла, а само приложение
является чужим кодом в чужом процессе. Поэтому адаптер проверяет фактический тип ответа, прежде чем
им пользоваться, и готов к тому, что сам вызов завершится неудачей:

[`native/macos/src/KRSession.mm:340-352`](../../native/macos/src/KRSession.mm#L340-L352)

```obj-c++
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
```

Сообщение `isKindOfClass:` запрашивает у объекта его класс во время выполнения, подобно `isinstance`
в Python и `instanceof` в JavaScript. Выражение `NSString.class` даёт сам объект класса; сообщение
`class` посылается ему через точечную запись. Если приложение вернуло `nil`, первая проверка
оказывается сообщением, посланным `nil`, и даёт `NO`, так что одно выражение обрабатывает и
отсутствующий ответ, и ответ неподходящего вида. Почему после вызова метод сравнивает `generation` с
`_generation`, объясняется в главе [«Реентерабельность и тесты»](reentrancy-and-tests.ru.md).

> [!TIP]
> **Запомните.** Класс всего, что возвращает приложение, необходимо проверять, а от любого обращения
> к приложению следует ожидать исключения.

## Литералы и коллекции

*Цель: научиться читать литералы коллекций Foundation и их изменяемые варианты.*

В Foundation, базовой библиотеке Apple, коллекции хранят только объекты, а
[синтаксис литералов](https://clang.llvm.org/docs/ObjectiveCLiterals.html) позволяет записывать их
кратко. Настройка для отдельного приложения обновляется так:

[`native/macos/src/KRController.mm:520-525`](../../native/macos/src/KRController.mm#L520-L525)

```obj-c++
  NSMutableDictionary *tiers = [runtime.tierOverrides mutableCopy];
  if (name)
    tiers[bundle] = name;
  else
    [tiers removeObjectForKey:bundle];
  NSDictionary *changes = @{@"ApplicationTiers" : tiers.count ? [tiers copy] : NSNull.null};
```

Каждая коллекция представлена двумя классами — изменяемым и неизменяемым, а сообщения `mutableCopy`
и `copy` переводят коллекцию из одного вида в другой; ближайшей парой в Python являются `list` и
`tuple`. Индексная запись `tiers[bundle] = name` есть сокращённая форма сообщения
`setObject:forKey:`. Словарь не может хранить `nil`, поэтому указание «удалить этот ключ» передаётся
единственным объектом `NSNull.null`, и код, применяющий изменение, его проверяет. Для обхода
коллекции используется быстрое перечисление `for (NSString *key in changes)` — аналог
`for key in changes:` в Python.

> [!TIP]
> **Запомните.** Литералы `@[]` и `@{}` создают неизменяемые коллекции; `mutableCopy` даёт
> изменяемую версию. Положить `nil` в коллекцию нельзя, поэтому вместо него используют `NSNull`.

## Память: ARC

*Цель: понять, что ARC делает за программиста и чего он не делает.*

У каждого объекта есть **счётчик ссылок** (англ. retain count) — число его владельцев; когда счётчик
обращается в нуль, объект освобождается. При
[автоматическом подсчёте ссылок](https://clang.llvm.org/docs/AutomaticReferenceCounting.html) (англ.
Automatic Reference Counting, **ARC**) компилятор сам вставляет вызовы, увеличивающие и уменьшающие
счётчик (*retain* и *release*), так что писать их не нужно. Указатель по умолчанию является сильным
(`__strong`) и сохраняет свой объект живым; слабый указатель (`__weak`) объект не удерживает и
становится равным `nil`, когда объект уничтожается, подобно
[`weakref`](https://docs.python.org/3/library/weakref.html) в Python и
[`WeakRef`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WeakRef)
в JavaScript.

CPython также подсчитывает ссылки, и `std::shared_ptr` в C++ работает так же. Отличие от CPython и
JavaScript состоит в том, что за ARC не стоит сборщик мусора: два объекта, удерживающие друг друга
сильными указателями, навсегда сохраняют друг друга живыми; такая ситуация называется **циклом
сильных ссылок** (англ. retain cycle). Контроллер хранит слабый указатель на последний
активированный контроллер, так что эта глобальная переменная никогда не удерживает контроллер, от
которого InputMethodKit уже отказался:

[`native/macos/src/KRController.mm:10-13`](../../native/macos/src/KRController.mm#L10-L13)

```obj-c++
// Process-wide, main thread only
static __weak KeyrillAlphaController *activeController;
static KRRuntime *runtime;
static BOOL (^preferencesUpdater)(NSDictionary *);
```

У ARC нет анализатора заимствований, и одно следствие этого здесь существенно. Метод — это функция
со скрытым параметром `self`, и ARC не удерживает `self` на время вызова:
[спецификация ARC](https://clang.llvm.org/docs/AutomaticReferenceCounting.html#self) считает, что
его удерживает вызывающий код, и называет освобождение объекта во время обработки посланного ему
сообщения «undefined behavior, or at least dangerous». В большинстве программ ничто не может
освободить объект, пока выполняется один из его методов. В методе ввода — может: обращение к
приложению способно дать выполниться другим событиям, в том числе тому, из-за которого
InputMethodKit отпускает последнюю ссылку на данный сеанс. Отсюда строка, которая выглядит странно,
пока не известно её назначение:

[`native/macos/src/KRSession.mm:191-195`](../../native/macos/src/KRSession.mm#L191-L195)

```obj-c++
- (BOOL)handleEvent:(const kr_event *)input client:(id<IMKTextInput>)client {
  __attribute__((objc_precise_lifetime)) KRSession *lifetime = self;
  (void)lifetime;
  if (!NSThread.isMainThread || !input)
    return NO;
```

Локальная сильная ссылка удерживает `self` на время вызова. Без атрибута ARC вправе освободить
локальную переменную после её последнего использования, а у переменной, которую никогда не читают,
использований нет вовсе. Атрибут
[`objc_precise_lifetime`](https://clang.llvm.org/docs/AutomaticReferenceCounting.html#precise-lifetime-semantics)
предписывает ARC сохранять её до конца области видимости, а `(void)lifetime;` подавляет
предупреждение о неиспользуемой переменной. Так же начинаются и остальные точки входа сеанса и
контроллера, которые могут обратиться к приложению.

> [!TIP]
> **Запомните.** ARC подсчитывает ссылки, но циклов не собирает. Он не удерживает `self` во время
> вызова, поэтому методы, обращающиеся к приложению, удерживают `self` сами.

## Блоки

*Цель: научиться читать блок, его захваты и схему «слабая ссылка — сильная ссылка».*

[**Блок**](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/WorkingwithBlocks/WorkingwithBlocks.html)
(англ. block) является замыканием Objective-C: это литерал `^{ ... }`, который **захватывает**
(англ. captures) используемые им переменные и может быть сохранён, скопирован и вызван позднее
([спецификация языка](https://clang.llvm.org/docs/BlockLanguageSpec.html)). Существенно одно отличие
от замыканий Python и JavaScript: блок захватывает *значение*, которое локальная переменная имела в
момент создания блока, и не может присвоить ей новое значение, если переменная не объявлена как
`__block`; замыкание Python или JavaScript всегда видит саму переменную
([замыкания в JavaScript](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Guide/Closures)).

Свойство сеанса `secureInputEnabled`, задаваемое в рассмотренном выше инициализаторе, хранит блок.
Рабочий код оставляет значение по умолчанию, которое запрашивает у macOS, включён ли
[Secure Input](https://developer.apple.com/library/archive/technotes/tn2150/_index.html) — режим,
при котором клавиши поступают только в поле пароля; тест заменяет блок и включает Secure Input в
нужный момент. Это то же внедрение зависимости, которое сторона Rust выполняет с помощью
параметра-замыкания в
[`set_policy`](rust.ru.md#%D0%B7%D0%B0%D0%BC%D1%8B%D0%BA%D0%B0%D0%BD%D0%B8%D0%B5-%D0%BA%D0%B0%D0%BA-%D0%BF%D0%B0%D1%80%D0%B0%D0%BC%D0%B5%D1%82%D1%80).

Блок сохраняет живыми захваченные объекты, поэтому циклы сильных ссылок возникают легко: объект,
хранящий блок, который захватывает этот же объект, удерживает сам себя. Сильнее всего это
проявляется при работе, отложенной на будущее, и адаптер решает эту задачу по схеме *«слабая
ссылка — сильная ссылка»*:

[`native/macos/src/KRController.mm:311-317`](../../native/macos/src/KRController.mm#L311-L317)

```obj-c++
  // Permissions granted after launch take effect here; tap creation can't run in a callback
  __weak KeyrillAlphaController *weakController = self;
  dispatch_async(dispatch_get_main_queue(), ^{
    KeyrillAlphaController *controller = weakController;
    if (controller && activeController == controller && usesCompatibility(controller->_tier))
      controller->_startGuard();
  });
```

Блок захватывает только слабый указатель, поэтому блок, стоящий в очереди, не удерживает уже
закрытый контроллер. При выполнении блок берёт сильную локальную копию, которая равна либо `nil`
(контроллера больше нет, и блок ничего не делает), либо контроллеру, который теперь остаётся живым
до конца блока. Внутри блока выражение `controller->_tier` читает поле через указатель точно так же,
как в C; именно так блок добирается до закрытых полей объекта, который он держит в локальной
переменной.

Противоположное решение также принято сознательно. Работа, которую загрузчик словарей ставит в свою
очередь, захватывает `self` сильно и явно пишет `self->`, чтобы захват был виден при чтении:

[`native/macos/src/KRRuntime.mm:295-302`](../../native/macos/src/KRRuntime.mm#L295-L302)

```obj-c++
  dispatch_async(_queue, ^{
    if (self->_stopped)
      return;
    NSString *home = self->_homeResolver();
    self->_mainExecutor(^{
      [self configureDefaultsInHome:home];
    });
  });
```

Одноразовый блок, удерживающий загрузчик до своего выполнения, безвреден: цикл разрывается, когда
очередь отпускает блок. Повторяющийся таймер — другое дело: таймер живёт столько же, сколько
загрузчик, и удерживал бы его вечно, поэтому обработчик таймера захватывает `self` слабо:

[`native/macos/src/KRRuntime.mm:329-336`](../../native/macos/src/KRRuntime.mm#L329-L336)

```obj-c++
  _timer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, _queue);
  __weak KRRuntime *weakSelf = self;
  dispatch_source_set_event_handler(_timer, ^{
    [weakSelf observe];
  });
  dispatch_source_set_timer(_timer, dispatch_time(DISPATCH_TIME_NOW, (int64_t)_interval), _interval,
                            _interval / 2);
  dispatch_resume(_timer);
```

Если загрузчика уже нет, `[weakSelf observe]` оказывается сообщением, посланным `nil`, и ничего не
делает, так что явная проверка не нужна.

> [!TIP]
> **Запомните.** Блок захватывает значения и удерживает захваченные объекты. Код, который будет
> выполнен позднее, захватывает `self` слабо и берёт сильную копию в начале работы.

## Очереди вместо потоков

*Цель: научиться читать `dispatch_async` и понимать, какая очередь что выполняет.*

Адаптер никогда не создаёт потоков. Он пользуется
[Grand Central Dispatch](https://developer.apple.com/documentation/dispatch) (**GCD**) — библиотекой
очередей работ Apple: вызов `dispatch_async(queue, block)` ставит блок в **очередь** (англ. queue) и
немедленно возвращает управление. **Главная очередь** (англ. main queue) выполняет свои блоки один
за другим в главном потоке, куда поступают все обратные вызовы ввода, подобно тому как цикл событий
JavaScript выполняет обратные вызовы
([модель выполнения](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Execution_model)).
**Последовательная очередь** (англ. serial queue) выполняет свои блоки по одному, по порядку, в
одном из потоков общего пула — примерно так, как это делал бы `ThreadPoolExecutor(max_workers=1)` в
Python. Для файлов словарей процесс создаёт одну последовательную очередь с низким приоритетом:

[`native/macos/src/main.mm:26-38`](../../native/macos/src/main.mm#L26-L38)

```obj-c++
static KRRuntime *runtimeForPreferences(NSDictionary *preferences) {
  return [[KRRuntime alloc] initWithPreferences:preferences
      homeResolver:^NSString * {
        return accountHome();
      }
      workQueue:dispatch_queue_create("org.keyrill.vocabulary",
                                      dispatch_queue_attr_make_with_qos_class(DISPATCH_QUEUE_SERIAL,
                                                                              QOS_CLASS_UTILITY, 0))
      mainExecutor:^(dispatch_block_t block) {
        dispatch_async(dispatch_get_main_queue(), block);
      }
      observationInterval:2 * NSEC_PER_SEC];
}
```

Это и есть та «serialized acquisition queue», которой требует контракт словарей в заголовочном файле
([граница C](boundary.ru.md#%D0%BF%D0%BE%D1%82%D0%BE%D0%BA%D0%B8-%D0%B8-%D0%B1%D0%BB%D0%BE%D0%BA%D0%B8%D1%80%D0%BE%D0%B2%D0%BA%D0%B8)):
`kr_vocabulary_observe` выполняется только в этой очереди, а принятие снимков — только в главном
потоке. `mainExecutor` выполняет обратный шаг — «выполни это в главной очереди», подобно тому как
[`call_soon_threadsafe`](https://docs.python.org/3/library/asyncio-eventloop.html#asyncio.loop.call_soon_threadsafe)
в Python передаёт работу из рабочего потока в цикл событий. Даже этот исполнитель внедряется:
`native/macos/tests/runtime_tests.mm` передаёт исполнителя, который накапливает блоки в очереди,
пока тест их не выполнит, и тем самым чередование двух очередей превращается в фиксированный
сценарий, а не в гонку.

> [!TIP]
> **Запомните.** Работа передаётся между потоками в виде блоков, поставленных в очереди. Ввод
> обрабатывается в главной очереди, чтение файлов — в одной последовательной очереди.

## Исключения

*Цель: понять, почему метод ввода обязан перехватывать исключения, которые программы Cocoa обычно не
перехватывают.*

В Python и JavaScript исключениями сообщают об обычных сбоях. В Cocoa
[исключения](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Exceptions/Exceptions.html)
сигнализируют об ошибках программиста и перехватываются редко; об ожидаемых сбоях сообщают возвратом
`nil`, `NO` или объекта `NSError`. Однако приложение является чужим кодом, который может выбросить
из метода протокола что угодно, а исключение, вырвавшееся из обратного вызова метода ввода, может
привести к аварийному завершению самого метода ввода. Поэтому адаптер компилируется с флагом
`-fobjc-exceptions` и оборачивает каждое обращение к приложению:

[`native/macos/build.sh:31-33`](../../native/macos/build.sh#L31-L33)

```sh
  "$cxx" -std=c++20 -fobjc-arc -fobjc-exceptions -isysroot "$sdk" \
    -mmacosx-version-min=13.0 -g -Wall -Wextra -Wpedantic -Werror \
    -Wno-gnu-zero-variadic-macro-arguments -DKR_BUNDLE_IDENTIFIER="\"$bundle_id\"" \
```

Конструкция `@catch` уже встречалась выше, в `clientAllowed:`. Блок `@finally` выполняется при любом
способе выхода из блока — по `return` или по исключению, — подобно `finally` в Python и JavaScript;
с его помощью сеанс гарантирует, что флаг «план выполняется» будет сброшен:

[`native/macos/src/KRSession.mm:221-226`](../../native/macos/src/KRSession.mm#L221-L226)

```obj-c++
  _busy = YES;
  @try {
    return [self runEvent:*input client:client generation:_generation];
  } @finally {
    _busy = NO;
  }
```

Освобождаются ли объекты, на которые ссылаются локальные переменные, когда через функцию проходит
исключение, зависит от настройки компилятора, причём значение по умолчанию в Objective-C и
Objective-C++ различно; почему это важно для данного файла, объясняется в
[следующей главе](objective-cpp.ru.md#raii-%D0%B2%D0%BE-%D0%B2%D1%80%D0%B5%D0%BC%D1%8F-%D0%B8%D1%81%D0%BA%D0%BB%D1%8E%D1%87%D0%B5%D0%BD%D0%B8%D0%B9-objective-c).

> [!TIP]
> **Запомните.** В Cocoa исключения предназначены для ошибок программиста, однако ошибка приложения
> не должна приводить к аварийному завершению метода ввода, поэтому каждое обращение к приложению
> обёрнуто.

## Селекторы и target-action

*Цель: научиться читать `@selector` и понимать, что связывает пункт меню с методом.*

Селектор может быть значением: выражение `@selector(name:)` имеет тип `SEL` и подобно имени метода,
сохранённому в строке для `getattr(obj, name)` в Python. Меню в **AppKit** — части Cocoa, отвечающей
за окна, меню и события, — используют селекторы по схеме
[target-action](https://developer.apple.com/library/archive/documentation/General/Conceptual/Devpedia-CocoaApp/TargetAction.html):
пункт меню хранит селектор и при выборе посылает это сообщение своей цели. Каждый пункт «Correction
in This App» в меню ввода указывает на метод из одной строки:

[`native/macos/src/KRController.mm:540-543`](../../native/macos/src/KRController.mm#L540-L543)

```obj-c++
- (void)useDefaultTier:(id)sender {
  (void)sender;
  [self overrideTier:nil];
}
```

Двоеточие в `useDefaultTier:` является частью имени: метод действия принимает отправителя в качестве
единственного аргумента. Поскольку диспетчеризация выполняется по имени, пункт меню и метод не
связаны ничем, кроме селектора, и опечатка в `@selector(...)` компилируется без единого
предупреждения при `-Wall -Wextra`; clang предупреждает о селекторе, не объявленном ни в одном
видимом классе, только при флаге `-Wundeclared-selector`, а эта сборка его не включает.

> [!TIP]
> **Запомните.** Селектор — это имя метода в виде значения, и пункт меню связан с методом только
> этим именем.

## Где начинается процесс

*Цель: научиться читать функцию `main` программы, которая работает до выхода пользователя из
системы.*

Метод ввода является обычным приложением без окна документа; Keyrill открывает окно только для
настроек и только по команде пользователя. Функция `main` настраивает AppKit, регистрирует класс
контроллера, создаёт
[`IMKServer`](https://developer.apple.com/documentation/inputmethodkit/imkserver) и бессрочно
запускает **цикл выполнения** (англ. run loop) — цикл событий главного потока:

[`native/macos/src/main.mm:99-127`](../../native/macos/src/main.mm#L99-L127)

```obj-c++
int main() {
  @autoreleasepool {
    [NSApplication sharedApplication];
    // Registers the controller class before IMK resolves it by name from Info.plist
    (void)KeyrillAlphaController.class;
    __attribute__((objc_precise_lifetime)) KRApplicationDelegate *delegate =
        [KRApplicationDelegate new];
    NSApp.delegate = delegate;
    NSBundle *bundle = NSBundle.mainBundle;
    NSString *connection = [bundle objectForInfoDictionaryKey:@"InputMethodConnectionName"];
    if (!connection || ![bundle.bundleIdentifier isEqualToString:@KR_BUNDLE_IDENTIFIER])
      return 2;
    KRRuntime *runtime = runtimeForPreferences(
        [NSUserDefaults.standardUserDefaults persistentDomainForName:bundle.bundleIdentifier]);
    if (!runtime || ![KeyrillAlphaController setRuntime:runtime])
      return 2;
    KeyrillAlphaController.preferencesUpdater = ^BOOL(NSDictionary *changes) {
      return applyPreferences(changes);
    };
    [runtime start];
    // The server must outlive the run loop
    __attribute__((objc_precise_lifetime)) IMKServer *server =
        [[IMKServer alloc] initWithName:connection bundleIdentifier:bundle.bundleIdentifier];
    if (!server)
      return 2;
    [NSApp run];
  }
  return 0;
}
```

[`@autoreleasepool`](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/MemoryMgmt/Articles/mmAutoreleasePools.html) —
область для объектов, освобождение которых отложено, наследие ручного подсчёта ссылок, которое ARC
всё ещё использует при взаимодействии с API Apple. InputMethodKit находит класс контроллера по имени
в `Info.plist` и затем создаёт по одному
[`IMKInputController`](https://developer.apple.com/documentation/inputmethodkit/imkinputcontroller)
на каждый сеанс ввода клиента; ничто в программе не ссылается на класс напрямую, поэтому `main`
посылает ему сообщение `class`, чтобы среда выполнения гарантированно зарегистрировала его раньше,
чем InputMethodKit станет искать.

Две локальные переменные с атрибутом `objc_precise_lifetime` — то же средство, что и в
`handleEvent:`. `NSApp` не удерживает своего делегата, а на сервер больше ничто не ссылается, так
что каждая из этих переменных является единственной сильной ссылкой на свой объект. Ни одна из них
далее не читается, и без атрибута ARC мог бы освободить оба объекта ещё до вызова `[NSApp run]`,
который возвращает управление только при завершении процесса.

> [!TIP]
> **Запомните.** Функция `main` создаёт сервер и навсегда входит в цикл выполнения. Объекты, которые
> удерживает только `main`, сохраняются живыми с помощью `objc_precise_lifetime`.
