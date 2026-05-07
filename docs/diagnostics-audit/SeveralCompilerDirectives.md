# SeveralCompilerDirectives

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Запрещает более одной директивы компиляции у переменной модуля или метода.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/several_compiler_directives.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/SeveralCompilerDirectives.md`

## Как реализовано

Через `item_tree` проверяет `annotations.len() > 1` у процедур, функций и переменных; diagnostic ставится на имя.

## Что покрыто

Покрыты переменные, процедуры, функции, комментарии между директивами и сортировка diagnostics по позиции.

## Пробелы и ограничения

Не проверяется конфликтность директив, только количество. Нет fix для выбора нужной директивы.

## Может ли инфраструктура улучшить качество

Да. Нужна модель compiler directive semantics: взаимоисключающие, допустимые комбинации, platform compatibility.

## Возможное объединение

Близко к `CompilationDirectiveLost`, `CompilationDirectiveNeedLess`, `UnknownPreprocessorSymbol`. Общий preprocessor/directive analyzer нужен.

## Вывод

Правило простое и надежное по синтаксису, но не объясняет, какую директиву оставить.
