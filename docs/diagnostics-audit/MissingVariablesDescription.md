# MissingVariablesDescription

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что объявления переменных модуля имеют описание в комментарии.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_variables_description.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `<v8std mirror>/docs/diagnostics/bslls/MissingVariablesDescription.md`
- `<v8std mirror>/docs/std/455.md`

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

## Закрыто Track 2

**Phase B §5.1 Slice A (`55ce9dc0`) + §5.2 Slice B (`e289cec8`),
2026-05:** detection переехал на `hir_def::docs::VariableDocs`
(SymbolTree-owned, parallel `MethodDocs`); handler потребляет
`ctx.variable_docs(var_id)` через `AnalysisProvider`. Hyperlink-only
delegated-doc guard работает идентично `MissingParameterDescription`/
`MissingReturnedValueDescription` (см. en/ru card-level docs «Strict
semantic check»).
