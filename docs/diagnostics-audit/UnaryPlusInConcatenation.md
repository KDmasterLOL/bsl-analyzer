# UnaryPlusInConcatenation

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит случайный унарный плюс внутри конкатенации строк.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unary_plus_in_concatenation.rs`
- `<v8std mirror>/docs/diagnostics/bslls/UnaryPlusInConcatenation.md`

## Как реализовано

HIR lowering определяет паттерн, handler создает blocker diagnostic на диапазон унарного `+`.

## Что покрыто

Покрыты `"Строка" + + "Строка"`, вложенные выражения и переменные; `+ 5` и числовые выражения не срабатывают.

## Пробелы и ограничения

Нет fix для удаления лишнего плюса, хотя он обычно безопасен.

## Может ли инфраструктура улучшить качество

Да. Добавить точечный fix удаления унарного `+`.

## Возможное объединение

Близко к `IncorrectUseOfStrTemplate`, `SelfAssign`, `IdenticalExpressions`: простые expression bug patterns.

## Вывод

Точная bug-диагностика, хороший кандидат на safe auto-fix.
