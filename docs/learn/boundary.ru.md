<!-- translation of docs/learn/boundary.md at bbcfba59df0695b82d96a12e97a89edfadb17079 -->

# Граница C

Rust и Objective-C++ не могут вызывать друг друга непосредственно. Они встречаются на единственном
интерфейсе, понятном обоим, — на языке C. Эта глава посвящена месту их встречи: тому, что компилятор
перестаёт здесь доказывать, и тому, что делает код вместо этого.

## Почему C и что такое ABI

*Цель: понять, что такое ABI и FFI и из каких файлов они состоят в Keyrill.*

Когда скомпилированная программа вызывает функцию из другой скомпилированной библиотеки, обе стороны
должны договориться о подробностях, не видных ни в каком исходном тексте: в каких регистрах
процессора передаются аргументы (**соглашение о вызовах**, англ. calling convention); какое точное
имя имеет функция в скомпилированном файле (её **символ**, англ. symbol, по которому
**компоновщик**, англ. linker, — инструмент, собирающий скомпилированные файлы в одну программу, —
сопоставляет файлы между собой); каково побайтовое расположение каждой передаваемой структуры. Такая
договорённость называется
[двоичным интерфейсом приложения](https://en.wikipedia.org/wiki/Application_binary_interface) (англ.
application binary interface, **ABI**). С ABI языка C умеет работать любой язык, поэтому C и служит
здесь общим языком.

**Интерфейсом внешних функций** (англ. foreign function interface, **FFI**) называется та часть
языка, которая позволяет вызывать код на другом языке или быть вызванным из него. В Python для этого
служат [`ctypes`](https://docs.python.org/3/library/ctypes.html) и cffi, в Node.js —
[Node-API](https://nodejs.org/api/n-api.html), а в Rust — функции `extern "C"`, описанные в
[Rustonomicon](https://doc.rust-lang.org/nomicon/ffi.html). FFI в Keyrill состоит из двух половин:

- крейта `keyrill-ffi`, скомпилированного в **статическую библиотеку** (англ. static library) — файл
  скомпилированного кода, который компоновщик копирует в программу для macOS;
- **заголовочного файла** (англ. header) C `include/keyrill.h` — файла объявлений, который
  подключают код на C, C++ и Objective-C++, чтобы компилятору были известны имена, типы и
  расположение структур на другой стороне.

> [!TIP]
> **Запомните.** ABI — это договорённость между скомпилированными программами на машинном уровне. В
> Keyrill используется ABI языка C: на стороне Rust ему соответствует `keyrill-ffi`, на стороне C —
> `include/keyrill.h`.

## Где живёт `unsafe`

*Цель: научиться читать блок `unsafe` и комментарий, который обязан его обосновывать.*

В ядре `unsafe` запрещён полностью
([Rust в ядре](rust.ru.md#%D0%BC%D0%BE%D0%B4%D1%83%D0%BB%D0%B8-%D0%B4%D0%BE%D0%BA%D1%83%D0%BC%D0%B5%D0%BD%D1%82%D0%B0%D1%86%D0%B8%D1%8F-%D0%B8-%D0%BB%D0%B8%D0%BD%D1%82%D1%8B)),
поэтому все блоки `unsafe` проекта находятся в `keyrill-ffi`. Это слово легко понять неправильно.
`unsafe` не отключает ни одной проверки: анализатор заимствований и проверка типов действуют внутри
блока `unsafe` так же, как и везде. Оно лишь разрешает пять дополнительных операций, из которых в
этом крейте используются две: чтение через **необработанный указатель** (англ. raw pointer) `*mut T`
или `*const T` — простой адрес в духе C, у которого нет времени жизни и нет гарантии, что он
указывает на что-либо допустимое, — и вызов функции, объявленной как `unsafe fn`.

Ошибка в любой из этих операций приводит к **неопределённому поведению** (англ. undefined behavior):
программа может завершиться аварийно, испортить память или выглядеть работающей, и ничто не
гарантирует, какой из исходов наступит. Python попадает в ту же область, когда `ctypes` получает
неверный адрес и интерпретатор завершается аварийно. Поэтому блок `unsafe` фиксирует, что
ответственность за доказательство отныне несёт *человек*, а не компилятор, и каждый блок здесь
сопровождается комментарием `// SAFETY:`, в котором изложено обоснование, как того требуют
[справочник Rust](https://doc.rust-lang.org/reference/unsafety.html) и
[глава книги о небезопасном Rust](https://doc.rust-lang.org/book/ch20-01-unsafe-rust.html).

Функция `unsafe fn` имеет **предусловие** (англ. precondition) — условие, которое должен обеспечить
вызывающий код и которое компилятор проверить не может; поэтому её вызов требует блока `unsafe`. В
редакции 2024 тело функции `unsafe fn` является обычным безопасным кодом, так что для каждой
небезопасной операции внутри требуется собственный блок с собственным обоснованием
([`unsafe_op_in_unsafe_fn`](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-op-in-unsafe-fn.html));
рабочая область повышает этот линт с уровня предупреждения до уровня ошибки.

> [!TIP]
> **Запомните.** `unsafe` не отключает никаких проверок; оно позволяет человеку поручиться за то,
> чего компилятор доказать не может, а комментарий `SAFETY` является письменным доказательством.

## Шаг 1: договориться о расположении в памяти

*Цель: увидеть, как два объявления одной структуры на двух языках удерживаются одинаковыми.*

Rust вправе переставлять поля структуры, чтобы сэкономить **выравнивающие байты** (англ. padding) —
неиспользуемые байты, которые компилятор вставляет, чтобы каждое поле начиналось с подходящего
адреса. Атрибут
[`#[repr(C)]`](https://doc.rust-lang.org/reference/type-layout.html#the-c-representation) запрещает
такую перестановку и располагает поля по правилам C, в порядке объявления. Класс
[`ctypes.Structure`](https://docs.python.org/3/library/ctypes.html#structures-and-unions) в Python
со своим списком `_fields_` выполняет ту же работу со стороны Python:

[`crates/keyrill-ffi/src/lib.rs:132-153`](../../crates/keyrill-ffi/src/lib.rs#L132-L153)

```rust
/// `kr_event`: one key or command, read by value; `len` counts the UTF-16 units in use.
#[repr(C)]
pub struct NativeEvent {
    role: u32,
    mode: u32,
    context: u32,
    len: u32,
    units: [u16; 8],
}

/// `kr_plan`: a proposal copied into caller-owned arrays; no Rust allocation crosses the
/// interface.
#[repr(C)]
pub struct NativePlan {
    plan_id: u64,
    action: u32,
    disposition: u32,
    expected_len: u32,
    mark_len: u32,
    expected: [u16; 128],
    mark: [u16; 128],
}
```

Те же структуры объявлены ещё раз на языке C, и ничто, кроме договорённости, не связывает два
объявления. Поэтому расположение закреплено с обеих сторон. В Rust элемент `const` вычисляется
компилятором, и невыполненный `assert!` внутри него является ошибкой компиляции:

[`crates/keyrill-ffi/src/lib.rs:167-176`](../../crates/keyrill-ffi/src/lib.rs#L167-L176)

```rust
// The same layouts the header's static assertions pin
const _: () = assert!(size_of::<CompatibilityPlan>() == 280);

const _: () = assert!(size_of::<NativeEvent>() == 32);
const _: () = assert!(size_of::<NativePlan>() == 536);
const _: () = assert!(std::mem::offset_of!(NativePlan, expected) == 24);
// A word plus its Space fits the 128-unit plan arrays
const _: () = assert!(keyrill_core::session::MAX_WORD < 128);
// KR_MAX_DICTIONARY_BYTES
const _: () = assert!(MAX_SOURCE_BYTES == 1_048_576);
```

На стороне C заголовочный файл документирует каждое смещение и проверяет те же числа:

[`include/keyrill.h:173-192`](../../include/keyrill.h#L173-L192)

```c
/* A copied plan. expected is the exact text the owned composition must hold before the write,
 * empty when no word is owned; mark is the text to write. Units past each length are zero.
 *
 *   offset   0  plan_id       token for kr_commit; only the latest plan is accepted
 *            8  action        KR_PASS, KR_MARK, KR_FINISH or KR_UNAVAILABLE
 *           12  disposition   KR_KEY_PASS, KR_KEY_CONSUMED or KR_KEY_REPLAN
 *           16  expected_len
 *           20  mark_len
 *           24  expected      uint16_t[128]
 *          280  mark          uint16_t[128]                          536 bytes in all
 */
typedef struct {
  uint64_t plan_id;
  uint32_t action;
  uint32_t disposition;
  uint32_t expected_len;
  uint32_t mark_len;
  uint16_t expected[KR_TEXT_CAPACITY];
  uint16_t mark[KR_TEXT_CAPACITY];
} kr_plan;
```

[`include/keyrill.h:435-446`](../../include/keyrill.h#L435-L446)

```c
#ifdef __cplusplus
}
static_assert(sizeof(kr_event) == 32);
static_assert(sizeof(kr_plan) == 536);
static_assert(offsetof(kr_plan, expected) == 24);
static_assert(sizeof(kr_compatibility_plan) == 280);
#else
_Static_assert(sizeof(kr_event) == 32, "kr_event ABI");
_Static_assert(sizeof(kr_plan) == 536, "kr_plan ABI");
_Static_assert(offsetof(kr_plan, expected) == 24, "kr_plan text offset");
_Static_assert(sizeof(kr_compatibility_plan) == 280, "kr_compatibility_plan ABI");
#endif
```

Теперь изменение размера или сдвиг текстового массива вызывают ошибку компиляции на той стороне, где
произошло изменение, а не приводят к чтению плана по неверному смещению во время выполнения. Зачем
закреплять смещение поля `expected`, а не только размер структуры? Без `#[repr(C)]` Rust может
поместить массивы в начало, и структура всё равно займёт 536 байт; сдвиг обнаружит только проверка
смещения. Упражнения 5 и 6 показывают, какая проверка срабатывает в каждом из этих случаев.

Следует, однако, заметить, что проверки закрепляют не каждое поле: перестановка `action` и
`disposition` в одном из объявлений проходит обе проверки. Этот оставшийся риск закрывает дымовой
тест на C `crates/keyrill-ffi/tests/abi_smoke.c`, который прогоняет настоящие планы через структуру,
объявленную в заголовочном файле. Директива `#ifdef` выбирает запись, понятную каждому из языков:
`_Static_assert` из C11 или `static_assert` из C++. Скобка `}` в начале закрывает блок `extern "C"`,
о котором рассказано в
[главе об Objective-C++](objective-cpp.ru.md#%D1%81%D0%B2%D1%8F%D0%B7%D1%8B%D0%B2%D0%B0%D0%BD%D0%B8%D0%B5-c).

> [!TIP]
> **Запомните.** `#[repr(C)]` располагает поля структуры по правилам C, а проверки во время
> компиляции с обеих сторон закрепляют её размер и то смещение, которое сдвинулось бы при
> перестановке.

## Шаг 2: передать владение и забрать его обратно

*Цель: проследить один движок от создания в Rust до уничтожения по запросу из C.*

Коду на C нужен **дескриптор** (англ. handle) движка — значение, которое обозначает движок;
заглядывать внутрь движка код на C не должен. Программистам на Python эта идея знакома по файловым
дескрипторам: `os.open` возвращает целое число, обозначающее файл, и программа обязана закрыть его
ровно один раз. В заголовочном файле объявлен **непрозрачный тип** (англ. opaque type) — структура,
у которой есть имя, но нет определения:

[`include/keyrill.h:74`](../../include/keyrill.h#L74)

```c
typedef struct kr_engine kr_engine;
```

Код на C может хранить `kr_engine *` и передавать его обратно, но не может создать такой объект,
скопировать его или прочитать его поле, поскольку компилятору неизвестен его размер. На стороне Rust
этот указатель является [`Box<Engine>`](https://doc.rust-lang.org/std/boxed/struct.Box.html) —
владеющим указателем Rust на значение в куче, аналогом `std::unique_ptr` в C++:

[`crates/keyrill-ffi/src/lib.rs:330-366`](../../crates/keyrill-ffi/src/lib.rs#L330-L366)

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_engine_new(abi: u32, out: *mut *mut Engine) -> u32 {
    if out.is_null() {
        return NULL;
    }
    // SAFETY: `out` is non-null (checked above), and the caller guarantees it is aligned and
    // writable for one pointer
    unsafe {
        out.write(std::ptr::null_mut());
    }
    if !compatible_abi(abi) {
        return VERSION;
    }
    match catch_unwind(|| {
        Box::new(Engine {
            session: Session::default(),
            poisoned: false,
            automatic: Automatic::Off,
            dictionaries: [Snapshot::default(), Snapshot::default()],
            learned: Vec::new(),
            backend: Backend::Unselected,
            compatibility_boundary: false,
            compatibility_word_boundary: false,
            compatibility_collect: true,
            compatibility_pending: None,
        })
    }) {
        Ok(engine) => {
            // SAFETY: same output contract; allocation is transferred exactly once
            unsafe {
                out.write(Box::into_raw(engine));
            }
            OK
        }
        Err(_) => POISONED,
    }
}
```

Проследим порядок действий: сначала проверяется выходной указатель, затем он обнуляется, затем
сравнивается версия ABI, и только после этого выделяется память. Даже если вызывающий код не
проверит код состояния, он найдёт нулевой дескриптор, а не мусор из собственного стека. Функция
[`Box::into_raw`](https://doc.rust-lang.org/std/boxed/struct.Box.html#method.into_raw) отказывается
от владения и возвращает необработанный указатель, так что Rust движок не удалит; с этого момента
единственная ссылка на движок находится у кода на C. Обратный путь выполняет
[`Box::from_raw`](https://doc.rust-lang.org/std/boxed/struct.Box.html#method.from_raw): она забирает
владение обратно, после чего `drop` уничтожает движок и освобождает его память:

[`crates/keyrill-ffi/src/lib.rs:368-382`](../../crates/keyrill-ffi/src/lib.rs#L368-L382)

```rust
/// Free once, after every call on the engine has returned and no callback can reach it. Null is
/// a no-op.
///
/// # Safety
/// A non-null pointer must be the live allocation returned by `kr_engine_new`, with no aliases.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_engine_free(pointer: *mut Engine) {
    if !pointer.is_null() {
        // An unwind must not cross into C; a panic while dropping is discarded
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: caller transfers the original unique allocation exactly once
            drop(unsafe { Box::from_raw(pointer) });
        }));
    }
}
```

Раздел `# Safety` документирующего комментария содержит предусловие этой функции `unsafe fn`,
сформулированное для вызывающего кода: указатель, освобождённый дважды или освобождённый во время
выполнения другого вызова с ним, означает ошибку вызывающего кода.

Обеспечивается ли это условие? Ни компилятором, ни самой функцией: она не может отличить живой
указатель от освобождённого. Такие ошибки обнаруживают при тестировании два инструмента — на тех
путях, которые проходит тест. Команда `./tools/verify sanitize` запускает нативные тесты под
[AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer.html), который сообщает о двойном
освобождении или об использовании после освобождения в момент, когда оно происходит; команда
`./tools/verify miri` запускает тесты указателей Rust под
[Miri](https://github.com/rust-lang/miri) — интерпретатором, обнаруживающим неопределённое
поведение. В поставляемой программе гарантией служит владение: каждый движок принадлежит ровно
одному объекту Objective-C — `KRSession` или, в режиме совместимости, `KRCompatibilitySession`, —
который освобождает движок один раз в своём деструкторе, как показано в следующей главе. Других
освобождений нет, за исключением движков, которые не покидали создавшую их функцию: при неудачной
настройке в `KRController.mm` и у пробного движка для проверки в `KRRuntime.mm`.

> [!TIP]
> **Запомните.** `Box::into_raw` передаёт выделенную память коду на C, а `Box::from_raw` забирает её
> обратно ровно для одного освобождения. Двойное освобождение является ошибкой вызывающего кода;
> тесты её обнаруживают, компилятор — не может.

## Шаг 3: остановить панику на границе

*Цель: понять, почему паника Rust никогда не должна доходить до C и как это предотвращает одна
функция.*

Паника **раскручивает** (англ. unwinds) стек: она покидает функции одну за другой, вызывая
деструкторы их локальных значений, пока что-нибудь её не перехватит, — примерно так же, как
исключение в Python, JavaScript и C++. Раскручиваться в кадры C или Objective-C, которым ничего не
известно о раскрутке в Rust, она не должна. Начиная с версии
[Rust 1.81](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/), паника, дошедшая до функции
`extern "C"`, **аварийно завершает** (англ. aborts) **процесс** (англ. process), то есть работающую
программу, — немедленно и без завершающих действий; до версии 1.81 это было неопределённым
поведением. Аварийно завершившийся метод ввода уносит с собой набор пользователя. Поэтому каждая
экспортируемая функция, работающая с движком, проходит через одну функцию — самые поучительные
двадцать строк крейта:

[`crates/keyrill-ffi/src/lib.rs:292-316`](../../crates/keyrill-ffi/src/lib.rs#L292-L316)

```rust
// Callers provide live, uniquely borrowed handles. No callback is made while this borrow
// exists; every export releases it before returning to native code. A panic in `f` poisons the
// handle, which also drops the word, history and pending plans it held. AssertUnwindSafe is
// acceptable because nothing relies on state a panic may have left inconsistent: the handle is
// only reset, and every later call returns POISONED before touching it.
unsafe fn with_engine(pointer: *mut Engine, f: impl FnOnce(&mut Engine) -> u32) -> u32 {
    if pointer.is_null() {
        return NULL;
    }
    // SAFETY: non-null was checked above; the caller contract requires a live aligned handle from
    // kr_engine_new, not yet freed, with no other reference to it alive during this call
    let engine = unsafe { &mut *pointer };
    if engine.poisoned {
        return POISONED;
    }
    match catch_unwind(AssertUnwindSafe(|| f(engine))) {
        Ok(result) => result,
        Err(_) => {
            engine.poisoned = true;
            engine.session.reset();
            engine.compatibility_pending = None;
            POISONED
        }
    }
}
```

Здесь последовательно происходят три действия. Сначала выполняется проверка на нуль, поскольку код
на C может передать что угодно. Затем необработанный указатель превращается в `&mut Engine`; это
единственный шаг, требующий `unsafe`: начиная с него анализатор заимствований считает движок
исключительно заимствованным, а комментарий `SAFETY` объясняет, почему это верно. Наконец, работа
оборачивается в [`catch_unwind`](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html), который
превращает панику в `Err`. Говоря языком Python, каждая экспортируемая функция обёрнута в
`try`/`except` и превращает любое исключение в код ошибки.

Перехваченная паника **отравляет** (англ. poisons) дескриптор навсегда. Паника могла оставить сеанс
обновлённым наполовину, и ни один последующий вызов не должен исходить из такого состояния; поэтому
дескриптор сбрасывается, а каждый следующий вызов возвращает `KR_POISONED`, ничего не трогая.
Обёртка [`AssertUnwindSafe`](https://doc.rust-lang.org/std/panic/struct.AssertUnwindSafe.html)
является явным утверждением, что это допустимо: по умолчанию `catch_unwind` отказывается принимать
замыкание, удерживающее `&mut` во время паники, — именно из-за возможного наполовину обновлённого
состояния, — а комментарий объясняет, почему здесь на такое состояние ничто не опирается.

Посмотрим ещё раз на параметр-замыкание `f: impl FnOnce(&mut Engine) -> u32`. Функция `with_engine`
одалживает движок замыканию `f` и получает его обратно; заимствование заканчивается, когда `f`
возвращает управление, и функция сразу же возвращается в нативный код. Следовательно, пока
выполняется нативный код, ни одна ссылка Rust на движок существовать не может, а поскольку Rust
никогда не вызывает адаптер в обратную сторону, другого пути для такой ссылки нет. Тем самым правило
«never hold Rust borrows across native callbacks» из `AGENTS.md` соблюдается самой конструкцией
программы.

Перехват паники зависит от одной настройки сборки:

[`Cargo.toml:20-22`](../../Cargo.toml#L20-L22)

```toml
[profile.release]
# The C ABI catches a panic and poisons the handle; with abort the panic would end the input server
panic = "unwind"
```

При настройке [`panic = "abort"`](https://doc.rust-lang.org/cargo/reference/profiles.html#panic)
раскрутки, которую можно было бы перехватить, нет, и у `catch_unwind` не было бы никакой возможности
сработать.

> [!TIP]
> **Запомните.** Паника не должна переходить в код на C. Функция `with_engine` перехватывает её,
> отравляет дескриптор и возвращает вместо неё код ошибки.

## Шаг 4: анатомия экспортируемой функции

*Цель: прочитать функцию `extern "C"` строка за строкой.*

[`crates/keyrill-ffi/src/lib.rs:602-627`](../../crates/keyrill-ffi/src/lib.rs#L602-L627)

```rust
/// Acknowledge the latest plan; refused and unknown outcomes invalidate the session until
/// [`kr_reset`]. A stale token changes nothing.
///
/// # Safety
/// `pointer` must meet the live unique serialized handle contract, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_commit(pointer: *mut Engine, token: u64, outcome: u32) -> u32 {
    // SAFETY: forwarded handle contract
    unsafe {
        with_engine(pointer, |engine| {
            let outcome = match outcome {
                0 => Outcome::Verified,
                1 => Outcome::Refused,
                2 => Outcome::Unknown,
                _ => return BAD_TAG,
            };
            if engine.backend == Backend::Compatibility {
                return STATE;
            }
            engine
                .session
                .acknowledge(token, outcome)
                .map_or_else(status, |()| OK)
        })
    }
}
```

- [`extern "C"`](https://doc.rust-lang.org/reference/items/functions.html#extern-function-qualifier)
  выбирает соглашение о вызовах C.
- `#[unsafe(no_mangle)]` сохраняет имя символа в точности как `kr_commit`. Без этого атрибута
  компилятор **декорировал бы** (англ. mangle) имя, закодировав в символе крейт, модуль и хеш, чтобы
  одинаковые имена в разных местах программы оставались различными
  ([декорирование имён](https://en.wikipedia.org/wiki/Name_mangling)), и компоновщик не смог бы
  сопоставить символ с объявлением в заголовочном файле. Редакция 2024 помечает сам этот атрибут как
  [`unsafe`](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-attributes.html), поскольку
  недекорированное имя может совпасть с другим символом в итоговой программе.
- Целое число, пришедшее из C, становится перечислением только через `match` с универсальной ветвью,
  которая возвращает `BAD_TAG`. В Rust нет приведения целого числа к перечислению, а `transmute` —
  переосмысление битов — создал бы из случайной `3` значение `Outcome`, не соответствующее ни одному
  варианту, что является неопределённым поведением; значение должно быть отвергнуто прежде, чем оно
  станет перечислением.
- `return BAD_TAG` внутри замыкания выполняет возврат из замыкания, и это значение становится
  результатом `with_engine`.
- `map_or_else(status, |()| OK)` превращает возвращённый сеансом `Result<(), Error>` в код
  состояния: функция `status`, переданная по имени, преобразует ошибку, а замыкание — успех.

Планы передаются в обратную сторону по значению. Функция `kr_plan_event` преобразует событие C в
собственный тип Rust `Event` и записывает полную структуру `NativePlan` в память, принадлежащую
вызывающему коду:

[`crates/keyrill-ffi/src/lib.rs:228-255`](../../crates/keyrill-ffi/src/lib.rs#L228-L255)

```rust
fn decode_event(event: &NativeEvent) -> Result<(Event, Mode, Context), u32> {
    if event.len > 8 {
        return Err(BOUNDS);
    }
    let mode = match event.mode {
        1 => Mode::En,
        2 => Mode::Ru,
        _ => return Err(BAD_TAG),
    };
    let context = match event.context {
        0 => Context::Boundary,
        1 => Context::Ineligible,
        _ => return Err(BAD_TAG),
    };
    if event.role != 1 && event.len != 0 {
        return Err(BAD_TAG);
    }
    let input = match event.role {
        1 => Event::Text(event.units[..event.len as usize].to_vec()),
        2 => Event::Backspace,
        3 => Event::Escape,
        4 => Event::Manual,
        5 => Event::Reverse,
        6 => Event::Finish,
        _ => return Err(BAD_TAG),
    };
    Ok((input, mode, context))
}
```

[`crates/keyrill-ffi/src/lib.rs:593-597`](../../crates/keyrill-ffi/src/lib.rs#L593-L597)

```rust
            // Conversion maps unit for unit, so both texts are at most a MAX_WORD word plus Space
            copied.expected[..plan.expected.len()].copy_from_slice(&plan.expected);
            copied.mark[..plan.text.len()].copy_from_slice(&plan.text);
            out.write(copied);
            OK
```

Длина проверяется прежде, чем берётся срез `event.units[..event.len as usize]`, хотя выход среза за
пределы вызвал бы всего лишь панику, которая была бы перехвачена: код состояния является лучшим
ответом на неверные входные данные, чем отравленный дескриптор. Метод `to_vec()` копирует единицы в
вектор, принадлежащий сеансу, так что ничто в Rust не хранит указатель на структуру вызывающего
кода. На выходе векторы плана копируются в массивы фиксированного размера структуры `NativePlan`,
остаток массивов остаётся нулевым, а `out.write` записывает всю структуру за один раз. Ни одна
область памяти, выделенная в Rust, не пересекает интерфейс ни в одном направлении, и адаптер не
освобождает ничего, кроме дескрипторов.

> [!TIP]
> **Запомните.** Экспортируемая функция использует соглашение о вызовах C и недекорированное имя,
> проверяет каждое целое число, прежде чем оно станет перечислением, и копирует данные через
> границу, а не разделяет их.

## Потоки и блокировки

*Цель: научиться читать `Mutex`, атомарный флаг и порядок захвата блокировок и понимать, что
предотвращает каждый из них.*

Движок используется из одного потока: заголовочный файл требует, чтобы вызовы одного движка
выполнялись по одному из потока, которому принадлежит его контекст ввода, и `&mut` в `with_engine`
выражает именно это требование. С файлами словарей дело обстоит иначе. Чтение файла выполняется
медленно и не должно происходить в потоке, обрабатывающем клавиши. Поэтому второй дескриптор,
`kr_vocabulary`, разделяется между фоновой очередью, которая читает файлы, и главным потоком — тем,
в котором macOS доставляет ввод и который передаёт словари движкам.

Если два потока обращаются к одной памяти без согласования и хотя бы один из них выполняет запись,
возникает **гонка данных** (англ. data race) — неопределённое поведение в Rust и C++. Глобальная
блокировка интерпретатора Python скрывает часть таких ситуаций, но не все, а JavaScript избегает их,
запуская по одному потоку на цикл событий. Устройство модуля изложено в его документации:

[`crates/keyrill-ffi/src/vocabulary_host.rs:1-19`](../../crates/keyrill-ffi/src/vocabulary_host.rs#L1-L19)

````rust
//! `kr_vocabulary`: dictionary sources that engines adopt, without scheduling or native editing
//! authority.
//!
//! A vocabulary handle holds six vocabulary sources, the English and Russian Vim spell files and
//! four learned-word slots, each with a configured path, flags, a generation and a current
//! snapshot. Two locks keep the slow path away from the controller thread:
//!
//! ```text
//!   acquisition  one Source per slot; held across file I/O by kr_vocabulary_observe
//!   published    path, flags, generation and snapshot per slot; held briefly by configure,
//!                adopt, and the start and end of observe
//! ```
//!
//! Observe takes `acquisition` before `published` and nothing takes them in the other order, so
//! configure and adopt never wait for a file read. A configuration change advances the slot's
//! generation; an observation that began under the old generation finds the change when it
//! locks `published` again and publishes nothing (`STALE_CONFIGURATION`). Adoption clones every
//! snapshot under one lock, so an engine never pairs a language from before a change with one
//! from after it.
````

**Блокировка** (англ. lock) пропускает в защищаемый ею код только один поток за раз. Если два потока
захватывают две блокировки в разном порядке, каждый из них может бесконечно ждать блокировку,
захваченную другим; такая ситуация называется
[взаимной блокировкой](https://en.wikipedia.org/wiki/Deadlock_%28computer_science%29) (англ.
deadlock). Фиксированный порядок захвата — в данном случае `acquisition` перед `published` —
исключает её. Структура данных располагает каждую блокировку точно вокруг того, что она защищает:

[`crates/keyrill-ffi/src/vocabulary_host.rs:72-79`](../../crates/keyrill-ffi/src/vocabulary_host.rs#L72-L79)

```rust
/// `kr_vocabulary`: six vocabulary sources behind two locks, shared by the acquisition queue and
/// the controller thread. Free only after every operation has completed.
#[derive(Default)]
pub struct VocabularyHandle {
    poisoned: AtomicBool,
    acquisition: Mutex<Acquisition>,
    published: Mutex<[Language; SOURCES]>,
}
```

[`Mutex<T>`](https://doc.rust-lang.org/std/sync/struct.Mutex.html) в Rust *содержит* данные, которые
охраняет. Единственный путь к `Acquisition` — `lock()`, который возвращает умный указатель
**`MutexGuard`**, дающий доступ к данным и снимающий блокировку при удалении, так что забыть взять
блокировку невозможно.
[`threading.Lock`](https://docs.python.org/3/library/threading.html#lock-objects) в Python в блоке
`with` снимается так же, но сам по себе ничего не защищает: любой код может тронуть данные, не беря
его.

Rust отслеживает, какими типами можно пользоваться из нескольких потоков, с помощью двух типажей,
которые компилятор выводит самостоятельно:
[`Send` и `Sync`](https://doc.rust-lang.org/nomicon/send-and-sync.html). Именно `Mutex` и
`AtomicBool` делают общую ссылку `&VocabularyHandle` безопасной для одновременного использования из
двух потоков. Необработанный указатель, пришедший из C, эту проверку обходит, поэтому функция,
превращающая его в ссылку, излагает обоснование сама:

[`crates/keyrill-ffi/src/vocabulary_host.rs:212-234`](../../crates/keyrill-ffi/src/vocabulary_host.rs#L212-L234)

```rust
// Shared references are synchronized internally; no native calls occur under either lock.
// Any POISONED result, a caught panic or a lock poisoned by one, poisons the handle for good.
unsafe fn with_vocabulary(
    pointer: *const VocabularyHandle,
    operation: impl FnOnce(&VocabularyHandle) -> u32,
) -> u32 {
    if pointer.is_null() {
        return NULL;
    }
    // SAFETY: non-null was checked above; the caller contract requires the live aligned
    // allocation from kr_vocabulary_new, not yet freed and kept alive for this call. Only shared
    // references are made, and every mutation goes through a Mutex or the atomic flag, so
    // concurrent calls on the handle are allowed.
    let vocabulary = unsafe { &*pointer };
    if vocabulary.poisoned.load(Ordering::Acquire) {
        return POISONED;
    }
    let status = catch_unwind(AssertUnwindSafe(|| operation(vocabulary))).unwrap_or(POISONED);
    if status == POISONED {
        vocabulary.poisoned.store(true, Ordering::Release);
    }
    status
}
```

Сравним её с `with_engine`. Эта функция создаёт `&*pointer`, то есть общую ссылку, тогда как
`with_engine` создавала `&mut *pointer`; общие ссылки могут одновременно удерживать два потока, а
тип не позволяет делать через них ничего, кроме работы с блокировками и атомарных операций.
**Атомарной** (англ. atomic) называется переменная, чтение и запись которой неделимы даже при
обращении из разных потоков; флаг отравления является такой переменной с упорядочением
[`Acquire` и `Release`](https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html) — правилами,
определяющими, какие другие записи поток гарантированно увидит. Эти правила совпадают с
`std::memory_order` в C++, а в JavaScript они доступны через
[`Atomics`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Atomics)
для общей памяти между воркерами.

У **мьютекса** (англ. mutex, от *mutual exclusion* — «взаимное исключение»; так называется вид
блокировки) в Rust есть собственный механизм отравления: если поток паникует, удерживая
`MutexGuard`, последующие вызовы `lock()` возвращают ошибку, а не выдают данные, которые могут
оказаться несогласованными
([отравление](https://doc.rust-lang.org/std/sync/struct.Mutex.html#poisoning)). Код превращает и эту
ошибку в `KR_POISONED`:

[`crates/keyrill-ffi/src/vocabulary_host.rs:133-160`](../../crates/keyrill-ffi/src/vocabulary_host.rs#L133-L160)

```rust
        let Ok(mut languages) = self.published.lock() else {
            return POISONED;
        };
        let language = &mut languages[index];
        if language.path == path
            && language.missing_is_empty == missing_is_empty
            && language.excluded == excluded
        {
            return OK;
        }
        let Some(generation) = language.generation.checked_add(1) else {
            return STATE;
        };
        let retired = std::mem::replace(
            language,
            Language {
                generation,
                path,
                missing_is_empty,
                excluded,
                current: None,
            },
        );
        // Free the old snapshot after unlocking, so a large vocabulary never holds the lock
        drop(languages);
        drop(retired);
        OK
    }
```

[`std::mem::replace`](https://doc.rust-lang.org/std/mem/fn.replace.html) ставит на место новое
значение и отдаёт старое, что сохраняет старый снимок живым после присваивания. Затем порядок двух
вызовов `drop` делает то, что описывает комментарий: сначала `MutexGuard`, который снимает
блокировку, потом вытесненное значение, так что освобождение большого списка слов происходит вне
блокировки. В C++ то же намерение требует аккуратно расставленной области видимости; здесь
уничтожение — это вызов функции, который можно поставить, куда нужно.

Поколение — то же средство, которым сторона Objective-C++ защищается от повторного входа, но
применённое к потокам: наблюдение запоминает поколение, при котором оно началось, и ничего не
публикует, если конфигурация изменилась, пока читался файл. С этим средством, под именем
`_generation`, читатель снова встретится в главе
[«Реентерабельность и тесты»](reentrancy-and-tests.ru.md).

> [!TIP]
> **Запомните.** `Mutex` в Rust владеет защищаемыми данными, поэтому обойти его невозможно.
> Блокировки следует захватывать в одном фиксированном порядке, а большие значения освобождать после
> снятия блокировки.
