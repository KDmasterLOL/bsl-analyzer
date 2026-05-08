# CommentedCode

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Диагностика ищет закомментированные фрагменты кода. По `#std456` в модулях не
должно оставаться мертвого кода, отладочных остатков и служебных пометок,
связанных с процессом разработки.

В текущей реализации правило фактически отвечает на более узкий вопрос:
"похож ли блок `//`-комментариев на закомментированный BSL-код?". Служебные
пометки вроде `TODO`, `MRG`, `debug` в основном покрывает соседняя диагностика
`UsingServiceTag`.

## Проверенные источники

- Реализация:
  `crates/ide-diagnostics/src/handlers/commented_code.rs`.
- Запуск:
  `crates/ide-diagnostics/src/runner.rs`,
  `crates/ide-diagnostics/src/handlers.rs`,
  `crates/ide-diagnostics/src/code.rs`.
- Смежные comment/text diagnostics:
  `crates/ide-diagnostics/src/handlers/using_service_tag.rs`,
  `crates/ide-diagnostics/src/handlers/space_at_start_comment.rs`,
  `crates/ide-diagnostics/src/handlers/bad_words.rs`,
  `crates/ide-diagnostics/src/handlers/missing_code_try_catch_ex.rs`.
- Конфигурация:
  `docs/configuration/DIAGNOSTICS.md`,
  `docs/configuration/PROJECT_CONFIGURATION.md`.
- Rule-доки:
  `crates/ide-diagnostics/docs/ru/CommentedCode.md`,
  `crates/ide-diagnostics/docs/en/CommentedCode.md`.
- Provenance:
  `docs/legal/diagnostics/CommentedCode.md`.
- Локальный `v8std`:
  `<v8std mirror>/docs/diagnostics/bslls/CommentedCode.md`,
  `<v8std mirror>/docs/std/456.md`.
- Внешние ссылки из rule-доков:
  `https://its.1c.ru/db/v8std/content/456/hdoc`,
  `https://v8std.ru/std/456/`,
  `https://v8std.ru/diagnostics/bslls/CommentedCode/`,
  `https://1c-syntax.github.io/bsl-language-server/diagnostics/CommentedCode/`.

## Как реализовано

Диагностика находится в line/text группе, но работает через parser tokens:

- собирает все `SyntaxKind::COMMENT`;
- группирует комментарии на соседних строках в `CommentGroup`;
- для каждой строки убирает ведущие `//`;
- отбрасывает пустые комментарии, doc-markers, описательные начала и
  `exclusionPrefixes`;
- считает эвристический score по признакам кода: `=`, `;`, BSL keywords,
  `Конец`, скобки, точка с вызовом/присваиванием, identifier с `=` или `(`;
- если score строки `>= 4`, строка считается code-like;
- если в группе есть хотя бы одна code-like строка, создается один diagnostic;
- range дополнительно обрезается до первой и последней code-like строки.

Quick-fix нет. Настройка `exclusionPrefixes` реализована как comma-separated
список.

## Что покрыто

- обычные текстовые комментарии не диагностируются;
- одиночное закомментированное присваивание;
- многострочный закомментированный блок;
- закомментированная процедура;
- несколько соседних закомментированных строк группируются в один diagnostic;
- описательные комментарии вокруг code-like блока не попадают в range;
- `exclusionPrefixes` исключает шаблонные comment-prefix строки;
- comment tokens берутся из lexer/parser, поэтому `//` внутри строковых
  литералов не должен давать false positive.

Базовый сценарий "в модуле оставили закомментированный BSL-блок" покрыт.

## Пробелы покрытия

- Параметр `threshold` задокументирован в handler-комментарии и
  `docs/configuration/DIAGNOSTICS.md`, но в реализации не читается. Фактический
  порог жестко зашит как `score >= 4`.
- `docs/configuration/DIAGNOSTICS.md` перечисляет только `threshold`, но не
  реализованный `exclusionPrefixes`.
- Документация правила говорит про служебные пометки, TODO и merge/debug
  остатки из `#std456`. Сам `CommentedCode` их почти не ловит; это зона
  `UsingServiceTag`. В текущем виде пользовательская документация смешивает две
  разные диагностики.
- Recognizer не парсит очищенный от `//` код, а использует эвристики. Это
  неизбежно дает и false positives, и false negatives.
- Keyword scoring неполный: нет многих BSL-конструкций (`ИначеЕсли`,
  `Попытка`, `Исключение`, `ВызватьИсключение`, `Выполнить`,
  `ДобавитьОбработчик`, `УдалитьОбработчик`, препроцессорные директивы), а
  English keywords почти не участвуют в score.
- `SpaceAtStartComment` прямо содержит TODO про `CodeRecognizer`: сейчас
  закомментированный код без пробела после `//` может одновременно получать
  `CommentedCode` и `SpaceAtStartComment`.
- Группировка смотрит только на соседние строки comment tokens. Она не
  анализирует, есть ли на этих строках реальный код до inline-комментария, и не
  использует `LineIndex` из контекста.
- Обычный inline-комментарий с примером кода, например `// X = Y;`, может быть
  воспринят как закомментированный код.
- Блоки с физически пустыми строками между commented lines будут разбиты на
  несколько групп и могут дать несколько diagnostics.
- Нет тестов на задокументированный `threshold`, на служебные теги из `#std456`,
  на inline-comment false positives, на английский закомментированный метод,
  на препроцессор, на `#Если`/`&НаКлиенте`, на реальные дубли со
  `SpaceAtStartComment`.

## Может ли инфраструктура улучшить качество

Да. Для этой диагностики уже достаточно токенов parser'а, но качество сильно
выиграет от общего comment-analysis слоя:

- один проход по `COMMENT` токенам на файл;
- общий `CommentGroup` с информацией о line/column и наличии кода до/после
  комментария;
- общий `CodeRecognizer`, который сначала применяет быстрые эвристики, а затем
  при необходимости пробует распарсить очищенный BSL-фрагмент;
- shared классификация `commented code`, `service tag`, `documentation`,
  `annotation`, `separator`;
- общий механизм, чтобы `SpaceAtStartComment` не ругался на строки, уже
  признанные закомментированным кодом.

HIR здесь не нужен для основного сценария, но parser можно использовать как
валидацию stripped comment block.

## Возможное объединение

Ближайшие правила по механике и смыслу: `UsingServiceTag`,
`SpaceAtStartComment`, `BadWords`, `MissingCodeTryCatchEx`, `LineLength`,
`ConsecutiveEmptyLines`, частично `DuplicateStringLiteral`.

`CommentedCode` и `UsingServiceTag` особенно близки по стандарту `#std456`:
один пункт стандарта требует убрать и закомментированный код, и служебные
пометки. Внешние `DiagnosticCode` лучше оставить раздельными, потому что
исправления и объяснения разные: удалить dead code block против убрать
служебный маркер. Но документацию стоит развести яснее: `CommentedCode` — про
кодоподобные comment blocks, `UsingServiceTag` — про TODO/MRG/debug/служебные
теги.

С `SpaceAtStartComment` объединение должно быть инфраструктурным. Это не одно
правило, но `SpaceAtStartComment` должен использовать результат
`CommentedCode`/`CodeRecognizer`, чтобы не создавать шум на закомментированном
коде без пробела.

## Варианты снятия ограничений

1. Либо реализовать параметр `threshold`, либо убрать его из документации и
   оставить только реальный `exclusionPrefixes`.
2. Вынести `CommentGroup`/`collect_comment_tokens` в общий helper для
   `CommentedCode`, `UsingServiceTag`, `SpaceAtStartComment`, `BadWords` и
   `MissingCodeTryCatchEx`.
3. Добавить `CodeRecognizer`: strip `//`, сохранить line mapping, попробовать
   parser на фрагменте, а эвристику оставить fallback'ом.
4. Расширить keyword set или заменить его на таблицу canonical BSL keywords,
   уже используемую синтаксическими диагностиками.
5. Синхронизировать docs: служебные пометки вынести в `UsingServiceTag` или
   явно описать, что `CommentedCode` их не покрывает.
6. Научить `SpaceAtStartComment` пропускать comment tokens, которые
   классифицированы как commented code.
7. Добавить tests на `threshold`/его отсутствие, `exclusionPrefixes`,
   TODO/MRG/debug, английские keywords, препроцессор, inline comments и
   взаимодействие со `SpaceAtStartComment`.
8. Рассмотреть quick-fix "удалить comment block", но только после точного range
   и низкого false-positive rate.

## Вывод

Диагностика полезна, но сейчас это эвристический recognizer с рассинхроном
документации и реализации. Самое важное — определиться с границами:
`CommentedCode` должен проверять закомментированный код, а служебные пометки
должны жить в `UsingServiceTag`. Следующий практичный шаг — общий
comment-analysis/CodeRecognizer слой и синхронизация конфигурации
`threshold`/`exclusionPrefixes` с реальным поведением.
