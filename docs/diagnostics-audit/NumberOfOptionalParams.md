# NumberOfOptionalParams

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Ограничивает количество необязательных параметров метода. Дефолт `maxOptionalParamsCount` равен 3.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/number_of_optional_params.rs`
- `<v8std mirror>/docs/diagnostics/bslls/NumberOfOptionalParams.md`
- `<v8std mirror>/docs/std/640.md`

## Как реализовано

Количество считает HIR, handler получает count и диапазон имени метода, сравнивает с конфигом.

## Что покрыто

Покрыты функции/процедуры, настройка порога, случаи на пороге и методы без optional parameters.

## Пробелы и ограничения

Нет учета API-стабильности: иногда optional параметры нужны для совместимости. Нет рекомендации заменить параметры структурой опций.

## Может ли инфраструктура улучшить качество

Да. Можно связывать с exported/public API и давать разные severity или рекомендации для публичных и внутренних методов.

## Возможное объединение

Близко к `NumberOfParams`, `OrderOfParams`, `MissedRequiredParameter`, `MismatchedArgCount`. Нужен общий method-signature policy layer.

## Вывод

Базовый счетчик работает, но правило остается грубой метрикой без анализа назначения метода.

## Закрыто Track 2

**Phase B §6.4 (commit `68f65fcd`, 2026-05):** detection переехал на
`HirMethodMetrics.optional_params_count` (§6.1).
