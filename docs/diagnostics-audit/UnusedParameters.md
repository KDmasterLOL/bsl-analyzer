# UnusedParameters

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит параметры метода, которые не используются в теле.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unused_parameters.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/UnusedParameters.md`

## Как реализовано

Обходит module bodies, собирает used identifiers из HIR expressions и сравнивает с параметрами. Пропускает пустые тела, platform event handlers, form/HTTP handlers, `NotifyDescription` callbacks и attachable prefixes.

## Что покрыто

Покрыты обычные методы, fixed-signature handlers, callbacks и настройка `attachableMethodPrefixes`.

## Пробелы и ограничения

Использование определяется по имени в expressions, без полноценного semantic binding. Удаление параметра требует обновить все вызовы, поэтому fix нет.

## Может ли инфраструктура улучшить качество

Да. Нужен binding-aware usage analysis и signature refactoring.

## Возможное объединение

Близко к `UnusedLocalVariable`, `UnusedLocalMethod`, `TransferringParametersBetweenClientAndServer`, `RewriteMethodParameter`.

## Вывод

Практичная диагностика с важными исключениями для platform handlers, но исправление требует refactoring engine.
