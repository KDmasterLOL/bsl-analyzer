# CreateQueryInCycle

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Выполнение `Запрос.Выполнить()` и аналогичных query-like объектов внутри
цикла приводит к многократным однотипным запросам. Основание - `#std436`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/create_query_in_cycle.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/CreateQueryInCycle.md`
- `docs/legal/diagnostics/CreateQueryInCycle.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/436.md`

## Как реализовано

Lowering tracks loop depth and variables assigned from `Новый Запрос`,
`ПостроительЗапроса`, `ПостроительОтчета` / English variants. Внутри loop
diagnostic эмитится при вызове `.Выполнить()`/`.Execute()` на tracked receiver.

## Что покрыто

Тесты проверяют query в цикле, query созданный до цикла, English keywords,
case-insensitive name и query builder.

## Пробелы и ограничения

- Track идет по assignment и copy by simple path; не покрыты returns from
  factory methods, fields, arrays, parameters typed as query.
- Не различается безопасный сценарий, где loop выполняется один раз.
- Нет анализа текста запроса: однотипность, параметры, временные таблицы.
- Range стоит на весь call expression, но message предлагает перенос запроса,
  что не всегда корректно для `УстановитьПараметр` внутри цикла.

## Инфраструктурные улучшения

Нужен lightweight dataflow/type tracking для query-like значений и отдельный
query-performance layer, который сможет видеть текст запроса и параметры.

## Возможное объединение

Близко к будущим performance diagnostics по запросам, но внешний код лучше
сохранить. Внутренне объединить с query analyzer, не с generic loop rules.

## Вывод

Сейчас ловится частый и опасный паттерн, но coverage зависит от простого
tracking переменных. Улучшение требует dataflow и query context.

