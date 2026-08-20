# CanonicalSpellingKeywords

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Диагностика проверяет канонический регистр ключевых слов BSL, директив
препроцессора, символов препроцессора и директив компиляции. BSL
регистронезависим, но стандарт `#std441` требует писать ключевые слова как в
документации и синтакс-помощнике.

Для каждого некорректного токена диагностика предлагает quick-fix с заменой на
каноническую форму в той же языковой ветке: русское написание заменяется на
русское, английское — на английское.

## Проверенные источники

- Реализация:
  `crates/ide-diagnostics/src/handlers/canonical_spelling_keywords.rs`.
- Запуск:
  `crates/ide-diagnostics/src/runner.rs`,
  `crates/ide-diagnostics/src/code.rs`.
- Lexer/parser token kinds:
  `crates/lexer/src/lib.rs`,
  `crates/parser/src/syntax_kind.rs`,
  `crates/syntax/src/syntax_kind.rs`,
  `crates/parser/src/grammar.rs`.
- Связанный список известных preprocessor symbols:
  `crates/ide-diagnostics/src/utils/preprocessor_symbols.rs`.
- Rule-доки:
  `crates/ide-diagnostics/docs/ru/CanonicalSpellingKeywords.md`,
  `crates/ide-diagnostics/docs/en/CanonicalSpellingKeywords.md`.
- Provenance:
  `docs/legal/diagnostics/CanonicalSpellingKeywords.md`.
- Локальный `v8std`:
  `<v8std mirror>/docs/diagnostics/bslls/CanonicalSpellingKeywords.md`,
  `<v8std mirror>/docs/diagnostics/v8-code-style/bsl-canonical-pragma.md`,
  `<v8std mirror>/docs/std/441.md`.
- Внешние ссылки из rule-доков:
  `https://its.1c.ru/db/v8std#content:441:hdoc`,
  `https://v8std.ru/std/441/`,
  `https://v8std.ru/diagnostics/acc/1248/`,
  `https://1c-syntax.github.io/bsl-language-server/diagnostics/CanonicalSpellingKeywords/`.

## Как реализовано

Handler берет `ctx.parse()`, проходит по всем tokens через
`root.descendants_with_tokens()` и пропускает trivia. Для известных
`SyntaxKind` он вызывает `check_keyword(actual, canonical_forms)`.

Проверяемые группы:

- BSL keywords: процедуры/функции, ветвления, циклы, try/except, переменные,
  `Новый`, `Выполнить`, event handlers, async/await, логические операторы,
  boolean/null/undefined literals;
- preprocessor directive tokens: `#Если`, `#ИначеЕсли`, `#Иначе`,
  `#КонецЕсли`, `#Область`, `#КонецОбласти`, `#Вставка`;
- известные preprocessor symbols, если `IDENT` находится внутри
  preprocessor-директивы;
- compilation directives: `&НаКлиенте`, `&НаСервере`,
  `&НаСервереБезКонтекста`, `&НаКлиентеНаСервере`,
  `&НаКлиентеНаСервереБезКонтекста`.

Для логических операторов допускаются несколько канонических форм:
`И`, `And`, `AND`; `Или`, `ИЛИ`, `Or`, `OR`; `Не`, `НЕ`, `Not`, `NOT`.
Для `Каждого` допустимы `Каждого`, `каждого`, `Each`, `each`.

## Что покрыто

- полностью канонический BSL-код не диагностируется;
- lower-case, upper-case и mixed-case BSL keywords;
- русские и английские keywords;
- логические операторы с несколькими допустимыми формами;
- `Каждого` / `каждого` и `Each` / `each`;
- `Перем`, `Новый`, `Неопределено`, ветвления, циклы, `Прервать`,
  `Продолжить`, `try/except`, `ВызватьИсключение`;
- определения процедур/функций, `Знач`, `Экспорт`;
- базовые preprocessor directives и preprocessor symbols;
- `&НаСервере` / `&НаКлиенте` style annotations;
- quick-fix на точный диапазон токена.

Покрытие хорошее для core language tokens, и правило реализовано на правильном
уровне: через lexer/parser tokens, а не через поиск строк.

## Пробелы покрытия

- В `SyntaxKind` есть токены `PRE_END_INSERT`, `PRE_DELETE`,
  `PRE_END_DELETE`, но handler проверяет только `PRE_INSERT`. Поэтому
  `#КОНЕЦВСТАВКИ`, `#УДАЛЕНИЕ`, `#КОНЕЦУДАЛЕНИЯ` не получают canonical
  diagnostic, хотя lexer их распознает.
- В `SyntaxKind` есть annotation tokens `ANN_BEFORE`, `ANN_AFTER`,
  `ANN_AROUND`, `ANN_CHANGE_AND_VALIDATE`, но handler их не проверяет. Локальный
  `v8std` дополнительно указывает `v8cs:bsl-canonical-pragma`, так что
  каноническое написание extension-аннотаций `&Перед`, `&После`, `&Вместо`,
  `&ИзменениеИКонтроль` тоже стоит покрывать.
- Список известных preprocessor symbols в `preprocessor_symbols.rs` шире, чем
  `check_preproc_symbol()`: есть `МобильныйАвтономныйСервер` /
  `MobileStandaloneServer`, `Linux`, `Windows`, `MacOS`, но canonical-fix для
  них не предлагается.
- Rule-доки не синхронизированы с реализацией: таблицы не показывают `Новый`,
  `NULL`, `Асинх`, `Ждать`, `#Вставить`, extension-аннотации и часть реально
  проверяемых или потенциально проверяемых токенов.
- `SyntaxKind::is_preprocessor()` тоже отстает от enum: он не включает
  `PRE_INSERT`, `PRE_END_INSERT`, `PRE_DELETE`, `PRE_END_DELETE`. Это не ломает
  данный handler напрямую, но показывает, что canonical lists размазаны по
  нескольким местам и могут расходиться.
- Handler проверяет все keyword tokens без учета синтаксической роли. В parser
  keyword tokens могут быть name tokens после точки или в других name-slot
  позициях. Поэтому keyword-shaped identifier или метод вроде
  `Объект.выполнить()` может получить style diagnostic как keyword. Иногда это
  желаемо, но стандарт `#std441` говорит именно про ключевые слова конструкций
  языка, а не про любые имена, совпадающие с keywords.
- Нет теста, который сравнивает coverage handler'а со всеми keyword /
  preprocessor / annotation `SyntaxKind`. Поэтому при добавлении нового token
  легко забыть canonical mapping.
- Диагностика находится в `collect_line_diagnostics`, хотя фактически требует
  parse tree. Это не баг поведения, но название фазы может вводить в
  заблуждение при дальнейшей оптимизации пайплайна.

## Может ли инфраструктура улучшить качество

Да. Для этой диагностики достаточно token/parser-инфраструктуры:

- lexer уже знает все keyword/preprocessor/annotation tokens;
- parser уже отделяет preprocessor symbols в `PRE_SYMBOL`;
- `SyntaxKind` содержит классификаторы `is_keyword`, `is_preprocessor`,
  `is_annotation`;
- quick-fix infrastructure уже подходит для точечных замен;
- рядом есть `UnknownPreprocessorSymbol`, который уже содержит список известных
  preprocessor symbols.

Лучшее улучшение — сделать один источник правды для canonical forms, а не
держать списки отдельно в handler, docs, lexer tests и preprocessor utils.

## Возможное объединение

Близкие по смыслу style/token правила: `UnknownPreprocessorSymbol`,
`CompilationDirectiveLost`, `CompilationDirectiveNeedLess`,
`SeveralCompilerDirectives`, `IncorrectLineBreak`, `MissingSpace`,
`SpaceAtStartComment`, `YoLetterUsage`, `LatinAndCyrillicSymbolInWord`.

Объединять их в один `DiagnosticCode` не нужно: `CanonicalSpellingKeywords`
имеет очень простой quick-fix и информационную severity, а соседние правила
проверяют разные стандарты. Но полезен общий `syntax-style` слой:
классификация token kinds, canonical spelling table, preprocessor symbol table и
общие fixture-тесты. `UnknownPreprocessorSymbol` и
`CanonicalSpellingKeywords` особенно стоит связать через общий список
preprocessor symbols, чтобы известный символ автоматически имел canonical form
или явно был помечен как case-sensitive exception.

## Варианты снятия ограничений

1. Вынести canonical forms в таблицу рядом с lexer/syntax или в отдельный helper
   и покрыть ее тестом "каждый keyword/preprocessor/annotation token либо имеет
   mapping, либо явно excluded".
2. Добавить недостающие preprocessor directives:
   `#КонецВставки`, `#Удаление`, `#КонецУдаления`.
3. Добавить extension-аннотации:
   `&Перед` / `&Before`, `&После` / `&After`, `&Вместо` / `&Instead`,
   `&ИзменениеИКонтроль` / `&ChangeAndValidate`.
4. Синхронизировать `check_preproc_symbol()` с
   `utils::preprocessor_symbols`: добавить `МобильныйАвтономныйСервер` /
   `MobileStandaloneServer` и явно решить, нужны ли diagnostics для `Linux`,
   `Windows`, `MacOS`.
5. Уточнить role-aware поведение для keyword-shaped names: проверять все такие
   tokens как сейчас или пропускать токены в name-slot позициях после `.` и в
   объявлениях имен, если они не являются конструкциями языка.
6. Обновить ru/en docs: таблицы должны соответствовать фактическому списку
   проверяемых токенов или явно говорить, что таблицы неполные.
7. Перенести diagnostic из "line" группы в более точную token/parse phase, если
   пайплайн будет дальше разделяться по стоимости.

## Вывод

Диагностика хорошо реализует основную часть `#std441` и имеет полезный точечный
quick-fix. Основной риск сейчас — рассинхронизация списков: lexer уже знает
больше preprocessor directives и annotation tokens, чем handler и документация.
Следующий практичный шаг — сделать общий canonical table и добавить coverage
test по `SyntaxKind`, после чего расширить проверку на недостающие директивы и
аннотации.
