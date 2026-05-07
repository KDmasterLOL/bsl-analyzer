# CommandModuleExportMethods

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Диагностика запрещает экспортные процедуры и функции в модулях команд и общих
команд. По `#std544` к таким модулям нельзя обращаться из внешнего прикладного
кода как к публичному API, поэтому `Экспорт` в объявлении метода создает
ложное ожидание повторного использования.

Правило относится именно к `CommandModule`. Модули форм находятся рядом по
смыслу, но имеют другие исключения и сейчас покрываются другими диагностиками
только частично.

## Проверенные источники

- Реализация:
  `crates/ide-diagnostics/src/handlers/command_module_export_methods.rs`.
- Запуск:
  `crates/ide-diagnostics/src/runner.rs`,
  `crates/ide-diagnostics/src/handlers.rs`,
  `crates/ide-diagnostics/src/code.rs`.
- Module metadata:
  `crates/ide-db/src/metadata.rs`,
  `crates/bsl-metadata/src/enums.rs`.
- ItemTree:
  `crates/hir-def/src/item_tree.rs`,
  `crates/hir-def/src/item_tree/lower.rs`.
- Смежные export/API правила:
  `crates/ide-diagnostics/src/handlers/server_side_export_form_method.rs`,
  `crates/ide-diagnostics/src/handlers/common_module_missing_api.rs`,
  `crates/ide-diagnostics/src/handlers/non_export_methods_in_api_region.rs`.
- Rule-доки:
  `crates/ide-diagnostics/docs/ru/CommandModuleExportMethods.md`,
  `crates/ide-diagnostics/docs/en/CommandModuleExportMethods.md`.
- Provenance:
  `docs/legal/diagnostics/CommandModuleExportMethods.md`.
- Локальный `v8std`:
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/CommandModuleExportMethods.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/std/544.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/v8-code-style/export-method-in-command-form-module.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/std/630.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/std/404.md`.
- Внешние ссылки из rule-доков:
  `https://its.1c.ru/db/v8std/content/544/hdoc`,
  `https://v8std.ru/std/544/`,
  `https://v8std.ru/diagnostics/bslls/CommandModuleExportMethods/`,
  `https://1c-syntax.github.io/bsl-language-server/diagnostics/CommandModuleExportMethods/`.

## Как реализовано

Handler работает на `ItemTree + ModuleMetadata`:

- получает `ctx.module_metadata()`;
- если `module_type != ModuleType::CommandModule`, сразу выходит;
- проходит по `item_tree.procedures()` и `item_tree.functions()`;
- для каждого метода с `is_export` создает diagnostic на `name_range`;
- quick-fix нет.

Тип `CommandModule` определяется в metadata по путям вида:

- `CommonCommands/<Name>/Ext/CommandModule.bsl`;
- `ОбщиеКоманды/<Name>/Ext/CommandModule.bsl`;
- `<TypePlural>/<Name>/Commands/<Cmd>/Ext/CommandModule.bsl`.

И обычные команды объектов, и общие команды сводятся к одному
`ModuleType::CommandModule`.

## Что покрыто

- экспортная процедура в модуле команды диагностируется;
- экспортная функция в модуле команды диагностируется;
- несколько методов дают несколько diagnostics;
- неэкспортные процедуры и функции игнорируются;
- обычный `.bsl` файл с экспортным методом не диагностируется;
- metadata-level tests отдельно проверяют `CommonCommands`,
  `ОбщиеКоманды` и обычный `Commands/.../CommandModule.bsl`.

Основной сценарий покрыт хорошо: если файл распознан как `CommandModule`,
диагностика надежно находит все экспортные методы через `ItemTree`.

## Пробелы покрытия

- В тестах самой диагностики есть только subordinate command path
  `Catalogs/.../Commands/.../CommandModule.bsl`. Нет diagnostic-level теста на
  `CommonCommands/.../CommandModule.bsl` и `ОбщиеКоманды/...`.
- Path detector поддерживает русское имя каталога `ОбщиеКоманды`, но для
  subordinate commands ищет только `Commands`, не `Команды`. Если такие пути
  возможны в поддерживаемых выгрузках, модуль станет `Unknown` и правило
  пропустит экспортные методы.
- Diagnostic подсвечивает имя метода, а не само ключевое слово `Экспорт`.
  Пользователю понятнее подсвечивать лишний модификатор.
- `ItemTree` хранит только `is_export`, но не `export_keyword_range`. Из-за
  этого сложно сделать точный quick-fix "удалить `Экспорт`" без повторного
  AST-поиска.
- v8-code-style правило `export-method-in-command-form-module` упоминает
  исключение для экспортных процедур-обработчиков оповещений
  (`ОписаниеОповещения.ИмяПроцедуры`). Текущая диагностика не анализирует
  ссылки на методы из `ОписаниеОповещения` и будет ругаться на любой `Экспорт`
  в `CommandModule`. Если это исключение нужно применять и к модулям команд,
  возможны false positives.
- Диагностика не покрывает модули форм, хотя соседняя v8-code-style карточка
  говорит про "модуль команд и форм". У нас есть `ServerSideExportFormMethod`,
  но это другое правило: только managed forms, только server-side export,
  severity `Blocker`.
- Нет тестов на disabled config и на английский `Export`, хотя `ItemTree`
  должен распознавать оба варианта через parser.

## Может ли инфраструктура улучшить качество

Да, но не требуется тяжелый анализ для базового правила. Текущая связка
`ModuleMetadata + ItemTree` подходит хорошо: тип модуля и список методов уже
кэшируются через Salsa.

Качество можно улучшить точечно:

- расширить `ItemTree` range'ом ключевого слова `Экспорт`;
- добавить общий helper для обхода export methods, чтобы export/API
  диагностики не дублировали `procedures()` / `functions()`;
- добавить общий слой распознавания module paths и тестовые fixtures для всех
  supported directory variants;
- если принимать исключение v8-code-style для `ОписаниеОповещения`, использовать
  HIR/call-reference анализ, который умеет находить строковые или symbol-based
  ссылки на обработчики.

## Возможное объединение

Ближайший кластер — правила про публичный API и `Экспорт`:
`ServerSideExportFormMethod`, `CommonModuleMissingAPI`,
`NonExportMethodsInApiRegion`, `PublicMethodsDescription`,
`MissingCommonModuleMethod`, `ScheduledJobHandler`, `ExportVariables`,
`UnusedLocalMethod`.

Сливать `CommandModuleExportMethods` с form/common-module правилами в один
`DiagnosticCode` не стоит. У них разные module types, разные причины, разные
severity и разные исправления: в командном модуле `Экспорт` обычно лишний, в
общем модуле он может быть обязательной частью API, а в форме допустимость
зависит от контекста и аннотаций.

Но общий внутренний слой нужен:

- `ExportedMethod { name, name_range, source_range, export_range, annotations }`;
- helper для фильтрации по `ModuleType`;
- общий quick-fix для удаления или добавления `Экспорт`;
- единые тестовые builders для `CommandModule`, `CommonModule`, `FormModule`.

## Варианты снятия ограничений

1. Добавить в `Procedure`/`Function` `export_range: Option<TextRange>` и
   заполнять его в `item_tree/lower.rs`.
2. Перенести diagnostic range на `Экспорт` и добавить quick-fix удаления
   модификатора вместе с лишним пробелом.
3. Добавить тесты `CommandModuleExportMethods` на `CommonCommands`,
   `ОбщиеКоманды`, английский `Export` и disabled config.
4. Решить, поддерживаем ли русское имя subordinate каталога `Команды`; если да,
   добавить его в `get_module_type_from_uri()`.
5. Явно синхронизировать документы: `#std544` говорит про модули команд и общих
   команд, а v8-code-style соседнее правило дополнительно затрагивает формы и
   callback-исключение.
6. Если callback-исключение нужно для командных модулей, добавить отдельный
   анализ references из `ОписаниеОповещения` и не диагностировать такие методы.
7. Рассмотреть отдельную диагностику/расширение для форм, вместо смешивания ее
   с `CommandModuleExportMethods`.

## Вывод

Для своего узкого scope диагностика реализована корректно и дешево: metadata
определяет `CommandModule`, `ItemTree` дает экспортные методы. Основные риски
качества — неполная тестовая матрица путей, отсутствие range/fix для самого
`Экспорт` и неразрешенный вопрос с v8-code-style исключением для обработчиков
оповещений. Внутренне это правило стоит объединять с export/API кластером на
уровне shared helpers, но внешний diagnostic должен остаться отдельным.
