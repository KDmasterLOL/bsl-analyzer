# OrderOfParams

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит обязательные параметры, объявленные после необязательных.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/order_of_params.rs`
- `<v8std mirror>/docs/diagnostics/bslls/OrderOfParams.md`
- `<v8std mirror>/docs/std/640.md`

## Как реализовано

Через `item_tree` обходятся top-level функции/процедуры. После первого параметра с default value все обязательные параметры диагностируются по диапазону имени.

## Что покрыто

Покрыты отсутствие параметров, все обязательные, все необязательные, корректный порядок и несколько нарушений в одной сигнатуре.

## Пробелы и ограничения

Нет fix для перестановки параметров. Перестановка опасна: меняет ABI и все вызовы метода.

## Может ли инфраструктура улучшить качество

Частично. Для safe fix нужен project-wide rename/signature refactoring с обновлением всех вызовов.

## Возможное объединение

Близко к `NumberOfParams`, `NumberOfOptionalParams`, `MissedRequiredParameter`, `MismatchedArgCount`. Общий signature analyzer оправдан.

## Вывод

Диагностика точная, но автоматическое исправление возможно только при полноценном refactoring engine.
