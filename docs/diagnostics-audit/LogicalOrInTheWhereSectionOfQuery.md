# LogicalOrInTheWhereSectionOfQuery

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит `ИЛИ` / `OR` в секции `ГДЕ` запроса, где такие условия часто мешают оптимальному использованию индексов.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/logical_or_in_the_where_section_of_query.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `<v8std mirror>/docs/diagnostics/bslls/index.md`

## Как реализовано

Диагностика реализована через SDBL dispatch: `sdbl_hir` находит `OR` в `WHERE`, обработчик создает diagnostic с маппингом диапазона в BSL.

## Что покрыто

Покрыты обычные и вложенные условия `WHERE`, скобки и вложенные запросы. Тесты отделяют `OR` в `CASE` и `JOIN ON`, чтобы они не попадали в это правило.

## Пробелы и ограничения

Диагностика не оценивает, можно ли заменить выражение на `В` / `IN`, объединить условия или переписать запрос без изменения семантики. Нет учета индексов и реального плана. В отличие от `LogicalOrInJoin`, тут нет исключения для `ИЛИ` по вариантам одного поля — рулдок сам предупреждает о ложных срабатываниях. `HAVING` не лоуэрится и не покрыт, хотя проблема та же.

## Может ли инфраструктура улучшить качество

Да, если SDBL HIR будет отдавать нормализованное дерево булевых выражений и информацию об одинаковых полях. Это позволит снизить шум и давать более конкретные подсказки.

## Возможное объединение

Ближайшая пара - `LogicalOrInJoinQuerySection`. Возможен общий internal engine “OR in query clauses”, но публично лучше сохранить две диагностики из-за разных контекстов.

## Вывод

Базовый поиск опасного `OR` покрыт, но правило остается эвристическим и требует более богатой query semantics для рекомендаций.

## Закрыто Track 2

**Phase C §4 delta-audit (2026-05):** subquery WHERE coverage
проанализирован — текущая реализация `sdbl-hir` уже эмитит для
вложенных WHERE; работ Track 2 не требуется. Closed без implementation
slice.

## Закрыто Track 3

**Phase C sub-slice C3 (commit `<pending>`, 2026-05):** subquery WHERE gap закрыт
новыми snapshot-fixtures:

- `track3_or_in_russian_subquery_where_snapshot` — `ИЛИ` во вложенном
  русском `ГДЕ`.
- `track3_or_in_deep_subquery_where_snapshot` — `OR` в глубоко вложенном
  английском `WHERE`.

Оба fixtures используют `check_diagnostics_snapshot_for` и подтверждают, что
текущий SDBL dispatch мапит диагностику из subquery WHERE обратно в BSL-строку.
