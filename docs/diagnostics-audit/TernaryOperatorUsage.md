# TernaryOperatorUsage

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Запрещает любое использование тернарного оператора `?(...)`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/ternary_operator_usage.rs`
- `<v8std mirror>/docs/diagnostics/bslls/TernaryOperatorUsage.md`

## Как реализовано

HIR lowering передает range каждого ternary expression. Диагностика выключена по умолчанию.

## Что покрыто

Покрыты простые и вложенные тернарные операторы; nested case дает diagnostic на внешний и внутренний оператор.

## Пробелы и ограничения

Это грубая policy-диагностика: не различает короткие очевидные выражения и сложные. Нет fix для разворачивания в `Если`.

## Может ли инфраструктура улучшить качество

Да. Нужен code action “expand ternary to if” и связь с expression complexity.

## Возможное объединение

Близко к `NestedTernaryOperator`, `UselessTernaryOperator`, `IfConditionComplexity`. Можно объединить ternary AST/HIR analyzer.

## Вывод

Правило полезно для строгих code style профилей, но по умолчанию отключено из-за широты запрета.
