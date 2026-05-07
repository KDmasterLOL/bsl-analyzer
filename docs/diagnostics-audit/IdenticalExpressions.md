# IdenticalExpressions

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Идентичные выражения по обе стороны бинарного оператора часто указывают на
ошибку: `x = x`, `a - a`, `a > a`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/identical_expressions.rs`
- `crates/ide-diagnostics/docs/ru/IdenticalExpressions.md`
- `docs/legal/diagnostics/IdenticalExpressions.md`

## Как реализовано

Гибрид: основной анализ по HIR сравнивает выражения семантически, есть AST
fallback для выражений, разорванных препроцессором. Есть исключения для
сложения/умножения, популярных делителей и нормализация транзитивных сравнений
в logical chains.

## Что покрыто

Тесты покрывают сравнения/арифметику, исключения, logical chains,
case/parentheses/whitespace normalization и fallback для preprocessor split.

## Пробелы и ограничения

- Две реализации (HIR и AST fallback) повышают риск рассинхрона.
- Semantic equality не равна полной value equivalence: aliases, constants и
  function purity не анализируются.
- Popular divisors и другие exceptions являются policy, которую лучше вынести в
  конфиг/данные.

## Может ли инфраструктура улучшить качество

Общий expression canonicalizer с configurable exceptions и adapter для AST
fallback только там, где HIR реально теряет структуру.

## Возможное объединение

Внутренне с `DoubleNegatives`, `IfElseDuplicatedCondition`,
`IfConditionComplexity` через expression-quality analyzer. Внешне оставить.

## Вывод

Одна из более зрелых expression diagnostics, но гибридность нужно держать под
контролем тестами.

