# JoinWithSubQuery

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Запрещает соединения с вложенными запросами в SDBL, так как такие конструкции часто ухудшают план выполнения.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/join_with_sub_query.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `crates/sdbl-hir/src/lower/from_clause.rs`, `crates/sdbl-hir/src/lower/join_clause.rs`
- `<v8std mirror>/docs/diagnostics/bslls/JoinWithSubQuery.md`
- `<v8std mirror>/docs/std/655.md`

## Как реализовано

Диагностика делегирована `sdbl_hir`: обработчик принимает `SdblDiagnostic::JoinWithSubQuery`, мапит диапазон запроса обратно в BSL-строку и создает предупреждение. SDBL эмитит диагностику из двух мест: подзапрос как источник в `FROM` при наличии `JOIN` (`from_clause.rs`) и подзапрос как `data_source` самой `JOIN`-секции (`join_clause.rs`).

## Что покрыто

Покрыты inline и многострочные тексты запросов, разные варианты join и несколько срабатываний в одном запросе.

## Пробелы и ограничения

Нет анализа исключений, когда вложенный запрос безопасен или неизбежен. Нет подсказки, как переписать запрос через временную таблицу. Качество полностью зависит от извлечения текста запроса и SDBL-парсера.

## Может ли инфраструктура улучшить качество

Частично. Улучшение SDBL AST/HIR и richer query diagnostics дадут более точные диапазоны и классификацию случая. Автоматический rewrite потребует отдельного query transformation слоя.

## Возможное объединение

Близко к `JoinWithVirtualTable`, `FullOuterJoinQuery`, `FieldsFromJoinsWithoutIsNull`, `LogicalOrInJoinQuerySection`. Можно объединять инфраструктуру и документацию семейства query performance, но коды лучше оставить раздельными из-за разных рекомендаций.

## Вывод

Правило реализовано как тонкая обертка над SDBL diagnostic. Для следующего качества нужны не изменения обработчика, а развитие SDBL-анализа и рекомендаций.
