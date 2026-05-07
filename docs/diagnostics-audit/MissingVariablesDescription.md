# MissingVariablesDescription

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что объявления переменных модуля имеют описание в комментарии.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_variables_description.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/MissingVariablesDescription.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/455.md`

## Как реализовано

Через `item_tree` выбираются top-level `ModItem::Variable`. По AST находится соответствующий `VAR_DEF`, затем `syntax::has_variable_description` проверяет inline/header comments с учетом аннотаций. Для экспортной переменной диапазон включает `Экспорт`.

## Что покрыто

Покрыты описания в той же строке, комментарии над объявлением, экспортные переменные и объявления с аннотациями.

## Пробелы и ограничения

Проверяется наличие комментария, но не его содержательность. Локальные переменные не входят в scope. Формат описания переменных отделен от общего docs model методов.

## Может ли инфраструктура улучшить качество

Да. Можно унифицировать комментарии переменных с documentation parser и добавить минимальную проверку “не пустое/не шаблонное описание”.

## Возможное объединение

Близко к `MissingParameterDescription`, `MissingReturnedValueDescription`, `PublicMethodsDescription`, а также к region diagnostics из `#std455`. Лучше объединить docs/comment infrastructure.

## Вывод

Правило закрывает требование структуры модуля, но пока проверяет только факт комментария, не качество описания.
