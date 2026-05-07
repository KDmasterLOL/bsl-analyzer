# CodeBlockBeforeSub

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Диагностика проверяет базовый порядок структуры модуля: исполняемые операторы
тела модуля должны находиться после объявлений процедур и функций. По стандарту
`#std455` общий порядок такой: переменные модуля, процедуры/функции, затем
раздел инициализации.

Правило не проверяет всю структуру областей модуля. Оно ловит только ситуацию,
когда свободный исполняемый код расположен до первого найденного метода.

## Проверенные источники

- Реализация:
  `crates/ide-diagnostics/src/handlers/code_block_before_sub.rs`.
- Запуск:
  `crates/ide-diagnostics/src/runner.rs`,
  `crates/ide-diagnostics/src/code.rs`.
- Смежная структура областей:
  `crates/ide-diagnostics/src/handlers/code_out_of_region.rs`,
  `crates/hir-def/src/region_tree.rs`.
- Rule-доки:
  `crates/ide-diagnostics/docs/ru/CodeBlockBeforeSub.md`,
  `crates/ide-diagnostics/docs/en/CodeBlockBeforeSub.md`.
- Provenance:
  `docs/legal/diagnostics/CodeBlockBeforeSub.md`.
- Локальный `v8std`:
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/CodeBlockBeforeSub.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/std/455.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/acc/426.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/acc/428.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/v8-code-style/module-structure-init-code-in-region.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/v8-code-style/module-structure-top-region.md`.
- Внешние ссылки из rule-доков:
  `https://its.1c.ru/db/v8std/content/455/hdoc`,
  `https://v8std.ru/std/455/`,
  `https://v8std.ru/diagnostics/bslls/CodeBlockBeforeSub/`,
  `https://1c-syntax.github.io/bsl-language-server/diagnostics/CodeBlockBeforeSub/`.

## Как реализовано

Handler проходит только по непосредственным детям `SOURCE_FILE`:

- пока не найден top-level `PROCEDURE_DEF` или `FUNCTION_DEF`, собирает
  top-level executable blocks;
- `VAR_DEF`, trivia, comments, annotations и compiler directives игнорируются;
- `PRE_REGION_DIR` и `PRE_IF_DIR` считаются code block, если внутри есть
  исполняемый код вне процедур/функций;
- когда найден первый top-level метод и до него были code blocks, возвращается
  один diagnostic на диапазон от первого до последнего такого блока;
- если методов нет, diagnostic не выпускается.

Для области `#Область` диапазон начинается с первого исполняемого statement
внутри области, а заканчивается на конце последнего найденного блока. Quick-fix
нет.

Локальный `v8std` для BSLLS-карточки не привязывает правило к стандарту, но
наши rule-доки и provenance корректно выводят его из `#std455`.

## Что покрыто

- корректный порядок: переменные, методы, затем код инициализации;
- прямой top-level assignment/call перед процедурой;
- несколько top-level statements перед процедурой как один diagnostic;
- только переменные перед процедурой не диагностируются;
- модуль без процедур/функций не диагностируется;
- исполняемый код внутри top-level region перед top-level процедурой;
- top-level region, внутри которой только методы, не считается свободным кодом;
- английские keywords.

Покрытие достаточное для простого неструктурированного модуля, где методы
расположены на верхнем уровне.

## Пробелы покрытия

- Алгоритм ищет только top-level `PROCEDURE_DEF` / `FUNCTION_DEF`. Если методы
  находятся внутри областей, что как раз рекомендуется `#std455`, первый метод
  может не быть непосредственным ребенком `SOURCE_FILE`. В таком модуле
  область `Инициализация` с кодом перед областью методов может не попасть под
  диагностику.
- Порядок top-level regions не проверяется. Например, `#Область Инициализация`
  перед `#Область ПрограммныйИнтерфейс` требует module-structure анализа, а
  текущая диагностика видит только "есть ли потом top-level метод".
- `is_code_block()` и `contains_executable_code()` не совпадают с более полным
  набором statement kinds в соседних слоях. Они не учитывают `GOTO_STMT`,
  `EXECUTE_STMT`, `ADD_HANDLER_STMT`, `REMOVE_HANDLER_STMT`, `LABEL_STMT` и
  некоторые recovered/error statements.
- Для `PRE_IF_DIR` диапазон не подрезается до первого исполняемого statement,
  в отличие от `PRE_REGION_DIR`.
- Диагностика не использует `RegionTree`, поэтому не знает корневую область,
  вложенность, стандартные имена областей и тип модуля.
- Код внутри области перед методом и код вне области перед методом могут быть
  одновременно покрыты `CodeBlockBeforeSub` и `CodeOutOfRegion`, но эти правила
  сейчас не имеют общего view of module structure.
- Нет теста на основной рекомендованный шаблон `#std455`: все методы внутри
  областей, а раздел инициализации ошибочно расположен перед ними.
- Нет quick-fix. Для простого top-level случая можно было бы предложить
  переместить блок в конец модуля или в область `Инициализация`, но это
  потенциально рискованная правка.

## Может ли инфраструктура улучшить качество

Да. Текущая AST-проверка дешевая, но для качества ей не хватает общего
представления структуры модуля:

- `RegionTree` уже дает области, вложенность и ranges;
- `ItemTree` уже дает методы и их ranges;
- `ModuleMetadata` может подсказать тип модуля, а значит допустимый шаблон
  областей по `#std455`;
- соседний `CodeOutOfRegion` уже ходит по module-level elements и определяет,
  находится ли элемент внутри области.

Лучшее направление — общий `module_structure` анализ: собрать top-level
sections, module-level variables, methods and free statements один раз, затем
на его основе выпускать отдельные diagnostics.

## Возможное объединение

Ближайшие правила: `CodeOutOfRegion`, `NonStandardRegion`, `DuplicateRegion`,
`EmptyRegion`, `CommonModuleMissingAPI`, `MissingVariablesDescription`,
`NonExportMethodsInApiRegion`, `PublicMethodsDescription`, а также локальные
v8-code-style проверки `module-structure-top-region`,
`module-structure-init-code-in-region`, `module-structure-method-in-regions`,
`module-structure-var-in-region`.

Объединять внешние `DiagnosticCode` не стоит: пользователь должен отдельно
включать/настраивать "код до методов", "код вне областей", "нестандартная
область" и "пустая область". Но внутренне эти правила явно должны иметь общий
анализ структуры модуля. Тогда можно избежать противоречий, дублирования обходов
и пробелов для методов, спрятанных внутри областей.

## Варианты снятия ограничений

1. Добавить regression-тест: `#Область Инициализация` с кодом перед
   `#Область ПрограммныйИнтерфейс`, где все методы находятся внутри областей.
2. Построить общий `ModuleStructure` поверх `RegionTree + ItemTree`: список
   top-level sections, их типы, методы, переменные и свободные statements.
3. Использовать общий список executable/module-level statements с
   `CodeOutOfRegion` и HIR control-flow helpers, чтобы не забывать `GOTO`,
   `EXECUTE`, event handler statements и recovered statements.
4. Явно решить, что делать с препроцессорными ветками: проверять порядок в
   каждой потенциальной ветке или по синтаксическому порядку всех branches.
5. Уточнить диапазоны: для `PRE_IF_DIR` подрезать range до первого
   исполняемого statement так же, как для `PRE_REGION_DIR`.
6. В документации подчеркнуть, что правило проверяет только базовый порядок
   "код до методов"; полный стандарт областей покрывается другими diagnostics.
7. Рассмотреть осторожный quick-fix только для простого случая top-level блока
   без областей: перенести блок после последнего метода или обернуть в
   `#Область Инициализация`.

## Вывод

Диагностика правильно ловит простой и грубый дефект структуры модуля, но текущий
алгоритм не соответствует реальному рекомендованному стилю с методами внутри
областей. Главный практический шаг — общий module-structure анализ для
`CodeBlockBeforeSub`, `CodeOutOfRegion` и region diagnostics. До этого стоит
добавить тест на код инициализации перед областью методов, чтобы зафиксировать
самый важный пробел.
