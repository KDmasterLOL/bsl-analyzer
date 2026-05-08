# TooManyReturns

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Ограничивает количество операторов `Возврат` в методе. Дефолт `maxReturnsCount` равен 3.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/too_many_returns.rs`
- `<v8std mirror>/docs/diagnostics/bslls/TooManyReturns.md`

## Как реализовано

HIR считает ranges возвратов и передает имя метода. Handler сравнивает count с конфигом и ставит diagnostic на имя метода. Диагностика выключена по умолчанию.

## Что покрыто

Покрыты процедуры/функции с разным количеством return и настраиваемый порог.

## Пробелы и ограничения

Не различаются guard clauses и запутанный flow. Большое число ранних выходов иногда улучшает читаемость.

## Может ли инфраструктура улучшить качество

Да. Связать с `CognitiveComplexity`, CFG и классификацией guard returns.

## Возможное объединение

Близко к `NestedStatements`, `CognitiveComplexity`, `AllFunctionPathMustHaveReturn`, `FunctionShouldHaveReturn`. Нужен общий return/control-flow metrics layer.

## Вывод

Полезно как строгая метрика, но поэтому и выключено по умолчанию.
