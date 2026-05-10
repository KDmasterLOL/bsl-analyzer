# JoinWithVirtualTable

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит соединения с виртуальными таблицами в запросах. Стандарт `#std655` рекомендует избегать таких соединений из-за риска тяжелых планов выполнения.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/join_with_virtual_table.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `crates/sdbl-hir/src/lower/from_clause.rs`, `crates/sdbl-hir/src/lower/join_clause.rs`
- `<v8std mirror>/docs/diagnostics/bslls/JoinWithVirtualTable.md`
- `<v8std mirror>/docs/std/655.md`

## Как реализовано

Обработчик принимает `SdblDiagnostic::JoinWithVirtualTable` и через `sdbl_utils::dispatch_simple` мапит диапазон запроса в исходный BSL. Метаданные помечены как SQL/Standard/Performance. SDBL эмитит диагностику из двух мест: виртуальная таблица в `FROM` при наличии `JOIN` (`from_clause.rs`) и виртуальная таблица как `data_source` самой `JOIN`-секции (`join_clause.rs`).

## Что покрыто

Покрыты `LEFT`, `RIGHT`, вложенные join, несколько виртуальных таблиц и случаи, когда виртуальная таблица стоит в `FROM` без join и не должна срабатывать.

## Пробелы и ограничения

Диагностика не отличает опасные и приемлемые варианты по объему данных, параметрам виртуальной таблицы или фактическому плану. Нет подсказки по конкретному переписыванию.

## Может ли инфраструктура улучшить качество

Да, если SDBL HIR будет отдавать тип источника, параметры виртуальной таблицы и контекст соединения. Для рекомендаций потребуется отдельная база паттернов переписывания запросов.

## Возможное объединение

Очень близко к `JoinWithSubQuery`: оба правила покрывают `#std655` и являются query join performance. Можно рассмотреть общий internal handler для join-source restrictions, но публичные коды стоит оставить раздельными.

## Вывод

Покрытие базового паттерна хорошее. Главный предел - отсутствие семантики стоимости запроса и actionable rewrite.

## Закрыто Track 2

**Phase C §4 audit pass (task #84, 2026-05):** Track A — без изменений.
