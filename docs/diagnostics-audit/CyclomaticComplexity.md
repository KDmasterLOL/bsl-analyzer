# CyclomaticComplexity

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Ограничивает цикломатическую сложность метода. Это метрика McCabe: базовая
сложность плюс decision points. Прямой v8std standard не найден; правило
метрическое.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/cyclomatic_complexity.rs`
- `crates/hir-def/src/cyclomatic_complexity.rs`
- `docs/diagnostics-audit/CognitiveComplexity.md`
- `crates/ide-diagnostics/docs/ru/CyclomaticComplexity.md`
- `docs/legal/diagnostics/CyclomaticComplexity.md`

## Как реализовано

HIR layer считает complexity, handler применяет `complexityThreshold`
(default `20`) и диагностирует имя метода.

## Что покрыто

Есть тесты на простую функцию, counting else, high complexity и прямой расчет
complexity.

## Пробелы и ограничения

- Нужно синхронизировать список decision points с реальным HIR calculator:
  документация обещает logical ops, goto, except, ternary. Калькулятор
  дополнительно учитывает `PreprocIf` (#Если/#ИначеЕсли/#Иначе) как decision
  points, но это не отражено в rule-доке.
- Калькулятор стартует с `complexity = 1` и не добавляет `+1` за саму
  процедуру/функцию, хотя rule-док явно перечисляет `Процедура/Функция` как
  decision point.
- Нет набора unit-тестов на каждый decision point отдельно.
- Есть overlap с `CognitiveComplexity`, `IfConditionComplexity`,
  `NestedStatements`, `MethodSize`.
- Нет baseline/suppression для legacy methods с высокой сложностью.

## Инфраструктурные улучшения

Создать общий metrics visitor, который за один проход считает cyclomatic,
cognitive, nesting, condition complexity и code lens values.

## Возможное объединение

С `CognitiveComplexity` сливать внешне не стоит: метрики отвечают на разные
вопросы. Но реализация должна быть общей, чтобы decision points не расходились.

## Вывод

Правило полезное, но нужно доаудировать сам calculator и покрыть каждый
decision point отдельным тестом.

