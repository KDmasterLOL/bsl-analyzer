# FullOuterJoinQuery

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

`FULL OUTER JOIN` в запросах часто вреден для производительности, особенно на
PostgreSQL. Основание - `#std435`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/full_outer_join_query.rs`
- `crates/sdbl-hir`
- `crates/ide-diagnostics/docs/ru/FullOuterJoinQuery.md`
- `docs/legal/diagnostics/FullOuterJoinQuery.md`
- `<v8std mirror>/docs/std/435.md`

## Как реализовано

SDBL HIR эмитит `SdblDiagnostic::FullOuterJoin`; handler мапит range в BSL и
создает diagnostic.

## Что покрыто

Тесты проверяют русские/английские full joins, `FULL JOIN` без `OUTER`,
несколько full joins и negative cases с left join.

## Пробелы и ограничения

- Не анализируется, есть ли конкретный full join оправданным и маленьким по
  данным.
- Сообщение предлагает UNION/LEFT JOIN, но не строит rewrite.
- Зависит от корректной extraction query strings и SDBL parser.

## Может ли инфраструктура улучшить качество

Query analyzer может добавить explain-like hints: таблицы, join graph,
альтернативы rewrite, platform/dbms-specific severity.

## Возможное объединение

Внутренне объединить с SDBL performance diagnostics. Внешне оставить отдельным:
это конкретный SQL-pattern.

## Вывод

Правило простое и хорошо ложится на SDBL HIR. Улучшения - richer query context
и DBMS-aware severity.

