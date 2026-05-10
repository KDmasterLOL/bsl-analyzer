# QueryNestedFieldsByDot

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит разыменование ссылочных полей через точку в запросах, что может ухудшать производительность.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/query_nested_fields_by_dot.rs`
- `<v8std mirror>/docs/diagnostics/bslls/QueryNestedFieldsByDot.md`

## Как реализовано

Тонкий SDBL dispatch для `SdblDiagnostic::QueryNestedFieldsByDot`, диапазон мапится обратно в BSL.

## Что покрыто

Тестовый набор покрывает выборку, joins, virtual tables, `WHERE`, `ВЫРАЗИТЬ` и исключения для агрегатов/простых выражений.

## Пробелы и ограничения

Качество зависит от SDBL type/metadata inference. Нет rewrite в явное соединение или временную таблицу.

## Может ли инфраструктура улучшить качество

Да. Нужны typed query fields и query rewrite suggestions.

## Возможное объединение

Близко к `RefOveruse`, `JoinWithVirtualTable`, `FieldsFromJoinsWithoutIsNull`. Общая query performance diagnostics инфраструктура оправдана.

## Вывод

Полезное SDBL performance правило, но actionable fix требует более глубокого query transformation.

## Закрыто Track 2

**Phase C §4 Slice 1 (commit `329802ad`, 2026-05):** добавлен
`minPathDepth` config-knob (handler-side filter); SDBL-схема расширена
полем `parts_count` для emit'а; hard floor parts.len() >= 2 закреплён;
card-level docs (en/ru) описывают новую конфигурацию.
