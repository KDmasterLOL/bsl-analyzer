# NestedFunctionInParameters

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит вложенные вызовы функций/методов и параметризованные конструкторы, переданные как аргументы других вызовов или конструкторов.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/nested_function_in_parameters.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/NestedFunctionInParameters.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/640.md`

## Как реализовано

HIR-обход по `Expr::Call`, `Expr::MethodCall`, `Expr::New`. Для имени diagnostic используется hybrid AST lookup. Однострочные вызовы пропускаются всегда, независимо от `allowOneliner`; при `allowOneliner=true` дополнительно требуется хотя бы один параметр, разнесённый на несколько строк. Конфиг: `allowOneliner` и `allowedMethodNames`, дефолтно разрешены `НСтр`, `NStr`, `ПредопределенноеЗначение`, `PredefinedValue`.

## Что покрыто

Покрыты вложенные обычные вызовы, method calls, constructors, one-liner исключение и allow-list методов.

## Пробелы и ограничения

Readability эвристика сильно зависит от `allowOneliner`: короткие, но сложные выражения могут быть разрешены. Нет учета сложности вложенного выражения и нет fix для “extract local variable”.

## Может ли инфраструктура улучшить качество

Да. Нужны expression complexity metrics и safe extract-variable code action. Тогда правило сможет различать допустимые простые вызовы и реально тяжелые вложения.

## Возможное объединение

Близко к `NestedConstructorsInStructureDeclaration`, `NestedTernaryOperator`, `NestedStatements`, `IfConditionComplexity`. Возможен общий readability/nesting analyzer.

## Вывод

Правило покрывает важный стиль кода, но сейчас работает как синтаксическая эвристика без оценки фактической сложности выражения.
