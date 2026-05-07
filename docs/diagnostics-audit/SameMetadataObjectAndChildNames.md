# SameMetadataObjectAndChildNames

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что имя дочернего объекта метаданных не совпадает с именем родителя.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/same_metadata_object_and_child_names.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/SameMetadataObjectAndChildNames.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/474.md`

## Как реализовано

Metadata dispatch по `ModuleMetadata`: для MDO проверяются реквизиты, табличные части и реквизиты табличных частей; для регистров - измерения, ресурсы и реквизиты.

## Что покрыто

Покрыты object/manager modules и register metadata, case-insensitive сравнение имен.

## Пробелы и ограничения

`SessionModule` заявлен в metadata, но в коде отмечено, что не поддержан infrastructure. Диапазон `TextRange::default()`, то есть нет точного места в XML.

## Может ли инфраструктура улучшить качество

Да. Нужны source ranges для metadata XML и project-level запуск без привязки к модулю.

## Возможное объединение

Близко к `ForbiddenMetadataName`, `MetadataObjectNameLength`, `MetadataObjectName*`. Общий metadata naming analyzer нужен.

## Вывод

Семантически важная проверка, но сейчас сильно ограничена отсутствием точных ranges и полным project-level metadata pass.
