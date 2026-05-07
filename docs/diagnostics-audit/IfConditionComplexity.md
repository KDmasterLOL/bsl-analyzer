# IfConditionComplexity

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Сложные условия `Если` с большим числом boolean operations нужно упрощать или
выносить в именованные переменные.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/if_condition_complexity.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/ide-diagnostics/docs/ru/IfConditionComplexity.md`
- `docs/legal/diagnostics/IfConditionComplexity.md`

## Как реализовано

HIR lowering считает complexity условия и эмитит candidate с default threshold.
Handler повторно применяет пользовательский `maxIfConditionComplexity`
(default `3`).

## Что покрыто

Тесты проверяют simple condition, at-threshold, complex condition, `ИначеЕсли`,
English keywords и multi-line cases.

## Пробелы и ограничения

- Complexity model нужно синхронизировать с `CyclomaticComplexity` и
  `CognitiveComplexity`.
- Candidate эмитится на default threshold, а handler фильтрует по config; при
  очень низком config HIR может не эмитить нужный candidate, если lowering
  threshold выше пользовательского.
- Нет quick-fix extract variable.

## Может ли инфраструктура улучшить качество

Общий metrics visitor должен считать condition complexity независимо от
diagnostic threshold; handler применяет config.

## Возможное объединение

Внутренне объединить с metrics diagnostics. Внешне оставить отдельным: это
локальная сложность условия, а не метода.

## Вывод

Правило полезное, но threshold нужно применять после полного расчета, а не
частично в lowering.

