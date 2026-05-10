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

## Закрыто Track 2

**Phase C §4 Slice 3 (commit `c3cd1917`, 2026-05):** detection теперь
исключает агрегирующие subqueries (`subquery_has_aggregation` —
function-call-positioned scan для `СУММА`/`SUM`/`СРЕДНЕЕ`/`AVG`/
`МИНИМУМ`/`MIN`/`МАКСИМУМ`/`MAX`/`КОЛИЧЕСТВО`/`COUNT`, а также
`GROUP BY`-only subqueries). Card-level docs (en/ru) обновлены с
секцией Aggregation Exemption.

## Закрыто Track 3

**Phase C sub-slice C3 (commit `<pending>`, 2026-05):** добавлены
regression-guard snapshot-fixtures для edge cases aggregation exemption:

- `track3_join_with_nested_inner_aggregation_currently_emits_snapshot` —
  вложенная агрегация на внутреннем уровне сейчас не освобождает JOIN с
  внешним подзапросом.
- `track3_join_with_having_only_aggregation_currently_emits_snapshot` —
  агрегат только в `ИМЕЮЩИЕ`/`HAVING` сейчас не учитывается exemption-логикой.
- `track3_join_with_totals_subquery_currently_emits_snapshot` — `ИТОГИ` /
  `TOTALS` сейчас не классифицируется как aggregation exemption.

Расширение `subquery_has_aggregation` на nested query levels, `HAVING` и
`TOTALS` оставлено в Track 4/6; Track 3 фиксирует текущее поведение без
production changes.
