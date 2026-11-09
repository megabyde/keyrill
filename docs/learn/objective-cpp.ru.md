<!-- translation of docs/learn/objective-cpp.md at 6407460c8334278b32beb9b3ddd2f092b2f16080 -->

# Objective-C++

Objective-C++ — это то, что компилирует clang, когда имя файла оканчивается на `.mm`: C++ и
Objective-C в одной **единице трансляции** (англ. translation unit), то есть в одном исходном файле
вместе со всеми подключаемыми им заголовочными файлами, который компилятор превращает в один
объектный файл. Поэтому класс C++ может хранить объект Objective-C, метод Objective-C может
принимать ссылку C++, а блок может захватывать значение C++. Руководство Apple, описывавшее эту
смесь, больше не публикуется; правила кратко изложены в
[Википедии](https://en.wikipedia.org/wiki/Objective-C#Objective-C++), а во всём, что касается
памяти, справочником служит
[спецификация ARC](https://clang.llvm.org/docs/AutomaticReferenceCounting.html) clang.

В предыдущей главе был рассмотрен Objective-C в адаптере. В этой главе показано, что добавляет C++,
и для каждой конструкции объяснено, зачем адаптер ею пользуется. Читателю, не знакомому с C++,
следует читать каждый раздел ради решаемой в нём задачи; синтаксис C++ объясняется там, где он
встречается.

## Где какой язык

*Цель: научиться различать C, Objective-C и C++ в одном файле.*

Все три языка названы в строке сборки из `native/macos/build.sh`:
`clang++ -std=c++20 -fobjc-arc -fobjc-exceptions`.

| Конструкция | Язык | Где |
| -- | -- | -- |
| `[client markedRange]`, `@interface`, `@property`, `^{ }`, `@try` | Objective-C | Каждый файл адаптера |
| `kr_event`, `kr_plan`, `NSRange`, `#include "keyrill.h"` | C | ABI и типы C от Apple |
| `namespace { }`, `constexpr`, `nullptr`, `static_cast`, `enum class` | C++ | Вспомогательные функции и константы |
| `struct CallbackScope` с деструктором | C++ | `KRController.mm` |
| `- (uint32_t)apply:(const kr_plan &)plan ...` | Objective-C со ссылкой C++ | `KRSession.mm` |
| `std::atomic<bool> _stopped;` как переменная экземпляра | C++ внутри Objective-C | `KRRuntime.mm` |
| `struct Choice { NSString *title; SEL action; ... }` | Objective-C внутри C++ | `KRController.mm` |
| `std::vector`, `std::copy`, `std::memcmp`, `std::string`, `std::array` | Стандартная библиотека C++ | `main.mm`, `KRKeys.mm`, `KRSession.mm`, `KRRuntime.mm` |

> [!TIP]
> **Запомните.** Квадратные скобки и `@` относятся к Objective-C; `std::`, `namespace` и `&` в
> параметре — к C++; структуры из заголовочного файла — к C.

## Связывание C

*Цель: понять, почему заголовочный файл заключает свои объявления в `extern "C"`.*

C++ допускает две функции с одним именем и разными параметрами и поэтому кодирует типы параметров в
имени каждого символа; это и есть [декорирование имён](https://en.wikipedia.org/wiki/Name_mangling),
которое на стороне Rust было отключено (см. главу о
[границе C](boundary.ru.md#%D1%88%D0%B0%D0%B3-4-%D0%B0%D0%BD%D0%B0%D1%82%D0%BE%D0%BC%D0%B8%D1%8F-%D1%8D%D0%BA%D1%81%D0%BF%D0%BE%D1%80%D1%82%D0%B8%D1%80%D1%83%D0%B5%D0%BC%D0%BE%D0%B9-%D1%84%D1%83%D0%BD%D0%BA%D1%86%D0%B8%D0%B8)).
В C имена не декорируются, и Rust экспортирует `kr_commit` именно под этим именем. Файлы `.mm`
адаптера являются кодом на C++, поэтому, когда они подключают заголовочный файл, каждое объявление в
нём должно быть помечено как имеющее
[языковое связывание C](https://en.cppreference.com/w/cpp/language/language_linkage) (англ. language
linkage); в противном случае компилятор запросит у компоновщика декорированное имя, которого
библиотека Rust никогда не определяла:

[`include/keyrill.h:54-62`](../../include/keyrill.h#L54-L62)

```c
#ifndef KEYRILL_H
#define KEYRILL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
```

Макрос `__cplusplus` определён только при компиляции C++, так что компилятор C видит простые
объявления, а компилятор C++ — те же объявления внутри `extern "C" { ... }`. Этот блок закрывается
ближе к концу файла, непосредственно перед блоком `static_assert` для C++, показанным в главе о
[границе C](boundary.ru.md#%D1%88%D0%B0%D0%B3-1-%D0%B4%D0%BE%D0%B3%D0%BE%D0%B2%D0%BE%D1%80%D0%B8%D1%82%D1%8C%D1%81%D1%8F-%D0%BE-%D1%80%D0%B0%D1%81%D0%BF%D0%BE%D0%BB%D0%BE%D0%B6%D0%B5%D0%BD%D0%B8%D0%B8-%D0%B2-%D0%BF%D0%B0%D0%BC%D1%8F%D1%82%D0%B8).
Один и тот же заголовочный файл используется дымовым тестом на C, который `./tools/verify`
компилирует как C11, и адаптером, который компилирует его как C++20. В упражнении 7 этот блок
удаляется, и показано сообщение компоновщика.

Собственным функциям адаптера это не требуется. Функция `KREventForKey` объявлена в `KRKeys.h` и
определена в `KRKeys.mm`, а все вызывающие её файлы написаны на C++, поэтому её декорированное имя
совпадает с обеих сторон:

[`native/macos/src/KRKeys.h:10-11`](../../native/macos/src/KRKeys.h#L10-L11)

```objc
kr_event KREventForKey(unsigned short keyCode, NSEventModifierFlags flags, NSString *characters,
                       uint32_t mode);
```

> [!TIP]
> **Запомните.** C++ декорирует имена, а C — нет. `extern "C"` заставляет код на C++ искать простые
> имена, которые экспортирует Rust.

## Скрытые вспомогательные функции и константы времени компиляции

*Цель: научиться читать безымянное пространство имён и `constexpr`.*

Вспомогательная функция, которой пользуется только один файл, должна быть невидима для компоновщика,
чтобы другой файл не мог её вызвать и чтобы её имя ни с чем не совпало; в C++ это называется
**внутренним связыванием** (англ. internal linkage). В C его обеспечивает `static`, а C++ добавляет
[безымянное пространство имён](https://en.cppreference.com/w/cpp/language/namespace#Unnamed_namespaces),
которое скрывает всё своё содержимое, включая типы. В модуле Python того же можно добиться только
соглашением о начальном подчёркивании:

[`native/macos/src/KRSession.mm:6-24`](../../native/macos/src/KRSession.mm#L6-L24)

```obj-c++
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
```

[`constexpr`](https://en.cppreference.com/w/cpp/language/constexpr) требует значения, вычисленного
во время компиляции, так что `kCurrentSelection`, `NSRange` (простая структура C из Foundation),
строится компилятором. `u' '` —
[литерал `char16_t`](https://en.cppreference.com/w/cpp/language/character_literal), одна единица
UTF-16, которая напрямую сравнивается с `unichar` из Foundation. Последняя вспомогательная функция
показывает, что функция C++ может содержать Objective-C: она возвращает новую `NSString`, и ARC
управляет результатом точно так же, как в методе.

Два счётчика в пространстве имён составляют общее для процесса состояние, которое обновляют методы
класса `+beginInputCallback` и `+endInputCallback`. В Objective-C нет переменных класса, поэтому их
роль играют переменные уровня файла, а пространство имён делает их недоступными из любого другого
файла.

> [!TIP]
> **Запомните.** Безымянное пространство имён скрывает вспомогательные функции файла от всех
> остальных файлов. Значения `constexpr` вычисляются компилятором.

## RAII во время исключений Objective-C

*Цель: понять, почему одна небольшая структура C++ является главной причиной того, что эти файлы
написаны на Objective-C++.*

[**RAII**](https://en.cppreference.com/w/cpp/language/raii) (англ. resource acquisition is
initialization, «получение ресурса есть инициализация») связывает действие со временем жизни
локального объекта: **конструктор** (англ. constructor) выполняет действие, **деструктор** (англ.
destructor) его отменяет, причём деструктор срабатывает при любом выходе из области видимости — по
`return` или по исключению.
[Оператор `with`](https://docs.python.org/3/reference/compound_stmts.html#the-with-statement) в
Python выполняет ту же работу явно, а в JavaScript `try`/`finally` пишут вручную. Контроллер
использует RAII для подсчёта глубины вложенности обратных вызовов от приложения:

[`native/macos/src/KRController.mm:15-24`](../../native/macos/src/KRController.mm#L15-L24)

```obj-c++
namespace {

// Marks a host callback for the whole entry, including exception unwinding, so vocabulary
// adoption never runs inside one, even from a nested run loop serving another controller
struct CallbackScope {
  CallbackScope() { [KRSession beginInputCallback]; }
  ~CallbackScope() { [KRSession endInputCallback]; }
  CallbackScope(const CallbackScope &) = delete;
  CallbackScope &operator=(const CallbackScope &) = delete;
};
```

[`native/macos/src/KRController.mm:616-620`](../../native/macos/src/KRController.mm#L616-L620)

```obj-c++
- (BOOL)handleEvent:(NSEvent *)event client:(id)sender {
  if (!NSThread.isMainThread || !_session)
    return NO;
  CallbackScope scope;
  __attribute__((objc_precise_lifetime)) KeyrillAlphaController *retained = self;
```

Конструктор и деструктор C++ посылают сообщения Objective-C, а сам объект существует внутри метода
Objective-C. [Удалённые](https://en.cppreference.com/w/cpp/language/function#Deleted_functions)
конструктор копирования и копирующее присваивание (`= delete`) не позволяют случайно продублировать
область, что привело бы к двукратному завершению обратного вызова. В `handleEvent:` около дюжины
операторов `return`; парный вызов `endInputCallback` перед каждым из них означал бы дюжину
возможностей один из них забыть.

Слова комментария «including exception unwinding» и объясняют, почему этот файл написан на
Objective-C++, а не на Objective-C. В 64-разрядной среде выполнения исключения Objective-C
реализованы на том же механизме раскрутки, что и исключения C++, и, согласно документации Apple, во
время раскрутки исключения Objective-C выполняются и деструкторы C++, и блоки `@finally`
([Exceptions in 64-Bit Executables](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Exceptions/Articles/Exceptions64Bit.html)).
Поэтому даже исключение приложения, вырвавшееся из `handleEvent:`, корректно завершает область
обратного вызова. В чистом Objective-C для этого есть `@finally`, но нет средства, которое выполняло
бы завершающие действия автоматически.

То же относится к ARC. По умолчанию в Objective-C ARC *не* защищён от исключений: сильная локальная
переменная функции, через которую проходит исключение, так и не освобождается.
[Спецификация ARC](https://clang.llvm.org/docs/AutomaticReferenceCounting.html#exceptions)
объясняет, что Cocoa считает исключения фатальными и код завершающих действий лишь увеличивал бы
размер программы, а затем добавляет: «In Objective-C++, `-fobjc-arc-exceptions` is enabled by
default». Метод ввода, который перехватывает исключения приложения и продолжает работу, — именно та
программа, которой нужны завершающие действия, и расширение `.mm` обеспечивает их без
дополнительного флага.

> [!TIP]
> **Запомните.** Деструктор C++ срабатывает и тогда, когда через него проходит исключение
> Objective-C, а в Objective-C++ ARC на этом пути ещё и освобождает объекты.

## Ссылки C++ в методах Objective-C

*Цель: научиться читать `const T &` в сигнатуре метода.*

**Ссылка** (англ. reference) в C++ — это ещё одно имя существующего объекта; через ссылку `const`
можно читать, но нельзя писать, и, в отличие от указателя, ссылка не может быть нулевой и не может
быть перенаправлена на другой объект
([ссылки](https://en.cppreference.com/w/cpp/language/reference)). Python и JavaScript и так передают
каждый объект подобным образом, так что новым здесь является лишь то, что в C++ это нужно указывать
явно. План представляет собой структуру размером 536 байт, и метод `apply:` принимает её по ссылке
`const`, не копируя:

[`native/macos/src/KRSession.mm:445-447`](../../native/macos/src/KRSession.mm#L445-L447)

```obj-c++
- (uint32_t)apply:(const kr_plan &)plan
           client:(id<IMKTextInput>)client
       generation:(uint64_t)generation {
```

Функция может и возвращать ссылку; это позволяет вспомогательной функции выбрать, *какому* полю
структуры C вызывающий код присваивает значение:

[`native/macos/src/KRRuntime.mm:24-26`](../../native/macos/src/KRRuntime.mm#L24-L26)

```obj-c++
KRSourceStatus &source(KRRuntimeStatus &status, int index) {
  return index == kLearned ? status.learned : index ? status.russian : status.english;
}
```

[`native/macos/src/KRRuntime.mm:206-207`](../../native/macos/src/KRRuntime.mm#L206-L207)

```obj-c++
      source(_status, i) =
          _paths[i] == PathSetting::Unusable ? KRSourceStatusMissing : KRSourceStatusPending;
```

`KRRuntimeStatus` — структура C с тремя полями состояния, объявленная в `KRRuntime.h`. Без ссылки
выбор поля по номеру потребовал бы указателя или оператора `switch` при каждом присваивании.

> [!TIP]
> **Запомните.** Ссылка C++ — это имя существующего объекта, которое не может быть нулевым. Методы
> Objective-C++ могут принимать и возвращать ссылки.

## Объекты C++ внутри объектов Objective-C

*Цель: увидеть поля C++ в классе Objective-C и понять, почему два из них атомарны.*

Переменные экземпляра класса Objective-C могут иметь тип класса C++. Компилятор порождает скрытые
методы, которые вызывают конструкторы таких полей при выделении объекта и деструкторы перед
освобождением его памяти, так что поле `std::string` вначале пусто и уничтожается вместе со своим
объектом, как член класса C++. Поля загрузчика словарей демонстрируют все возможные варианты:

[`native/macos/src/KRRuntime.mm:117-139`](../../native/macos/src/KRRuntime.mm#L117-L139)

```obj-c++
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
```

Здесь соседствуют пять видов членов: структуры C и дескриптор C; массив значений `enum class` C++
(`PathSetting` —
[ограниченное перечисление](https://en.cppreference.com/w/cpp/language/enum#Scoped_enumerations),
значения которого записываются с именем типа; оно объявлено в безымянном пространстве имён файла);
объекты библиотеки C++; блоки; сильные и слабые указатели на объекты. При ARC даже
`dispatch_queue_t` является указателем на объект, который удерживается и освобождается, как любой
другой.

Причиной использовать здесь C++ служат два поля
[`std::atomic<bool>`](https://en.cppreference.com/w/cpp/atomic/atomic). Загрузчиком пользуются две
очереди: `_stopped` записывается в главном потоке и читается в блоках очереди словарей, а обычный
`bool`, разделяемый таким образом, приводит к гонке данных — неопределённому поведению в C++, как и
в Rust
([граница C](boundary.ru.md#%D0%BF%D0%BE%D1%82%D0%BE%D0%BA%D0%B8-%D0%B8-%D0%B1%D0%BB%D0%BE%D0%BA%D0%B8%D1%80%D0%BE%D0%B2%D0%BA%D0%B8)).
Собственного атомарного типа в Objective-C нет; в файле `.m` подошёл бы `_Atomic` из C11, а в файле
`.mm` принято использовать `std::atomic`. Комментарий к `_observing` указывает другой способ —
ограничение одной очередью: к этому полю обращаются только из рабочей очереди, поэтому атомарность
ему не нужна.

Блок, захватывающий объект C++, копирует его конструктором копирования. Именно так загрузчик
передаёт настроенное состояние рабочей очереди — по значению, чтобы у двух очередей не было общих
изменяемых данных:

[`native/macos/src/KRRuntime.mm:316-326`](../../native/macos/src/KRRuntime.mm#L316-L326)

```obj-c++
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
```

Тип `std::array` использован здесь ради захвата: простой массив C блок захватить не может, а объект
C++, который этот массив содержит, — может.

> [!TIP]
> **Запомните.** Поля C++ в объекте Objective-C создаются и уничтожаются вместе с ним. Флаг, общий
> для двух очередей, должен быть `std::atomic` либо доступен только из одной очереди.

## Объекты Objective-C внутри объектов C++

*Цель: увидеть, как ARC управляет указателями на объекты, хранящимися в структуре C++.*

Возможно и обратное. Структура C++ может хранить указатели на объекты Objective-C, и ARC удерживает
и освобождает их при создании, копировании и уничтожении структуры. Меню ввода описывает свои пункты
локальной таблицей:

[`native/macos/src/KRController.mm:442-461`](../../native/macos/src/KRController.mm#L442-L461)

```obj-c++
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
```

Здесь мы видим структуру, объявленную внутри функции, массив, инициализированный списком в фигурных
скобках, и [цикл `for` по диапазону](https://en.cppreference.com/w/cpp/language/range-for) — аналог
`for choice in items:` в Python, в данном случае по ссылке `const`. Первый заголовок строится во
время выполнения и не имеет другого владельца; массив удерживает его сильной ссылкой до возврата из
функции.

> [!TIP]
> **Запомните.** ARC управляет указателями на объекты внутри структур C++ так же, как и в любом
> другом месте.

## Стандартная библиотека на краях

*Цель: научиться читать места, где стандартная библиотека C++ встречается с C и Objective-C.*

Функция C, заполняющая буфер, который предоставляет вызывающий код, естественным образом сочетается
с `std::vector` — растущим массивом C++. Домашний каталог извлекается из базы учётных записей с
помощью буфера, размер которого определяется вызовом `sysconf`:

[`native/macos/src/main.mm:13-24`](../../native/macos/src/main.mm#L13-L24)

```obj-c++
// The account database, not $HOME, names the directory holding the default personal sources
static NSString *accountHome() {
  long size = sysconf(_SC_GETPW_R_SIZE_MAX);
  std::vector<char> buffer(size > 0 ? static_cast<size_t>(size) : 16384);
  passwd entry;
  passwd *result = nullptr;
  if (getpwuid_r(getuid(), &entry, buffer.data(), buffer.size(), &result) != 0 || !result ||
      !result->pw_dir)
    return nil;
  return [NSFileManager.defaultManager stringWithFileSystemRepresentation:result->pw_dir
                                                                   length:strlen(result->pw_dir)];
}
```

Обратите внимание на два вида нуля, стоящие рядом: `nullptr` для указателя C и `nil` для объекта.
Оба равны нулю; запись показывает, к какому миру принадлежит значение. `static_cast` заменяет
приведение типа в стиле C там, где преобразование обычным образом меняет тип значения, так что
читатель, разыскивающий опасные приведения, найдёт в адаптере единственный `reinterpret_cast` — в
`KRRuntime.mm`, где байты `std::string` превращаются в `const uint8_t *`, требуемый интерфейсом C
ABI.

Преобразование клавиш — самый простой пример смешения языков в адаптере, и с него удобно начинать
чтение кода на Objective-C++:

[`native/macos/src/KRKeys.mm:7-17`](../../native/macos/src/KRKeys.mm#L7-L17)

```obj-c++
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
```

[`native/macos/src/KRKeys.mm:32-47`](../../native/macos/src/KRKeys.mm#L32-L47)

```obj-c++
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
```

Здесь встречаются структура C из заголовочного файла, инициализированная списком в фигурных скобках
(где `{}` обнуляет единицы), маска `constexpr`, сообщение объекту `NSString` и `std::copy` в массив
структуры. Первая строка к тому же задаёт политику функции: событие изначально имеет тип
`KR_FINISH_REQUEST`, так что клавиша, которую функция не может классифицировать, завершает слово и
передаётся приложению, а каждый ранний `return` наследует этот осторожный ответ. Проверка длины
выполняется до вызова `getCharacters:`, который записывает данные в массив фиксированного размера в
стеке.

> [!TIP]
> **Запомните.** `nullptr` — нулевой указатель C, а `nil` — нулевой объект. Начинайте с осторожного
> ответа, чтобы каждый ранний возврат его унаследовал.

## `BOOL` и `bool`

*Цель: понять, почему код никогда не сравнивает значения с `YES`.*

Адаптер использует оба типа, и это разные типы. `BOOL` — тип Objective-C: на Apple Silicon это
настоящий `bool`, а на компьютерах Mac с процессорами Intel — `signed char`, где истиной считается
любой ненулевой байт, так что сравнение `BOOL` с `YES` может дать неверный результат. Код следует
одному правилу: `BOOL` используется там, где его требует API Objective-C, — в сигнатурах методов и в
свойствах, а `bool` C++ — во вспомогательных функциях C++ и локальных переменных. С `YES` код не
сравнивает никогда.

> [!TIP]
> **Запомните.** В интерфейсах Objective-C используйте `BOOL`, внутри C++ — `bool`, а `BOOL`
> проверяйте на истинность, а не на равенство `YES`.
