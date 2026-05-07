# ProcedureReturnsValue

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит `Возврат` со значением внутри процедуры.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/procedure_returns_value.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/ProcedureReturnsValue.md`

## Как реализовано

Срабатывание формируется в HIR; handler создает blocker diagnostic на диапазон `RETURN_STMT`.

## Что покрыто

Покрыты простые и вложенные return expressions в процедурах. `Возврат;` и return value в функции не диагностируются.

## Пробелы и ограничения

Нет quick fix: удалить значение после `Возврат` можно не всегда безопасно, потому что код мог быть ошибочно объявлен процедурой вместо функции.

## Может ли инфраструктура улучшить качество

Да. Code action может предложить два варианта: удалить значение или преобразовать процедуру в функцию с обновлением вызовов.

## Возможное объединение

Близко к `FunctionShouldHaveReturn`, `AllFunctionPathMustHaveReturn`, `FunctionReturnsSamePrimitive`. Общий return-flow analyzer уже напрашивается.

## Вывод

Точная синтаксико-семантическая ошибка. Улучшение - не в detection, а в refactoring/fix вариантах.
