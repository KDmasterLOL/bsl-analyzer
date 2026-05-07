# RefOveruse

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит избыточное использование поля `Ссылка` в запросах, когда объект уже является ссылкой.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/ref_overuse.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/RefOveruse.md`

## Как реализовано

Тонкий SDBL dispatch для `SdblDiagnostic::RefOveruse`; handler мапит диапазон и выдает предупреждение.

## Что покрыто

Правило рассчитано на `.Ссылка` в середине/конце доступа, double ref и исключения для простого `T.Ссылка` и табличных частей.

## Пробелы и ограничения

Тесты фиксируют, что без metadata/type resolution diagnostic не появляется. Нет fix для удаления лишнего `.Ссылка`.

## Может ли инфраструктура улучшить качество

Да. Нужны typed SDBL fields и metadata-backed tests; для fix нужен query text edit.

## Возможное объединение

Близко к `QueryNestedFieldsByDot` и query performance diagnostics. Можно объединить SDBL typed-access analyzer.

## Вывод

Правило потенциально полезное, но сейчас его фактическое покрытие сильно зависит от metadata/inference.
