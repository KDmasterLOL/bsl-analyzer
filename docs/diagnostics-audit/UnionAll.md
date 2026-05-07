# UnionAll

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит `ОБЪЕДИНИТЬ` / `UNION` без `ВСЕ` / `ALL`, чтобы избежать лишнего удаления дублей.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/union_all.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/UnionAll.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/434.md`

## Как реализовано

SDBL dispatch для `UnionWithoutAll`; handler мапит диапазон в BSL и дает сообщение “используйте ОБЪЕДИНИТЬ ВСЕ”.

## Что покрыто

Покрыты русские/английские запросы, batch queries и несколько `UNION` в одном файле.

## Пробелы и ограничения

Не анализируется, нужно ли удаление дублей по бизнес-смыслу. Нет fix для добавления `ВСЕ` / `ALL`.

## Может ли инфраструктура улучшить качество

Да. Можно добавить safe quick fix, но оставлять diagnostic как recommendation, потому что семантика дублей может быть намеренной.

## Возможное объединение

Близко к query performance diagnostics (`SelectTopWithoutOrderBy`, `LogicalOr*`, `JoinWith*`). Общий SDBL fix framework пригодится.

## Вывод

Простая и полезная query performance проверка, но автоматическое исправление должно учитывать возможное изменение результата.
