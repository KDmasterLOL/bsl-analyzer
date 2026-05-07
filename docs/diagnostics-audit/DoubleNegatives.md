# DoubleNegatives

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Двойные отрицания ухудшают читаемость: `Не Не X`, `Не (X <> Y)`,
`Не X <> Y`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/double_negatives.rs`
- `crates/ide-diagnostics/docs/ru/DoubleNegatives.md`
- `docs/legal/diagnostics/DoubleNegatives.md`

## Как реализовано

Есть node-based API для single-pass collection и отдельный full `check()`.
Обе версии ищут `UNARY_EXPR`/`BINARY_EXPR`, наличие `NOT`/`<>` token и
фильтруют выражения с logical operators.

## Что покрыто

Тесты в handler проверяют основные паттерны, отрицательные cases и часть
фильтров на сложные выражения.

## Пробелы и ограничения

- Две реализации (`*_simple` и `*_optimized`) могут расходиться.
- Фильтр `contains_logical_operators` может пропускать реальные двойные
  отрицания внутри больших условий.
- Нет HIR expression-level нормализации, поэтому анализ зависит от CST формы.
- Сообщение на английском в русскоязычной диагностике.
- Нет quick-fix упрощения выражения.

## Инфраструктурные улучшения

Свести к одному expression analyzer на HIR/AST adapter и добавить rewrite rules
с проверкой precedence.

## Возможное объединение

Близко к `IdenticalExpressions`, `IfConditionComplexity`,
`NestedTernaryOperator`: все анализируют выражения. Внешне не объединять, но
внутренне нужен общий expression-quality pass.

## Вывод

Правило полезное, но текущая двойная реализация и грубый filter создают риск
рассинхрона и пропусков.

