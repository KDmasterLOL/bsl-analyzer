# CodeOutOfRegion

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Диагностика проверяет, что код модуля находится внутри областей
`#Область/#КонецОбласти` или `#Region/#EndRegion`. Это часть стандарта
структуры модуля `#std455`: области делают модуль предсказуемым для навигации,
поддержки и коллективной разработки.

Правило не валидирует порядок и имена областей. Оно отвечает только на вопрос:
находится ли значимый module-level элемент внутри какой-либо области.

## Проверенные источники

- Реализация:
  `crates/ide-diagnostics/src/handlers/code_out_of_region.rs`.
- Запуск:
  `crates/ide-diagnostics/src/runner.rs`,
  `crates/ide-diagnostics/src/code.rs`.
- Region infrastructure:
  `crates/hir-def/src/region_tree.rs`,
  `crates/ide-diagnostics/src/context.rs`.
- Смежная структура модуля:
  `crates/ide-diagnostics/src/handlers/code_block_before_sub.rs`.
- Rule-доки:
  `crates/ide-diagnostics/docs/ru/CodeOutOfRegion.md`,
  `crates/ide-diagnostics/docs/en/CodeOutOfRegion.md`.
- Provenance:
  `docs/legal/diagnostics/CodeOutOfRegion.md`.
- Локальный `v8std`:
  `<v8std mirror>/docs/diagnostics/bslls/CodeOutOfRegion.md`,
  `<v8std mirror>/docs/std/455.md`,
  `module-structure-*` карточки в
  `<v8std mirror>/docs/diagnostics/v8-code-style/`.
- Внешние ссылки из rule-доков:
  `https://its.1c.ru/db/v8std#content:455:hdoc`,
  `https://v8std.ru/std/455/`,
  `https://v8std.ru/diagnostics/bslls/CodeOutOfRegion/`,
  `https://1c-syntax.github.io/bsl-language-server/diagnostics/CodeOutOfRegion/`.

## Как реализовано

Handler берет parse tree и cached `ctx.region_tree()`, затем рекурсивно
проходит по детям текущего узла:

- `SOURCE_FILE`, `PRE_ELSE_CLAUSE`, `PRE_ELSIF_CLAUSE` считаются module-level
  контейнерами;
- `PRE_IF_DIR` обрабатывается отдельно: переменные, методы и большинство
  statements считаются module-level, но top-level `CALL_STMT` / `ASSIGN_STMT`
  внутри `#Если` диагностируются только если до них в этой ветке уже был
  module-level definition или region;
- если значимый module-level элемент не находится внутри `RegionTree`, создается
  diagnostic;
- для процедур/функций подсвечивается имя метода;
- для statements диапазон расширяется до `;`;
- quick-fix нет.

Значимыми считаются процедуры, функции, module-level переменные и большая часть
исполняемых statements. `RAISE_STMT` намеренно не считается значимым в этой
диагностике.

## Что покрыто

- пустой файл;
- модуль без областей: переменные, методы и statements получают отдельные
  diagnostics;
- одиночный top-level statement вне области;
- процедура/функция вне области, диапазон на имени метода;
- код внутри обычной области не диагностируется;
- смешанный большой модуль с областями, препроцессором и кодом вне областей;
- `#Если/#ИначеЕсли/#Иначе` ветки;
- стандартный guard вида `#Если Сервер ... #Иначе ВызватьИсключение ...`;
- statement ranges с trailing semicolon.

Покрытие сильное для основного пользовательского сценария: найти элементы
модуля, которые забыли обернуть в область.

## Пробелы покрытия

- Список значимых statements локальный и не совпадает с другими слоями. В
  `is_significant_element()` есть `GOTO_STMT`, `EXECUTE_STMT`,
  `ADD_HANDLER_STMT`, `REMOVE_HANDLER_STMT`, но в `contains_executable_code()`
  они уже отсутствуют. В `CodeBlockBeforeSub` список снова другой.
- `RAISE_STMT` не диагностируется как code out of region. Это нужно для
  стандартного `#Если ... #Иначе ВызватьИсключение ... #КонецЕсли`, но правило
  становится неочевидным: обычный `ВызватьИсключение` вне области тоже не будет
  reported.
- Логика `PRE_IF_DIR` с `has_preceding_definition()` эвристическая. Она
  специально избегает шума в guard-ветках, но может пропустить реальные
  module-level assignments/calls вне областей в начале препроцессорной ветки.
- Диагностика проверяет "внутри любой области", не требуя top-level standard
  region. Код внутри произвольной вложенной или нестандартной области проходит
  это правило; за имена отвечает `NonStandardRegion`.
- Rule-доки говорят про контроль областей верхнего уровня, но реализация
  фактически использует `RegionTree::is_range_inside_region()` и принимает
  попадание в любую область, включая вложенные.
- Английская документация содержит только "Correct" пример, без явного
  "Incorrect" примера, хотя русская документация его показывает.
- Нет тестов на module-level `GOTO`, `EXECUTE`, `ADD_HANDLER`,
  `REMOVE_HANDLER`, standalone `RAISE`, `PRE_REGION_DIR` с такими statements и
  нестандартную область, которая технически прикрывает код.
- Нет quick-fix. Простое оборачивание в область возможно, но правильное имя
  области зависит от типа элемента, типа модуля и порядка секций.

## Может ли инфраструктура улучшить качество

Да. В отличие от `CodeBlockBeforeSub`, эта диагностика уже использует
`RegionTree`, но ей все еще не хватает общего представления структуры модуля:

- `RegionTree` знает области и вложенность;
- `ItemTree` знает методы и module-level переменные;
- `ModuleMetadata` может определить тип модуля и допустимый шаблон `#std455`;
- parser/HIR уже содержат общие списки statement kinds и терминаторов.

Лучшее улучшение — общий `ModuleStructure` слой, который один раз классифицирует
top-level sections, методы, переменные и свободные statements. Тогда
`CodeOutOfRegion` будет проверять только "у элемента нет enclosing section",
а не вручную решать, какой AST-узел значим.

## Возможное объединение

Ближайший кластер тот же, что у `CodeBlockBeforeSub`: `NonStandardRegion`,
`DuplicateRegion`, `EmptyRegion`, `CommonModuleMissingAPI`,
`MissingVariablesDescription`, `NonExportMethodsInApiRegion`,
`PublicMethodsDescription`, `CodeBlockBeforeSub`, плюс v8-code-style
`module-structure-top-region`, `module-structure-method-in-regions`,
`module-structure-init-code-in-region`, `module-structure-var-in-region`.

Внешние `DiagnosticCode` лучше оставить раздельными: "код вне области",
"нестандартная область", "дубликат области" и "код до методов" имеют разную
severity, разные сообщения и разные способы исправления. Но внутренне их стоит
объединить на уровне анализа структуры модуля и shared helpers:

- единый список module-level elements;
- единая классификация standard top-level regions;
- единая обработка препроцессорных branches;
- единый источник truth для ranges и quick-fix candidates.

## Варианты снятия ограничений

1. Вынести общий helper `is_module_level_statement` и использовать его в
   `CodeOutOfRegion`, `CodeBlockBeforeSub` и future `ModuleStructure`.
2. Явно задокументировать или пересмотреть исключение для `RAISE_STMT`: guard
   branch можно разрешать точечно, но standalone raise вне области должен быть
   осознанным решением.
3. Добавить тесты на `GOTO`, `EXECUTE`, `ADD_HANDLER`, `REMOVE_HANDLER`,
   standalone `RAISE`, `PRE_REGION_DIR` с этими statements.
4. Синхронизировать документацию с поведением: правило принимает любую область
   как covering region, а не валидирует только top-level standard regions.
5. Добавить в английскую документацию "Incorrect" пример.
6. При появлении `ModuleStructure` сделать это правило не рекурсивным AST-walk,
   а проверкой заранее собранных module-level elements.
7. Для quick-fix начинать не с автоматического исправления, а с code action
   "Create region around element" с выбором имени области.

## Вывод

Диагностика уже использует правильную базовую инфраструктуру `RegionTree` и
хорошо решает главную задачу. Основной риск качества — локальная и местами
несогласованная классификация значимых module-level statements, особенно внутри
препроцессора. Следующий практичный шаг — общий `ModuleStructure`/helper слой с
единым списком элементов и тестами на пропущенные statement kinds.

## Закрыто Track 2

**Phase C §3 Slice 1 (commit `effab845`, 2026-05):** локальный
significant-stmt list переехал в `hir_def::module_structure::significant`
(`is_significant_module_level_stmt`); audit gap по
`RAISE_STMT`/`LABEL_STMT` закрыт там же. Walk через `RegionTree` API
(Slice 2, `d32f55d9`).

## Закрыто Track 3

**Phase C sub-slice C1 (commit `[COMMIT]`, 2026-05):**

- `test_goto_stmt_outside_region_snapshot` — module-level `Перейти ~Метка;`
  outside any region is reported as `CodeOutOfRegion`.
- `test_label_stmt_outside_region_snapshot` — documents the current deliberate
  exclusion for a bare module-level `~Метка:`.
- `test_execute_stmt_outside_region_snapshot` — module-level
  `Выполнить("код");` outside any region is reported as `CodeOutOfRegion`.
- `test_add_handler_stmt_outside_region_snapshot` — module-level
  `ДобавитьОбработчик ...;` outside any region is reported.
- `test_remove_handler_stmt_outside_region_snapshot` — module-level
  `УдалитьОбработчик ...;` outside any region is reported.
- `test_standalone_raise_stmt_outside_region_snapshot` — documents the current
  deliberate exclusion for standalone module-level `ВызватьИсключение;`.
- `test_pre_region_dir_covers_inner_code_but_not_following_stmt_snapshot` —
  code inside `#Область` is covered, while following module-level code outside
  the region is reported.
