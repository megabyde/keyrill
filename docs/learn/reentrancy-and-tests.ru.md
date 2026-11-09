<!-- translation of docs/learn/reentrancy-and-tests.md at 888c4e0a2a80005124a39e566d3589d286fce091 -->

# Реентерабельность и тесты

Предыдущие главы были посвящены языкам. Эта глава посвящена самой трудной идее адаптера, которую не
решает никакое средство языка: вызову, который ждёт ответа и позволяет окружающему миру измениться,
пока он ждёт. Затем мы проследим одно слово через весь рассмотренный код, а в конце познакомимся с
двумя приёмами тестирования, которые проверяют, что эта конструкция действительно работает так, как
заявлено.

## Вызов, который выполняет ваш код

*Цель: объяснить, каким образом однопоточная программа может прервать сама себя.*

Однопоточная программа кажется безопасной: пока работает ваша функция, ничто другое не выполняется.
Адаптер Keyrill однопоточен — вся работа с вводом идёт в главном потоке, — и тем не менее в этом
смысле он не безопасен.

Сообщение вроде `[client markedRange]` является межпроцессным взаимодействием с приложением, и, пока
главный поток ждёт ответа, он может запустить **вложенный цикл выполнения** (англ. nested run loop):
цикл событий повторно входит в работу изнутри ожидающего вызова и доставляет другие события.
Деактивация, смена режима или другая клавиша могут прийти *внутри* вызова, и их обратный вызов
выполняется в вашем потоке, на вашем объекте, в то время как ваша функция всё ещё находится на стеке
и ждёт. Это явление называется
[**реентерабельностью**](https://en.wikipedia.org/wiki/Reentrancy_%28computing%29) (англ.
reentrancy, иначе повторная входимость): в код вошли повторно прежде, чем завершился первый вход.

Программистам на JavaScript и Python такая картина знакома по асинхронному коду. Во время
[`await`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Operators/await)
выполняются другие обратные вызовы, которые могут изменить любое состояние, прочитанное до него;
разница в том, что здесь вызов выглядит синхронным, и ничто в коде не отмечает это место. Блокировка
не помогает: блокировка, захваченная внешним вызовом, навсегда остановила бы внутренний вызов в том
же потоке. Не помогают и правила заимствования Rust, поскольку обращение к приложению происходит
целиком вне Rust.

В заголовочном файле сеанса документирован полный сценарий одного исправления с указанием места
каждой проверки. Прочтите его медленно: каждая строка с пометкой «recheck generation» охраняет одну
точку, в которой приложение могло снова выполнить код адаптера:

[`native/macos/src/KRSession.h:48-75`](../../native/macos/src/KRSession.h#L48-L75)

```objc
/// Main thread only. A client call is synchronous IPC and can spin a nested run loop, so any of
/// them can reenter the controller with a deactivation, a mode change or another key. A
/// reentrant callback bumps the generation, resets Rust and drops local authority; the
/// interrupted operation sees the new generation at its next recheck and refuses before the
/// write or reports an unknown outcome after it, and never writes again. Automatic correction
/// of "ghbdtn" at Space, with the mark at [s, s+6) and the caret at s+6:
///
///   kr_plan_event          MARK, expected "ghbdtn", mark "привет " a proposal only
///   markedRange            == [s, s+6)                             recheck generation
///   selectedRange          == {s+6, 0}                             recheck generation
///   Secure Input           off
///   stringFromRange        == "ghbdtn", actual range == [s, s+6)   recheck generation
///   Secure Input           off
///   -- local authority dropped: from here the outcome is verified or unknown --
///   setMarkedText          "привет ", selection at its end         the plan's one write
///   markedRange            == [s, s+7)                             recheck generation
///   selectedRange          == {s+7, 0}                             recheck generation
///   Secure Input           off
///   stringFromRange        == "привет ", actual range == [s, s+7)  readback
///   kr_commit              APPLIED_VERIFIED: Rust advances, the new mark is owned
///
/// A failed check before the write acknowledges REFUSED_BEFORE_WRITE: nothing was written and
/// the key passes. A failed check after it acknowledges ATTEMPTED_UNKNOWN: correction stops,
/// and a key the write carried is consumed, never replayed. When that check failed on the
/// client's answer rather than on reentrancy, an exception or Secure Input, the session commits
/// the client's own exact readback of its marked range once, if both are still available, so
/// that the next key can't replace it; that is the one write allowed after an unknown outcome,
/// and it repairs nothing.
```

> [!TIP]
> **Запомните.** Обращение к приложению может выполнить ваши собственные обратные вызовы прежде, чем
> вернёт управление. После такого обращения всё, что было известно до него, могло устареть.

## Поколение и флаг занятости

*Цель: научиться читать два средства, которые обнаруживают повторный вход.*

Защита состоит из двух частей. Первая — счётчик, называемый **поколением** (англ. generation).
Каждое событие, которое делает недействительными сведения сеанса, увеличивает `_generation` на
единицу, лишает сеанс полномочий над его текстом и сбрасывает движок Rust:

[`native/macos/src/KRSession.mm:120-132`](../../native/macos/src/KRSession.mm#L120-L132)

```obj-c++
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
```

Выполняемая операция запоминает поколение в начале и передаёт его аргументом; после **каждого**
обращения к приложению она сравнивает запомненное значение с текущим. Расхождение означает, что
внутри обращения выполнился какой-то обратный вызов и всё, что операция знала до него,
недействительно. В программировании пользовательских интерфейсов та же схема применяется против
устаревших ответов сети: каждый запрос нумеруется, и ответ, номер которого уже не последний,
игнорируется.

[`native/macos/src/KRSession.mm:401-408`](../../native/macos/src/KRSession.mm#L401-L408)

```obj-c++
  NSRange expected = NSMakeRange(start, text.length);
  NSRange marked = [client markedRange];
  if (generation != _generation)
    return NO;
  if (!NSEqualRanges(marked, expected)) {
    _inputStatus = KRInputMarkMismatch;
    return NO;
  }
```

Порядок двух проверок существен. Поколение проверяется первым: после обратного вызова с повторным
входом значение `marked` описывает документ, над которым у сеанса больше нет полномочий, и сравнение
его с ожидаемым диапазоном отвечало бы на вопрос, утративший смысл.

Вторая часть защиты — флаг, по которому обратный вызов с повторным входом распознаёт сам себя. Флаг
`_busy` установлен, пока выполняется план, и клавиша, пришедшая при установленном флаге, могла
прийти только через вложенный цикл выполнения:

[`native/macos/src/KRSession.mm:199-205`](../../native/macos/src/KRSession.mm#L199-L205)

```obj-c++
  // A key arriving while a plan is in flight came through a nested run loop inside a client
  // call. Invalidating here is what makes the outer operation's next recheck fail
  if (_busy) {
    _inputStatus = KRInputInterrupted;
    [self invalidate];
    return reserved;
  }
```

Вложенный вызов не пытается ни завершить работу внешнего вызова, ни выполнить свою. Он делает
состояние недействительным и возвращает управление, а внешняя операция, получив управление обратно
от приложения, обнаруживает новое поколение и прекращает работу. Незавершённая работа отменяется и
никогда не объединяется с новой.

Сравнение `_generation` — то же средство, что и поколение ячейки в
[дескрипторе словарей](boundary.ru.md#%D0%BF%D0%BE%D1%82%D0%BE%D0%BA%D0%B8-%D0%B8-%D0%B1%D0%BB%D0%BE%D0%BA%D0%B8%D1%80%D0%BE%D0%B2%D0%BA%D0%B8).
Там оно защищает от другого потока, здесь — от того же потока, вошедшего повторно.

> [!TIP]
> **Запомните.** Поколение запоминается перед вызовом и сравнивается после него; при любом
> расхождении работа прекращается. Обратный вызов, обнаруживший установленный `_busy`, делает
> состояние недействительным и возвращает управление.

## Два вида сбоя

*Цель: научиться отличать «отказ до записи» от «неизвестного исхода после записи» и видеть, где код
проводит эту границу.*

Неудачная проверка означает разное до записи и после неё. До записи с документом ничего не
произошло: исход — «отказ до записи», движку сообщают об этом, и клавиша передаётся приложению так,
как если бы Keyrill не было. После начала записи добросовестный ответ может быть только один из
двух: «проверено» или «неизвестно». Приложение могло применить текст целиком, частично или не
применить вовсе, а неудачное **обратное чтение** (англ. readback) — чтение текста для сравнения с
записанным — не позволяет установить, что именно произошло.

Адаптер проводит эту границу в коде: он отказывается от своих полномочий *до* записи, так что ни
один путь после неё не может этими полномочиями воспользоваться:

[`native/macos/src/KRSession.mm:495-524`](../../native/macos/src/KRSession.mm#L495-L524)

```obj-c++
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
```

Полномочия восстанавливаются ровно в одном месте: после совпадения обратного чтения поля `_owner`,
`_start` и `_text` получают проверенные значения. Все остальные пути выхода из этого блока оставляют
их равными `nil`. Возвращаемое значение метода роли не играет: `setMarkedText:` ничего не
возвращает, а если бы и возвращал `YES`, это было бы лишь утверждением приложения, а не
доказательством состояния документа. Доказательством служит только обратное чтение документа.

> [!TIP]
> **Запомните.** До записи сбой означает отказ, после записи — неизвестность. Полномочия
> восстанавливает только совпавшее обратное чтение.

## План, применение, подтверждение

*Цель: прочитать главный цикл адаптера для одной клавиши.*

Ядро цикла адаптера достаточно коротко, чтобы его можно было запомнить. Оно выполняется один раз на
клавишу и дважды, если завершённое слово требуется подтвердить прежде, чем та же клавиша начнёт
следующее слово:

[`native/macos/src/KRSession.mm:257-288`](../../native/macos/src/KRSession.mm#L257-L288)

```obj-c++
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
      // Only a check after the write that failed on the client's answer, with the generation
      // unchanged, earns the one readback commit; after reentrancy, a client exception or Secure
      // Input the client gets no further call
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
```

`kr_plan plan;` — структура размером 536 байт в стеке, которую заполняет Rust; `&plan` передаёт её
адрес, а `apply:` получает её по ссылке. Действие, не определённое заголовочным файлом, отклоняется,
а не угадывается. Неизвестный исход прекращает исправление: сеанс делает состояние недействительным,
помечает себя остановленным и ничего не повторяет, поскольку повтор был бы второй записью поверх
результата, которого никто не видел. Единственное исключение изложено в комментарии и в приведённом
выше заголовочном файле: если обратное чтение не прошло проверку на собственном ответе приложения,
сеанс один раз фиксирует в точности то, что приложение сообщает как свой размеченный текст, чтобы
следующая клавиша, набранная пользователем, не могла его заменить.

> [!TIP]
> **Запомните.** План, применение, подтверждение — один раз на клавишу. Неизвестный исход прекращает
> исправление и никогда не повторяется.

## Одно слово от начала до конца

*Цель: проследить `ghbdtn` и пробел через все слои программы.*

Перечислим по порядку все этапы обработки слова `ghbdtn` и пробела, набранных в TextEdit в
английском режиме.

1. **Контроллер.** Метод `-[KeyrillAlphaController handleEvent:client:]` проверяет Secure Input,
   среду выполнения и уровень приложения, открывает `CallbackScope` и строит `kr_event` с помощью
   функции `KREventForKey`.
1. **Вход в сеанс.** Метод `-[KRSession handleEvent:client:]` удерживает себя, проверяет флаг
   `_busy`, устанавливает его и, если слово ещё не начато, запрашивает у приложения символ перед
   курсором.
1. **ABI.** Функция `kr_plan_event` преобразует структуру C в `Event` внутри `with_engine`, под
   защитой `catch_unwind`.
1. **Решение.** Метод `propose_using` выполняет `transition` над подтверждённым состоянием; на
   пробеле вывод может преобразовать слово. Предложенное состояние ожидает в `pending`.
1. **Проверка и запись.** Метод `apply:` проверяет разметку, курсор и текст, сверяя поколение после
   каждого вызова, отказывается от полномочий, выполняет единственную запись и читает её обратно.
1. **Подтверждение.** Вызов `kr_commit` доходит до `acknowledge`, `take()` перемещает предложение, и
   предложенное состояние становится подтверждённым.

| Клавиша | Действие | Ожидается в разметке | Записано в разметку | Клавиша после |
| -- | -- | -- | -- | -- |
| `g` | `MARK` | пусто | `g` | поглощена |
| `h` ... `n` | `MARK` | `g` ... `ghbdt` | `gh` ... `ghbdtn` | поглощена |
| Пробел | `MARK` | `ghbdtn` | `привет ` | поглощена |
| следующая буква | `FINISH`, затем `MARK` | `привет ` | фиксирует, затем начинает новое слово | планируется заново |
| Control-Option-Z до этого | `MARK` | `привет ` | `ghbdtn ` | поглощена |

Слово остаётся размеченным, то есть подчёркнутым как текст, который метод ввода ещё составляет, и
после пробела; в этом и состоит секрет отмены: пока текст является композицией, принадлежащей
Keyrill, его замена — обычная правка собственной разметки. Зафиксированный текст на этом пути
Keyrill никогда не переписывает ([ADR 005](../adr/005-no-committed-text-rewrite.ru.md)).

Если включено переключение раскладки (по умолчанию оно включено), преобразованное слово, кроме того,
побуждает контроллер запросить другой режим ввода, в данном случае русский. Метод
`-[KeyrillAlphaController requestInputMode:client:]` посылает запрос из блока, поставленного в
очередь вызовом `dispatch_async` после того, как клавиша возвращена приложению; блок удерживает
контроллер слабой ссылкой, как описано в
[главе об Objective-C](objective-c.ru.md#%D0%B1%D0%BB%D0%BE%D0%BA%D0%B8), и отказывается от запроса,
если поколение выделения за это время изменилось.

> [!TIP]
> **Запомните.** После пробела слово остаётся собственной разметкой Keyrill, поэтому отмена является
> обычной правкой, а зафиксированный текст никогда не затрагивается.

## Тесты свойств в Rust

*Цель: научиться читать тест свойства и понимать, что делает его воспроизводимым.*

Тест на примерах проверяет случаи, которые кто-то придумал. **Тест свойства** (англ. property test)
формулирует закон, который должен выполняться для любых входных данных, и проверяет его на множестве
сгенерированных входов; если один из них не проходит, инструмент прежде, чем сообщить об ошибке,
**минимизирует** (англ. shrinks) его до небольшого входа, на котором тест по-прежнему не проходит. В
Python для этого служит [Hypothesis](https://hypothesis.readthedocs.io/), в JavaScript —
[fast-check](https://fast-check.dev/), в Rust — [proptest](https://proptest-rs.github.io/proptest/);
все они происходят от [QuickCheck](https://en.wikipedia.org/wiki/QuickCheck) из Haskell. Следующий
тест утверждает, что идентификатор плана никогда не повторяется, что бы ни происходило:

[`crates/keyrill-core/tests/contracts.rs:199-232`](../../crates/keyrill-core/tests/contracts.rs#L199-L232)

```rust
    // Ids grow strictly across resets and invalidation, so a late acknowledgement of an expired
    // plan can never match a later one; plans stay within one word and its Space. Codes divisible
    // by 3 reset before acknowledging, code 7 reports Unknown, and a failed proposal resets
    #[test]
    fn arbitrary_event_schedules_never_resurrect_a_reset_token(events in proptest::collection::vec(0u8..12, 0..=256)) {
        let mut session = Session::default();
        session.set_fixtures(true).unwrap();
        let mut previous = 0;
        for code in events {
            let event = match code {
                0 => Event::Manual, 1 => Event::Reverse, 2 => Event::Backspace,
                3 => Event::Escape, 4 => Event::Finish, 5 => Event::Text(vec![32]),
                6 => Event::Text(vec![0xd800]), 7 => Event::Text(vec![0x301]),
                _ => Event::Text(vec![97]),
            };
            if let Ok(plan) = session.propose(&event, Mode::En, Context::Boundary) {
                prop_assert!(plan.id > previous);
                prop_assert!(plan.text.len() <= MAX_WORD + 1);
                prop_assert!(plan.expected.len() <= MAX_WORD + 1);
                if plan.action == Action::Mark {
                    prop_assert!(plan.text.iter().all(|&u| mapping::is_letter(u) || u == 32 || b"[];',./`\\{}:\"<>~|".contains(&(u as u8))), "owned units remain in the defined map");
                }
                previous = plan.id;
                if code % 3 == 0 {
                    session.reset();
                    prop_assert!(session.acknowledge(plan.id, Outcome::Verified).is_err());
                } else {
                    session.acknowledge(plan.id, if code == 7 { Outcome::Unknown } else { Outcome::Verified }).unwrap();
                }
            } else {
                session.reset();
            }
        }
    }
```

Генератор `vec(0u8..12, 0..=256)` производит до 256 кодов; `match` превращает их в события, причём
буквы преобладают: четыре кода из двенадцати печатают `a`. Сценарий намеренно включает враждебные
случаи: одиночный суррогат (`0xd800`, половина символа из двух единиц без второй половины),
комбинируемое ударение (`0x301`), сбросы посреди плана и неизвестные исходы. Настройка,
расположенная несколькими строками выше теста, фиксирует **начальное значение** (англ. seed)
генератора случайных чисел, так что при каждом запуске проверяются одни и те же 512 сценариев и сбой
воспроизводится на любой машине.

> [!TIP]
> **Запомните.** Тест свойства проверяет закон на сгенерированных входах. Фиксированное начальное
> значение делает тест точно воспроизводимым.

## Враждебный фальшивый клиент

*Цель: научиться читать тест, который внедряет сбой на одном определённом шаге протокола.*

Адаптер тестируется на классе `FakeClient` из файла `native/macos/tests/session_tests.mm`, который
реализует `IMKTextInput` поверх `NSMutableString` и по требованию ведёт себя неправильно. Это
**фальшивый объект** (англ. fake) — работающая замена, написанная вручную, а не мок, который лишь
записывает вызовы, как `unittest.mock` в Python. Документация этого класса лучше всего описывает,
что такой тест может доказать и чего не может:

[`native/macos/tests/session_tests.mm:43-59`](../../native/macos/tests/session_tests.mm#L43-L59)

```obj-c++
/// A scripted IMKTextInput client: one document string, a selection and a marked range, all in
/// UTF-16 units. setMarkedText ignores its replacement range and writes over the mark, or over
/// the selection when there is none, then places the caret by the mark-relative selection.
/// insertText writes to the same place for an NSNotFound range, or to exactly the explicit range
/// it is given, and puts the caret after the text.
/// stringFromRange clamps the range to the document and reports the clamped actual range.
///
/// A test plays the hostile host through `hook`, which runs at the entry of each call Keyrill
/// makes: it can move the caret, plant a foreign mark, switch Secure Input on, reenter the
/// session or throw, at exactly the call where a real host could. The flags below script what a
/// host does with a write. `calls` and `writes` let a test assert zero client contact or zero
/// writes after a refusal.
///
/// The client does nothing on its own: no focus, layout, undo, attributes or IMK reentrancy, and
/// it never moves the caret or commits the mark unless a test tells it to. That keeps every
/// schedule deterministic. A pass shows that the session refuses or verifies the scripted
/// schedule; it shows nothing about what a real application does.
```

Перехватчик — свойство-блок, которое вызывается при входе в каждый метод клиента и ещё раз после
того, как подействовала каждая запись:

[`native/macos/tests/session_tests.mm:138-157`](../../native/macos/tests/session_tests.mm#L138-L157)

```obj-c++
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
```

Это [внедрение сбоев](https://en.wikipedia.org/wiki/Fault_injection) (англ. fault injection) на
каждом шаге протокола: перехватчик служит точкой внедрения, а тест выбирает шаг и вид сбоя.
Рассмотрим полный тест, который повторно входит в сеанс изнутри его собственной записи:

[`native/macos/tests/session_tests.mm:1786-1810`](../../native/macos/tests/session_tests.mm#L1786-L1810)

```obj-c++
// IMK asks for a commit from inside the first mark write, after its effect. The nested finish
// makes zero client calls and reports an interruption; the outer write is then unknown, its key
// stays consumed, and the session stops with the client's document as the write left it (S3, S4)
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
```

Здесь в естественном для них месте встречаются два средства из
[главы об Objective-C](objective-c.ru.md#%D0%B1%D0%BB%D0%BE%D0%BA%D0%B8). Пометка `__block`
позволяет блоку присваивать значение локальной переменной объемлющей функции; без неё захваченная
локальная переменная является копией, доступной только для чтения. Пометка `__weak` разрывает цикл:
клиент хранит перехватчик, и перехватчик, захвативший клиента сильной ссылкой, навсегда сохранил бы
живыми их обоих.

Проверки, следующие далее, формулируют весь контракт сценария. После прерывания сеанс больше не
обращался к клиенту (`client.calls == callsAtInterruption`), выполнил одну запись и ни одной второй,
остановился, не имеет полномочий, сообщает причину, а документ находится ровно в том состоянии, в
которое его привела единственная запись. Обозначения `S3, S4` ссылаются на инварианты безопасности в
[ARCHITECTURE.md](../../ARCHITECTURE.ru.md#%D0%B8%D0%BD%D0%B2%D0%B0%D1%80%D0%B8%D0%B0%D0%BD%D1%82%D1%8B-%D0%B1%D0%B5%D0%B7%D0%BE%D0%BF%D0%B0%D1%81%D0%BD%D0%BE%D1%81%D1%82%D0%B8).

Следует помнить о пределах такого доказательства. Фальшивый объект доказывает правильность логики
адаптера, а не поведение настоящих приложений: TextEdit, Safari и Firefox отвечают на эти вызовы
каждый по-своему, и некоторые их ответы неверны. Проект считает контролируемые тесты и физические
тесты приложений разными видами доказательств и никогда не подменяет одни другими; оба вида описаны
в разделе [«Тестирование»](../testing.ru.md).

> [!TIP]
> **Запомните.** Фальшивый объект с перехватчиком позволяет внедрить сбой на любом отдельном шаге.
> Успешное прохождение такого теста ничего не говорит о настоящих приложениях.
