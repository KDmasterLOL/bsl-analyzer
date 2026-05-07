# SelectTopWithoutOrderBy

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит `ВЫБРАТЬ ПЕРВЫЕ` / `SELECT TOP` без `УПОРЯДОЧИТЬ ПО`, где результат может быть недетерминированным.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/select_top_without_order_by.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/SelectTopWithoutOrderBy.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/412.md`

## Как реализовано

SDBL diagnostic передает `top_value`, `in_union`, `has_where`, `range`. Handler применяет конфиг `skipSelectTopOne` и мапит range в BSL.

## Что покрыто

Покрыты batch queries, nested subqueries, union, `TOP 1` с дефолтным пропуском и настройка `skipSelectTopOne`.

## Пробелы и ограничения

Не проверяется, что порядок уже детерминирован по уникальному условию. Нет предложения конкретных полей для сортировки.

## Может ли инфраструктура улучшить качество

Да. Typed query metadata и key/index info позволят предлагать сортировку по ключевым полям и снижать шум.

## Возможное объединение

Близко к query correctness/performance diagnostics: `QueryNestedFieldsByDot`, `LogicalOr*`, `SelectTopWithoutOrderBy`. Общий SDBL analyzer нужен.

## Вывод

Правило покрывает важный детерминизм запросов, но рекомендации пока слишком общие.
