# NestedTernaryOperator

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит вложенный тернарный оператор и тернарный оператор внутри условия `Если` / `ИначеЕсли`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/nested_ternary_operator.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `<v8std mirror>/docs/diagnostics/bslls/NestedTernaryOperator.md`

## Как реализовано

AST-обход по `IF_STMT`, `ELSIF_CLAUSE` и `TERNARY_EXPR`. Для условий ищутся все `TERNARY_EXPR` внутри condition. Для `TERNARY_EXPR` дополнительно ищутся вложенные `TERNARY_EXPR` в descendants.

## Что покрыто

Покрыты nested ternary в ветке другого тернарного оператора, тернарные выражения в `Если` и несколько срабатываний в одном условии.

## Пробелы и ограничения

Простая тернарная операция вне условия разрешена, даже если выражение длинное. Нет учета общей сложности condition и нет fix для переписывания в `Если`.

## Может ли инфраструктура улучшить качество

Да. Связка с expression complexity и code action “expand ternary to if” улучшит применимость. Для условий лучше использовать общий analyzer complexity.

## Возможное объединение

Близко к `TernaryOperatorUsage`, `UselessTernaryOperator`, `IfConditionComplexity`, `NestedStatements`. Можно объединить AST traversal для ternary family, но коды оставить раздельными.

## Вывод

Правило точно ловит самые читаемостно опасные формы тернарного оператора, но пока не помогает их исправлять.
