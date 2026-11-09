<!-- translation of docs/learn/rust.md at 9129818b21d9ff9422241b9cfb474987b0e8b8d0 -->

# Rust в ядре

Крейт `keyrill-core` принимает решения. Он написан на безопасном Rust и не имеет зависимостей, кроме
**стандартной библиотеки** (англ. standard library) — библиотеки, которую каждая программа на Rust
получает вместе с компилятором; о macOS, указателях (необработанных адресах памяти) и документах он
ничего не знает. Поэтому изучать язык удобнее всего именно здесь: каждое понятие, о котором пойдёт
речь ниже, проверяется компилятором, и для каждого будет показано место, где его нарушение
остановило бы сборку. Полным справочником служит книга [The Rust Programming Language] (далее просто
«книга»); ссылки ведут на её главы, где каждое понятие разобрано подробно.

## Перед первым фрагментом: редакции и проверки

*Цель: понять, что означает «Rust 2024» и что делает компилятор до запуска программы.*

Язык Rust развивается небольшими шагами, а изменения, которые нарушили бы работу старого кода,
собираются в **редакции** (англ. edition): Rust 2015, 2018, 2021 и 2024. Каждый крейт указывает свою
редакцию в файле `Cargo.toml` — манифесте, который читает Cargo, инструмент сборки Rust. В Keyrill
значение `edition = "2024"` задано для всего **рабочего пространства** (англ. workspace), то есть
набора крейтов, собираемых вместе из корневого `Cargo.toml`. Крейты разных редакций свободно
компонуются друг с другом; следовательно, редакция есть выбор диалекта для отдельного крейта, а не
ответвление языка ([Edition Guide](https://doc.rust-lang.org/edition-guide/editions/index.html)).
Когда в учебнике сказано «Rust 2024 требует…», имеется в виду правило этого диалекта.

Многое из того, о чём пойдёт речь дальше, происходит во **время компиляции** (англ. compile time),
когда компилятор переводит программу, а не во **время выполнения** (англ. run time), когда программа
работает. В Python и JavaScript большинство ошибок обнаруживается во время выполнения, при
исполнении ошибочной строки; C++ часть ошибок находит во время компиляции. Rust переносит на время
компиляции всё, что возможно, и рассматриваемый крейт построен так, чтобы этим воспользоваться: если
нарушено какое-либо правило из этой главы, программа не соберётся.

> [!TIP]
> **Запомните.** Редакция — это диалект Rust, выбираемый для каждого крейта; Keyrill использует
> редакцию 2024. Проверки, описанные в этой главе, выполняются во время компиляции, до запуска
> программы.

## Типы, которые несут свой смысл

*Цель: научиться читать `enum` с полями и разбирающий его `match`.*

**Структура** (англ. struct) — запись с именованными **полями** (англ. fields), подобная `struct` в
C++, классу данных (dataclass) в Python или объекту JavaScript с фиксированным набором свойств.
**Перечисление** (англ. enum) в Rust представляет собой выбор между несколькими **вариантами**
(англ. variants), причём у каждого варианта могут быть собственные поля, существующие только тогда,
когда значение является этим вариантом. Такая конструкция в общем случае называется
[**размеченным объединением**](https://en.wikipedia.org/wiki/Tagged_union) (англ. tagged union): в
C++ ей соответствует [`std::variant`](https://en.cppreference.com/w/cpp/utility/variant), в
TypeScript —
[размеченное объединение типов](https://www.typescriptlang.org/docs/handbook/2/narrowing.html#discriminated-unions),
а в Python и обычном JavaScript она существует лишь как соглашение — объект с полем `kind`.
Рассмотрим состояние набираемого слова:

[`crates/keyrill-core/src/session.rs:164-182`](../../crates/keyrill-core/src/session.rs#L164-L182)

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    /// Composing while `boundary` is clear, BoundaryRetained once the Space has joined the mark.
    Owned {
        /// What the owned mark holds, including the Space once `boundary` is set.
        text: Vec<u16>,
        /// The receipt: the text before the word was first converted; `None` if it was never
        /// converted, or after an edit or a Reverse.
        original: Option<Vec<u16>>,
        boundary: bool,
        /// Set by Manual or Reverse; Space then keeps the word without inference.
        explicit: bool,
        /// Input mode the word was started in.
        mode: Mode,
    },
    Invalidated,
}
```

Обратим внимание на то, чего здесь написать нельзя. В состоянии `Idle` нет поля `text`, потому что в
этом состоянии такого поля не существует. Класс с отдельными членами `state` и `text` допускал бы
бессмысленное сочетание «простой, но с текстом» и полагался бы на то, что ни один программист его не
создаст; здесь такое сочетание исключено самим типом.

Первая строка является **атрибутом** (англ. attribute) — пометкой для компилятора, которая
записывается в виде `#[...]`. Атрибут
[`derive`](https://doc.rust-lang.org/reference/attributes/derive.html) поручает компилятору
сгенерировать код для перечисленных **типажей** (англ. traits). Типаж — это именованный набор
методов, который может предоставить тип; по смыслу он находится между концептом C++, протоколом
Python и интерфейсом TypeScript. Типаж `Clone` даёт копирование, `Debug` — печать для программиста,
`PartialEq` — оператор `==`, `Default` — значение по умолчанию. Пометка
[`#[default]`](https://doc.rust-lang.org/std/default/trait.Default.html#enums) указывает вариант,
который возвращает `Default`; благодаря ей `Session::default()` начинает работу в состоянии `Idle`
без всякого конструктора.

**`Option<T>`** — перечисление стандартной библиотеки с двумя вариантами: `Some(T)` и `None`. Во
всём безопасном Rust оно заменяет нулевой указатель, `None` в Python и `null` в JavaScript, но с
одним существенным отличием: компилятор не позволит воспользоваться находящимся внутри значением,
пока программист не укажет, что делать, если значения нет. Поле `original` выше содержит либо
квитанцию, либо ничего, и код, в котором случай «ничего» забыт, не компилируется.

Небольшие перечисления заменяют целочисленные коды даже там, где число было бы короче. План
сообщает, что делать с клавишей, адаптер сообщает, что произошло, и для каждого из этих сообщений
заведён свой тип:

[`crates/keyrill-core/src/session.rs:121-141`](../../crates/keyrill-core/src/session.rs#L121-L141)

```rust
/// What happens to the key event once the plan's action is verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Let the host handle the event once.
    Pass,
    /// The event is handled; the host does not see it.
    Consumed,
    /// Finish was acknowledged; plan the same event again from Idle.
    Replan,
}

/// Native effect acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Native effect and readback verified, or the plan had no native effect.
    Verified,
    /// Rejected before any native write.
    Refused,
    /// Write attempted but outcome cannot be established.
    Unknown,
}
```

Оба значения можно было бы передавать целыми числами, но тогда функции можно было бы по ошибке
передать решение о клавише там, где ожидался исход. При двух различных типах такая ошибка не
компилируется. Типаж `Copy` в списке `derive` означает, что эти значения — просто биты, которые
копируются при присваивании, как `int` в C; что происходит с остальными значениями, объясняется в
следующем разделе.

### `match` ведёт счёт

*Цель: понять, почему новый вариант нарушает сборку и почему это полезно.*

[`match`](https://doc.rust-lang.org/book/ch06-02-match.html) сравнивает значение с **образцами**
(patterns), по одному на **ветвь** (arm), и выполняет первую подходящую ветвь. Он обязан охватить
каждый вариант, поэтому добавление варианта превращает каждый неполный `match` в ошибку компиляции,
даже в другом крейте. В Python есть
[оператор `match`](https://docs.python.org/3/tutorial/controlflow.html#match-statements) с похожим
синтаксисом, но он не проверяет полноту; TypeScript умеет это с
[проверкой исчерпывающести](https://www.typescriptlang.org/docs/handbook/2/narrowing.html#exhaustiveness-checking),
которую вы пишете сами. Вот место, где граница C превращает перечисления Rust обратно в числа из
`keyrill.h`:

[`crates/keyrill-ffi/src/lib.rs:575-592`](../../crates/keyrill-ffi/src/lib.rs#L575-L592)

```rust
            let mut copied = NativePlan {
                plan_id: plan.id,
                action: match plan.action {
                    Action::Pass => 0,
                    Action::Mark => 1,
                    Action::Finish => 2,
                    Action::Unavailable => 3,
                },
                disposition: match plan.disposition {
                    Disposition::Pass => 0,
                    Disposition::Consumed => 1,
                    Disposition::Replan => 2,
                },
                expected_len: plan.expected.len() as u32,
                mark_len: plan.text.len() as u32,
                expected: [0; 128],
                mark: [0; 128],
            };
```

Конструкция `match` является выражением, то есть имеет значение, и поэтому может стоять внутри
литерала структуры. Ветви `default:` здесь нет, и в этом весь смысл. Пятое действие, добавленное в
ядро, не сможет незаметно пройти через границу с C: этот файл перестанет компилироваться, пока
кто-нибудь не решит, какой номер присвоить новому действию. Эта ошибка показана в упражнении 2.

Образец может заглядывать внутрь варианта и дополняться условием — так называемым **условием
сопоставления** (англ. match guard), которое записывается после `if`. Следующий образец распознаёт
клавишу, начинающую или продолжающую слово:

[`crates/keyrill-core/src/session.rs:449-460`](../../crates/keyrill-core/src/session.rs#L449-L460)

```rust
        // In English mode the keys that type Russian letters can start or continue a word
        let letter = match event {
            Event::Text(units)
                if units.len() == 1
                    && (mapping::is_letter(units[0])
                        || (mode == Mode::En
                            && inference::is_provisional(units[0], self.keyboard))) =>
            {
                Some(units[0])
            }
            _ => None,
        };
```

Образец `Event::Text(units)` подходит только к текстовому варианту и даёт его содержимому имя
`units`; затем условие сопоставления требует, чтобы была ровно одна единица и чтобы она была буквой
либо, в английском режиме, клавишей вроде `[`, которая в русской раскладке печатает букву. Образец
`_` подходит ко всему остальному. Результатом является `Option<u16>`: буква или ничего.

Образцами могут быть и диапазоны, а макрос
[`matches!`](https://doc.rust-lang.org/std/macro.matches.html) превращает образец в значение типа
`bool`. Знаком `!` отмечается **макрос** (англ. macro) — код, который компилятор разворачивает в
другой код перед компиляцией; `matches!`, `assert!` и встречающийся ниже `include_bytes!` являются
макросами. Множество букв, с которым работает весь движок, задаётся одной строкой:

[`crates/keyrill-core/src/mapping.rs:42-46`](../../crates/keyrill-core/src/mapping.rs#L42-L46)

```rust
/// Whether a unit is an EN/RU letter: `A`-`Z`, `a`-`z`, `А`-`я`, `Ё` or `ё`. [`Mapping::Manual`],
/// the word session and language context detection all use this letter set.
pub fn is_letter(unit: u16) -> bool {
    matches!(unit, 0x41..=0x5a | 0x61..=0x7a | 0x410..=0x44f | 0x401 | 0x451)
}
```

Тип `u16` — беззнаковое 16-разрядное целое, и текст повсюду хранится как последовательность таких
чисел, **единиц кода UTF-16** (англ. UTF-16 code units). UTF-16 — кодировка, которую используют
macOS и Windows; большинство символов занимает в ней одну единицу, некоторые — две. Возможность
использовать одно и то же смещение по обе стороны границы с C здесь важнее, чем удобство строки
`String` в кодировке UTF-8.

> [!TIP]
> **Запомните.** Поля перечисления существуют только в своём варианте, поэтому невозможное состояние
> построить нельзя. Конструкция `match` обязана охватывать все варианты, поэтому новый вариант
> невозможно забыть.

## Владение и заимствование

*Цель: научиться определять по сигнатуре метода, может ли он изменить свой объект.*

В Python и JavaScript память освобождает **сборщик мусора** (англ. garbage collector): он находит
объекты, на которые никто не ссылается, и через некоторое время освобождает их. В C++ эта забота
лежит на программисте и деструкторах. В Rust нет ни сборщика мусора, ни ручного вызова `free`. У
каждого значения есть ровно один **владелец** (англ. owner) — переменная или поле, — и значение
уничтожается, или, как говорят, **удаляется** (англ. dropped), когда его владелец выходит из области
видимости.

Остальной код получает доступ к значению путём
[заимствования](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html) (англ.
borrowing): он берёт **ссылку** (англ. reference) на значение, не становясь его владельцем. Ссылки
бывают двух видов, и **анализатор заимствований** (англ. borrow checker) компилятора следит за
соблюдением одного правила: допускается сколько угодно общих ссылок `&T` или ровно одна
исключительная ссылка `&mut T`, но не то и другое одновременно. В Python и JavaScript ничего
подобного нет: любая функция, получившая список, может его изменить. Правило это короткое, но на нём
проект строит свою главную гарантию. Сравним первый параметр двух методов, определённых в одном
блоке `impl` (так называется блок, в котором определяются методы типа):

[`crates/keyrill-core/src/session.rs:438-446`](../../crates/keyrill-core/src/session.rs#L438-L446)

```rust
    /// The transition table: the plan's action, disposition and text, and the proposed state.
    /// Reads the confirmed state and changes nothing.
    fn transition(
        &self,
        event: &Event,
        mode: Mode,
        context: Context,
        automatic: Automatic<'_>,
    ) -> (Action, Disposition, Vec<u16>, State) {
```

Первый параметр метода, `self`, обозначает объект, у которого вызван метод, подобно `self` в Python
и `this` в JavaScript. Метод `transition` принимает `&self`, то есть общее заимствование сеанса, и
поэтому **не может** изменить сеанс. В комментарии сказано «changes nothing», и на этот раз верить
комментарию на слово не нужно: это обеспечивает компилятор, что и показывает упражнение 1.
Следовательно, вся таблица переходов, занимающая более 150 строк, является чистой функцией
подтверждённого состояния и события. Вызывающий метод принимает `&mut self` и сохраняет результат
как предложение, не затрагивая подтверждённого поля `state`:

[`crates/keyrill-core/src/session.rs:363`](../../crates/keyrill-core/src/session.rs#L363)

```rust
        let (action, disposition, text, next) = self.transition(event, mode, context, automatic);
```

[`crates/keyrill-core/src/session.rs:381-393`](../../crates/keyrill-core/src/session.rs#L381-L393)

```rust
        self.pending = Some(Pending {
            id,
            state: next,
            history,
            mode,
        });
        Ok(Plan {
            id,
            action,
            disposition,
            expected,
            text,
        })
```

Запись `state: next` **перемещает** (англ. moves) предложенное состояние в `pending`. После этой
строки пользоваться `next` уже нельзя, и при попытке это сделать компилятор сообщит об ошибке.
Перемещение — это то, что делает присваивание со значением любого типа, не являющегося `Copy`:
значение переходит к новому владельцу, и его буферы в **куче** уходят вместе с ним без копирования.
Кучей (англ. heap) называется память для данных, размер или время жизни которых заранее неизвестны,
например для содержимого растущего `Vec`; в **стеке** (англ. stack) хранятся локальные переменные
функции, и он освобождается при возврате из функции. В Python присваивание `b = a` создаёт второе
имя для того же объекта, и оба имени остаются пригодными; в Rust `let b = a;` отдаёт значение, и
переменная `a` становится непригодной. Ближе всего к этому `std::move` в C++, однако там переменная,
из которой выполнено перемещение, остаётся «допустимой, но в неопределённом состоянии», а не
непригодной.

Поскольку `transition` лишь заимствует сеанс, взять из него текущий текст для построения следующего
состояния он не может. Вместо этого он делает копию и явно указывает на это в коде:

[`crates/keyrill-core/src/session.rs:498-501`](../../crates/keyrill-core/src/session.rs#L498-L501)

```rust
                let mut text = text.clone();
                let mut original = original.clone();
                let mut boundary = *boundary;
                let mut explicit = *explicit;
```

Метод `clone()` выполняет глубокое копирование, как `copy.deepcopy` в Python или `structuredClone` в
JavaScript, и в Rust он всегда пишется явно; ничего не копируется незаметно для программиста, как
это может сделать конструктор копирования C++. Выражение `*boundary` читает значение `bool`, на
которое ссылается `boundary`. Именно копии обеспечивают следующее свойство: план `Plan`,
возвращаемый адаптеру, владеет своим текстом и ничего не заимствует у сеанса. Заимствующий план
держал бы сеанс заблокированным, пока план находится у адаптера, а адаптер должен иметь возможность
сбросить сеанс посреди обращения к приложению.

### Забрать значение ровно один раз

*Цель: увидеть, как значение, которое разрешено использовать один раз, становится невозможно
использовать дважды.*

Подтверждение — это момент, когда предложение становится действительностью, и для каждого плана оно
должно происходить не более одного раза. Метод
[`Option::take`](https://doc.rust-lang.org/std/option/enum.Option.html#method.take) перемещает
значение из `Option`, оставляя на его месте `None`:

[`crates/keyrill-core/src/session.rs:405-410`](../../crates/keyrill-core/src/session.rs#L405-L410)

```rust
    pub fn acknowledge(&mut self, id: u64, outcome: Outcome) -> Result<(), Error> {
        if self.pending.as_ref().map(|p| p.id) != Some(id) {
            return Err(Error::Stale);
        }
        let pending = self.pending.take().ok_or(Error::Stale)?;
        if outcome == Outcome::Verified {
```

[`crates/keyrill-core/src/session.rs:428-436`](../../crates/keyrill-core/src/session.rs#L428-L436)

```rust
            self.mode = Some(pending.mode);
            self.state = pending.state;
        } else {
            self.state = State::Invalidated;
            self.history.clear();
            self.mode = None;
        }
        Ok(())
    }
```

Прочтите первую проверку дважды. Выражение `self.pending.as_ref()` превращает ссылку на
`Option<Pending>` в `Option`, содержащий ссылку, так что **замыкание** (англ. closure) `|p| p.id` —
анонимная функция наподобие `lambda p: p.id` в Python или `p => p.id` в JavaScript — может прочитать
идентификатор, не забирая предложения. Результат сравнивается с `Some(id)`. Только после этого
`take()` перемещает предложение наружу. Повторное подтверждение с тем же идентификатором находит
`None` и возвращает `Stale`. Флаг «уже использовано» этому коду не нужен, поскольку самого значения
больше нет.

> [!TIP]
> **Запомните.** Через `&self` можно читать, но нельзя писать; через `&mut self` можно и то и
> другое, но такая ссылка может существовать только одна. Присваивание перемещает значение, если его
> тип не является `Copy`.

## Времена жизни

*Цель: научиться читать `'a` в типе и объяснять, от чего это защищает.*

Ссылка никогда не должна переживать значение, на которое она указывает. Язык со сборщиком мусора
гарантирует это, сохраняя значение живым, пока на него хоть что-то ссылается; Rust вместо этого
отказывается компилировать программу, в которой ссылка могла бы пережить своё значение. В
большинстве случаев компилятор справляется с этим сам. Если же тип *хранит* ссылку, нужно дать имя
тому промежутку времени, в течение которого ссылка остаётся действительной. Это имя называется
[**временем жизни**](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html) (англ. lifetime) и
записывается с апострофом: `'a`. Время жизни не порождает никакого кода: это ограничение, которое
компилятор разрешает и затем забывает.

Удобным первым примером служит модель слов, потому что в ней вообще ничего не копируется. Каждое её
поле является **срезом** (англ. slice) `&'a [u8]` — видом на часть одного массива байтов, состоящим
из указателя и длины. [`memoryview`](https://docs.python.org/3/library/stdtypes.html#memoryview) в
Python и
[`subarray`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/TypedArray/subarray)
в JavaScript представляют собой виды того же рода, но с той разницей, что они продлевают жизнь
нижележащего буфера, а срез Rust — нет:

[`crates/keyrill-core/src/model.rs:92-107`](../../crates/keyrill-core/src/model.rs#L92-L107)

```rust
/// One language's frequency table and character chain, borrowed from a validated asset.
#[derive(Clone, Copy)]
pub struct LanguageModel<'a> {
    alphabet: &'a [u8],
    offsets: &'a [u8],
    words: &'a [u8],
    frequencies: &'a [u8],
    chain: &'a [u8],
}

/// Validated EN and RU models.
#[derive(Clone, Copy)]
pub struct Model<'a> {
    en: LanguageModel<'a>,
    ru: LanguageModel<'a>,
}
```

Запись `Model<'a>` читается так: «модель, которая не может пережить байты, из которых она
разобрана». В C++ аналогичная конструкция представляла бы собой структуру из членов
[`std::span`](https://en.cppreference.com/w/cpp/container/span) с комментарием, предупреждающим о
висячих указателях. Здесь программа, в которой байты уничтожаются раньше модели, не компилируется.
Копирование всей `Model` сводится к копированию десяти срезов, то есть двадцати машинных слов и ни
одного байта данных, поэтому этому типу можно дать `Copy`.

Разборщик позволяет увидеть более тонкое обстоятельство. Структура `Reader` хранит ещё не
прочитанные байты, а метод `take` выдаёт следующий кусок:

[`crates/keyrill-core/src/model.rs:109-122`](../../crates/keyrill-core/src/model.rs#L109-L122)

```rust
/// Cursor over the asset; a read past the end is [`ModelError::Truncated`].
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ModelError> {
        if length > self.bytes.len() {
            return Err(ModelError::Truncated);
        }
        let (head, tail) = self.bytes.split_at(length);
        self.bytes = tail;
        Ok(head)
    }
```

Метод `take` заимствует читателя исключительно (`&mut self`), однако возвращаемый срез имеет время
жизни `'a`, привязанное к файлу модели, а не к читателю. Поэтому вызывающий код может хранить срез и
после того, как читатель продвинулся дальше; именно так `Model::parse` сохраняет все разделы, не
копируя ни одного байта. Если бы возвращаемый тип был записан как `&[u8]`, без имени времени жизни,
компилятор связал бы его с заимствованием читателя, и читатель оставался бы заблокированным, пока
жив хотя бы один раздел. Одна пометка указывает, какой из двух вариантов имеется в виду.

Сам файл модели встраивается в программу макросом
[`include_bytes!`](https://doc.rust-lang.org/std/macro.include_bytes.html) и поэтому существует
столько же, сколько программа; именно это и выражает особое время жизни `'static`:

[`crates/keyrill-core/src/model.rs:32`](../../crates/keyrill-core/src/model.rs#L32)

```rust
static EMBEDDED: &[u8] = include_bytes!("../../../assets/model.bin");
```

[`crates/keyrill-core/src/model.rs:327-329`](../../crates/keyrill-core/src/model.rs#L327-L329)

```rust
    pub fn embedded() -> Result<Model<'static>, ModelError> {
        Model::parse(EMBEDDED)
    }
```

Тип `Result<..., ModelError>` является возвращаемым типом функции, которая может завершиться
неудачей; ему посвящён раздел об ошибках ниже.

### Тип без времени жизни не может хранить заимствование

*Цель: научиться понимать отсутствие времени жизни как обещание.*

С помощью времён жизни тип может обещать и то, чего он *не* хранит. Модель и словари пользователя
нужны автоматическому исправлению на время одного вызова, поэтому у перечисления, которое их
передаёт, время жизни есть:

[`crates/keyrill-core/src/session.rs:184-188`](../../crates/keyrill-core/src/session.rs#L184-L188)

```rust
/// Automatic correction at Space: the fixture policy, on or off, or the model.
enum Automatic<'a> {
    Fixture(bool),
    Model(&'a Model<'a>, Dictionaries<'a>, Policy),
}
```

У предложения, ожидающего подтверждения, времени жизни нет:

[`crates/keyrill-core/src/session.rs:197-203`](../../crates/keyrill-core/src/session.rs#L197-L203)

```rust
/// The proposed state of the latest plan, adopted only by a verified acknowledgement.
struct Pending {
    id: u64,
    state: State,
    history: HistoryEffect,
    mode: Mode,
}
```

У типа `Pending` нет параметра времени жизни, следовательно, ни одно его поле не может быть ссылкой
с ограниченным временем жизни. В документации `propose_with_inference` сказано, что «the pending
operation retains no model, vocabulary reference or native callback» (**обратный вызов**, англ.
callback, — функция, переданная другому коду, чтобы тот вызвал её позднее), и определение типа это
доказывает. Между планом и его подтверждением адаптер обращается к чужому приложению, которое может
сбросить сеанс или заменить словари; ничто в сеансе не может указывать на прежние.

> [!TIP]
> **Запомните.** Время жизни обозначает промежуток, в течение которого заимствованная ссылка
> остаётся действительной. Тип без времени жизни не может хранить заимствованную ссылку, и за этим
> следит компилятор.

## Общие данные, которые никогда не меняются

*Цель: научиться разделять одно значение между читателями без копирования и без блокировок.*

Личные словари изменяются во время работы Keyrill: пользователь добавляет слово в Vim, а у Keyrill
остаётся прежний список. Читатель, начавший работу с прежним списком, должен видеть его
согласованным, а испорченный файл не должен заменять исправный. Средством для этого служит
[`Arc`](https://doc.rust-lang.org/std/sync/struct.Arc.html) — указатель с **атомарным подсчётом
ссылок** (англ. atomically reference-counted). Он ведёт счёт владельцам, разделяющим значение,
освобождает значение, когда счётчик обращается в нуль, и безопасно обновляет счётчик из нескольких
**потоков** (англ. threads), то есть независимых последовательностей выполнения внутри одной
работающей программы. В C++ ему соответствует `std::shared_ptr<const T>`. CPython точно так же
подсчитывает ссылки на каждый объект и, кроме того, имеет сборщик циклических ссылок, которого в
Rust нет. Значение внутри `Arc` никогда не изменяется:

[`crates/keyrill-core/src/vocabulary.rs:374-403`](../../crates/keyrill-core/src/vocabulary.rs#L374-L403)

```rust
/// Atomically replaceable immutable vocabulary, initially missing rather than silently empty.
#[derive(Default)]
pub struct Snapshot {
    current: Option<Arc<Vocabulary>>,
}

impl From<Option<Arc<Vocabulary>>> for Snapshot {
    fn from(current: Option<Arc<Vocabulary>>) -> Self {
        Self { current }
    }
}

impl Snapshot {
    /// Clone the current immutable snapshot; absence keeps automatic policy unready.
    pub fn current(&self) -> Option<Arc<Vocabulary>> {
        self.current.clone()
    }

    /// Parse `source` as a Vim word list and publish it whole; a candidate with skipped lines is
    /// still published. Readers that cloned the previous snapshot keep it unchanged.
    ///
    /// # Errors
    ///
    /// The candidate's [`ParseError`]; the current snapshot is left unchanged.
    pub fn replace(&mut self, source: &[u8]) -> Result<(), ParseError> {
        let candidate = Vocabulary::parse(source)?;
        self.current = Some(Arc::new(candidate));
        Ok(())
    }
}
```

Клонирование `Arc` копирует указатель и увеличивает счётчик на единицу; сам словарь при этом
остаётся общим. Публикация заменяет указатель, а не содержимое, так что читатель, у которого есть
прежний `Arc`, продолжает пользоваться прежним списком, пока не отпустит его. Обратите внимание на
порядок действий внутри `replace`: при ошибке разбора `?` покидает функцию *до* присваивания,
поэтому плохой файл не может испортить хороший снимок. В комментарии сказано «initially missing
rather than silently empty», и тип `Option` позволяет выразить это различие: пользователь, у
которого ещё нет словаря, — не то же самое, что пользователь с пустым словарём.

[`From`](https://doc.rust-lang.org/std/convert/trait.From.html) — стандартный типаж для
преобразований. Его реализация позволяет границе C позже написать `current.map(Snapshot::from)`; в
Python это был бы конструктор, принимающий другой тип.

На время одного предложения движок получает из снимков обычные ссылки:

[`crates/keyrill-ffi/src/lib.rs:271-287`](../../crates/keyrill-ffi/src/lib.rs#L271-L287)

```rust
    match &engine.automatic {
        Automatic::Model(model) => {
            let en = engine.dictionaries[0].current();
            let ru = engine.dictionaries[1].current();
            engine.session.propose_with_inference(
                input,
                mode,
                context,
                model,
                Dictionaries {
                    en: en.as_deref(),
                    ru: ru.as_deref(),
                    learned: &engine.learned,
                },
                Policy::default(),
            )
        }
```

Переменная `en` имеет тип `Option<Arc<Vocabulary>>`: это владелец со счётчиком, хранящийся в
локальной переменной на всё время вызова. Метод `as_deref()` заимствует значение через него и даёт
`Option<&Vocabulary>`, который и сохраняется в `Dictionaries`. Анализатор заимствований принимает
такой код только потому, что `en` живёт дольше вызова, использующего ссылку. Если построить
`Dictionaries` в операторе `let` непосредственно из `engine.dictionaries[0].current().as_deref()`,
то `Arc` окажется временным значением, которое удаляется в конце этого оператора, и компилятор
отвергнет висячую ссылку с сообщением `error[E0716]: temporary value dropped while borrowed`.

> [!TIP]
> **Запомните.** `Arc` позволяет разделять одно значение, которое никогда не изменяется. Обновление
> выполняется публикацией нового `Arc`, а читатели продолжают пользоваться прежним.

## Ошибки как значения

*Цель: научиться читать `Result`, `?` и `let ... else` и понимать, когда уместна паника.*

В Python и JavaScript о большинстве сбоев сообщают исключения, которые поднимаются по стеку, пока их
кто-нибудь не перехватит, и которые не видны в сигнатуре функции. В Rust для сбоев, ожидаемых
программой, исключений нет. Функция, которая может не справиться с задачей, возвращает
[`Result<T, E>`](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html) —
перечисление из вариантов `Ok(T)` и `Err(E)`; таким образом, сбой становится частью возвращаемого
типа, и вызывающий код обязан его обработать.
[Оператор `?`](https://doc.rust-lang.org/reference/expressions/operator-expr.html#the-try-propagation-expression)
означает «при сбое верни ошибку вызывающей функции» — то же, что делает неперехваченное исключение,
но записанное явно в каждом месте, где сбой возможен. Оператор работает и с `Option`, где сбоем
считается `None`. Разборщик модели использует оба варианта:

[`crates/keyrill-core/src/model.rs:128-145`](../../crates/keyrill-core/src/model.rs#L128-L145)

```rust
    fn u32(&mut self) -> Result<u32, ModelError> {
        u32_at(self.take(4)?, 0).ok_or(ModelError::Truncated)
    }
}

fn u16_at(bytes: &[u8], index: usize) -> Option<u16> {
    let at = index.checked_mul(2)?;
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(bytes: &[u8], index: usize) -> Option<u32> {
    let at = index.checked_mul(4)?;
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
```

Функция `u16_at` может не справиться с задачей четырьмя способами, и на каждый приходится по одному
`?`: переполнение при умножении, переполнение при сложении, выход диапазона за пределы среза (метод
`get` возвращает `None`, а не завершается аварийно, как завершилось бы `bytes[at]`) и неподходящая
длина куска для двухбайтового массива. Файл модели поступает из сборки, и тем не менее каждое чтение
проверяется, поскольку тот же разборщик обязан безопасно работать на испорченном файле; именно это
проверяет **фаззер** (англ. fuzzer) проекта — инструмент, подающий программе сгенерированные и
испорченные входные данные.

### `let ... else` и арифметика, которая не может переполниться

*Цель: научиться обрабатывать маловероятный случай, который нельзя пропустить.*

[`let ... else`](https://doc.rust-lang.org/reference/statements.html#let-statements) связывает
образец, если он подходит, а иначе выполняет блок `else`, который обязан покинуть функцию. Читайте
это как «иди по счастливому пути или уходи». Здесь эта конструкция охраняет счётчик, нумерующий
планы:

[`crates/keyrill-core/src/session.rs:350-357`](../../crates/keyrill-core/src/session.rs#L350-L357)

```rust
        // Exhaustion is permanent: reset keeps the counter, so every later proposal fails too
        let Some(id) = self.serial.checked_add(1) else {
            self.state = State::Invalidated;
            self.pending = None;
            self.history.clear();
            self.mode = None;
            return Err(Error::Invalidated);
        };
```

Простое `self.serial += 1` вызвало бы **панику** (англ. panic), то есть аварийную остановку, в
**отладочной сборке** (англ. debug build) — неоптимизированной сборке для разработки — и перешло бы
через нуль в **сборке выпуска** (англ. release build) — оптимизированной сборке, которая
поставляется пользователям. Целые числа Python никогда не переполняются, а числа JavaScript без
всякого сообщения теряют точность, так что ни тот, ни другой язык к этому не подготавливает. Метод
[`checked_add`](https://doc.rust-lang.org/std/primitive.u64.html#method.checked_add) возвращает
`Option`, и переполнение становится случаем, который код обязан обработать. Произойти оно может
только после 2^64 планов. Защищаемое правило таково: просроченный идентификатор плана никогда не
должен совпасть с идентификатором более позднего плана, и система типов не позволяет пропустить этот
случай лишь потому, что он маловероятен.

### Тип ошибки, которому нечего выдать

*Цель: научиться читать тип ошибки и его типажи.*

[`crates/keyrill-core/src/session.rs:143-162`](../../crates/keyrill-core/src/session.rs#L143-L162)

```rust
/// Session protocol errors; no raw text is included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The id is not the pending plan: a later proposal superseded it, it was already
    /// acknowledged, or a reset expired it.
    Stale,
    /// Session invalidated or token counter exhausted.
    Invalidated,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Stale => "plan is not the latest proposal",
            Self::Invalidated => "session is invalidated",
        })
    }
}

impl std::error::Error for Error {}
```

Тип ошибки — обычное перечисление. Реализация типажа
[`Display`](https://doc.rust-lang.org/std/fmt/trait.Display.html) даёт ему текстовое представление
для человека, подобно `__str__` в Python и `toString` в JavaScript, а пустая реализация
`impl std::error::Error` включает его в стандартное семейство ошибок. В комментарии сказано «no raw
text is included», и у перечисления действительно нет поля, в котором мог бы храниться такой текст:
ошибку этого крейта можно записать в журнал, не рискуя записать то, что набрал пользователь.

> [!TIP]
> **Запомните.** Сбой — это значение типа `Result` или `Option`, и оператор `?` передаёт его выше.
> Паника предназначена для ошибок в программе, а не для ожидаемых сбоев.

## Итераторы и замыкания

*Цель: научиться читать цепочку методов итератора и функцию, принимающую замыкание.*

[**Итератор**](https://doc.rust-lang.org/book/ch13-02-iterators.html) (англ. iterator) выдаёт
значения по одному, а его методы (`map`, `zip`, `chain`, `filter`) строят конвейер, который ничего
не делает, пока у него не попросят значения. Так же устроены генераторы и модуль
[`itertools`](https://docs.python.org/3/library/itertools.html) в Python и
[вспомогательные методы итераторов](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Iterator)
в JavaScript; метод `Array.prototype.map` в JavaScript устроен иначе: он сразу строит новый массив
целиком. Компилятор Rust превращает такой конвейер в обычный цикл. Всё преобразование раскладки
представляет собой один такой конвейер:

[`crates/keyrill-core/src/mapping.rs:60-88`](../../crates/keyrill-core/src/mapping.rs#L60-L88)

```rust
pub fn convert(text: &[u16], keyboard: Keyboard, mapping: Mapping) -> Vec<u16> {
    text.iter()
        .map(|&unit| convert_unit(unit, keyboard, mapping))
        .collect()
}

fn convert_unit(unit: u16, keyboard: Keyboard, mapping: Mapping) -> u16 {
    if mapping == Mapping::Manual && !is_letter(unit) {
        return unit;
    }
    let extra = match keyboard {
        Keyboard::Apple => ('\\', '|'),
        Keyboard::Pc => ('`', '~'),
    };
    for (en, ru) in EN
        .chars()
        .zip(RU.chars())
        .chain(EN_SHIFT.chars().zip(RU_SHIFT.chars()))
        .chain([(extra.0, 'ё'), (extra.1, 'Ё')])
    {
        if mapping != Mapping::RuToEn && unit == en as u16 {
            return ru as u16;
        }
        if mapping != Mapping::EnToRu && unit == ru as u16 {
            return en as u16;
        }
    }
    unit
}
```

Запись `|&unit|` — замыкание, образец параметра которого читает `u16` через ссылку, выдаваемую
итератором. Метод `collect()` строит коллекцию того типа, который указан как возвращаемый, в данном
случае `Vec<u16>`. В функции `convert_unit` метод `zip` проходит параллельно по двум строкам
подписей клавиш, а `chain` добавляет ряд с Shift и клавишу буквы ё, так что цикл обходит 66 пар без
индекса и без таблицы. Линейный поиск выбран сознательно: 66 сравнений на клавишу ничтожны по
сравнению с обращением к приложению, а пары остаются читаемыми как ряды клавиатуры.

Методы итераторов заменяют арифметику индексов там, где ошибка на единицу обошлась бы дорого.
История слов хранит не более 256 единиц и должна начинаться с целого слова:

[`crates/keyrill-core/src/session.rs:414-426`](../../crates/keyrill-core/src/session.rs#L414-L426)

```rust
                HistoryEffect::Append(text) => {
                    self.history.extend(text);
                    // Trim at a Space so the window starts at a whole word; this can drop one
                    // word more than the bound requires
                    if self.history.len() > MAX_HISTORY {
                        let minimum = self.history.len() - MAX_HISTORY;
                        let remove = self.history[minimum..]
                            .iter()
                            .position(|&unit| unit == 32)
                            .map_or(self.history.len(), |at| minimum + at + 1);
                        self.history.drain(..remove);
                    }
                }
```

Метод `position` находит первый пробел в месте обрезки или после него; `map_or` превращает результат
в точку удаления, а если пробела нет — во всю длину истории; `drain(..remove)` удаляет префикс на
месте. Образец `HistoryEffect::Append(text)` перемещает добавляемый текст из перечисления, так что
`extend` поглощает его без копирования.

### Замыкание как параметр

*Цель: увидеть внедрение зависимости через замыкание и значение, вычисляемое один раз за время
работы процесса.*

Функция может принимать замыкание в качестве параметра, так же как функция Python может принимать
вызываемый объект. Запись `impl FnOnce() -> ...` в списке параметров означает «любая функция или
любое замыкание, которое можно вызвать один раз и которое возвращает указанный тип»:

[`crates/keyrill-ffi/src/lib.rs:178-189`](../../crates/keyrill-ffi/src/lib.rs#L178-L189)

```rust
// Validated once per process; every engine that selects MODEL shares the result, a failure too
fn embedded_model() -> Result<Model<'static>, ModelError> {
    static MODEL: OnceLock<Result<Model<'static>, ModelError>> = OnceLock::new();
    *MODEL.get_or_init(Model::embedded)
}

// The model loader is a parameter so a test can inject an asset failure
fn set_policy(
    engine: &mut Engine,
    policy: u32,
    model: impl FnOnce() -> Result<Model<'static>, ModelError>,
) -> u32 {
```

Рабочий код передаёт `embedded_model`; тест передаёт замыкание, возвращающее ошибку, и тем самым
доказывает, что при испорченном файле модели политика остаётся выключенной. Такой приём называется
**внедрением зависимости** (англ. dependency injection): функции передают нужные ей вспомогательные
средства, вместо того чтобы она сама их получала, причём здесь это делается без интерфейса и без
виртуального вызова. Запись `impl FnOnce` обозначает **обобщённый** (англ. generic) код: компилятор
создаёт отдельную копию `set_policy` для каждого типа замыкания, с которым её вызывают, подобно
шаблону C++, поэтому во время выполнения вызов ничего не стоит. Сторона Objective-C делает то же
самое с помощью блоков, как показано в
[главе об Objective-C](objective-c.ru.md#%D0%B1%D0%BB%D0%BE%D0%BA%D0%B8).

[`OnceLock`](https://doc.rust-lang.org/std/sync/struct.OnceLock.html) — ячейка, которая заполняется
не больше одного раза, безопасно даже тогда, когда несколько потоков спрашивают одновременно.
Ближайший родственник —
[`functools.cache`](https://docs.python.org/3/library/functools.html#functools.cache) в Python на
функции без аргументов, только он при соперничестве потоков может вызвать функцию дважды, а
`OnceLock` никогда. Здесь он проверяет модель размером 5,6 МБ один раз за процесс, сколько бы
движков её ни запросили.

> [!TIP]
> **Запомните.** Цепочки итераторов ленивы и компилируются в обычные циклы. Параметр-замыкание
> позволяет тесту подменить зависимость кода на Rust.

## Модули, документация и линты

*Цель: научиться читать корень крейта и знать, какие проверки обеспечивает сборка.*

Корень крейта, файл `lib.rs`, невелик, и его последние строки содержат обещание, которое выполняет
компилятор:

[`crates/keyrill-core/src/lib.rs:55-61`](../../crates/keyrill-core/src/lib.rs#L55-L61)

```rust
#![forbid(unsafe_code)]

pub mod inference;
pub mod mapping;
pub mod model;
pub mod session;
pub mod vocabulary;
```

Запись `#![...]`, со знаком `!`, применяет атрибут ко всему крейту. **Линтом** (англ. lint)
называется проверка на подозрительную конструкцию, о которой сообщает компилятор; у каждого линта
есть уровень: `allow`, `warn`, `deny` или `forbid`
([уровни линтов](https://doc.rust-lang.org/rustc/lints/levels.html)). Уровень `forbid` самый
строгий: в отличие от `deny`, его нельзя ослабить ниже по коду, как `# noqa` ослабляет линтер Python
или `eslint-disable` — ESLint. Поэтому в этом крейте нет кода `unsafe` и нет способа незаметно его
добавить (упражнения 3 и 4); что такое `unsafe`, объясняется в следующей главе.

Объявление `pub mod` вводит **модуль** (англ. module) — пространство имён, исходный текст которого
находится в файле с тем же именем, — и делает его публичным. Элементы модуля являются закрытыми,
если они не помечены `pub`, и это обеспечивает компилятор; начальное подчёркивание в Python — не
более чем соглашение. Поэтому типы `State` и `Pending`, рассмотренные выше, не видны из других
крейтов, а `Session` и `Plan` видны.

Линты всего рабочего пространства собраны в одном месте:

[`Cargo.toml:12-18`](../../Cargo.toml#L12-L18)

```toml
[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "deny"
missing_docs = "warn"

[workspace.lints.clippy]
missing_errors_doc = "warn"
missing_panics_doc = "warn"
```

Команда `./tools/verify` запускает [Clippy](https://doc.rust-lang.org/clippy/), дополнительный
линтер Rust, рассматривая предупреждения как ошибки, так что и эти предупреждения нарушают сборку.
Именно из-за `missing_errors_doc` у каждой публичной функции, возвращающей `Result`, есть раздел
`# Errors`, как у рассмотренного выше `Snapshot::replace`. Документирующие комментарии (`///`) могут
содержать примеры, и `cargo test` компилирует и выполняет их как тесты, подобно
[`doctest`](https://docs.python.org/3/library/doctest.html) в Python. Пример в начале `lib.rs`
набирает `ghbdtn` и пробел через полный сеанс; это самый короткий законченный пример использования
движка в репозитории.

> [!TIP]
> **Запомните.** Уровень `forbid` нельзя ослабить ниже по коду. Элементы по умолчанию закрыты, а
> примеры в документации выполняются как тесты.

[the rust programming language]: https://doc.rust-lang.org/book/
