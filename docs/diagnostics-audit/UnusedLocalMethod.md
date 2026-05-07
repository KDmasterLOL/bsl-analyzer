# UnusedLocalMethod

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит неэкспортные локальные методы, которые не вызываются.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unused_local_method.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/UnusedLocalMethod.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/456.md`

## Как реализовано

Использует `call_summary`, direct local call edges и консервативно добавляет все `obj.Method()` имена. Добавляет platform-called handlers форм, команд и HTTP services. Пропускает export, extension annotations, platform event handlers и configurable attachable prefixes.

## Что покрыто

Покрыты локальные вызовы, обработчики платформы, HTTP handlers, attachable prefixes и optional `checkObjectModule`.

## Пробелы и ограничения

Консервативный сбор `obj.Method()` может скрыть реально неиспользуемый метод с таким же именем. Динамические вызовы и reflection требуют allow-list.

## Может ли инфраструктура улучшить качество

Да. Нужен более точный call graph с receiver resolution и project-wide entry points.

## Возможное объединение

Близко к `UnusedLocalVariable`, `UnusedParameters`, `UnreachableCode`. Общий unused/dead-code analyzer нужен.

## Вывод

Правило осторожное и практичное, но платит false negatives за снижение false positives.
