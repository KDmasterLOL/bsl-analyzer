# UnreachableCode

Статус: `done`, `needs-code-work`
Track 1 closure: foundation `819945b7`, O `47133aeb` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Находит недостижимые участки кода после терминаторов управления потоком.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unreachable_code.rs`
- `<v8std mirror>/docs/diagnostics/bslls/UnreachableCode.md`

## Как реализовано

Строит CFG для методов и module-level code, вычисляет reachable vertices от entry, отдельно выделяет локально недостижимые блоки, собирает ranges и объединяет соседние диапазоны.

## Что покрыто

Покрыты методы, код модуля, слияние смежных unreachable ranges и дедупликация в `lib.rs`.

## Пробелы и ограничения

Точность зависит от CFG и знания терминаторов. Constant conditions и platform-specific preprocessor paths могут требовать отдельной обработки.

## Может ли инфраструктура улучшить качество

Да. Связать CFG с constant folding/preprocessor evaluation и добавить safe delete unreachable block fix.

## Возможное объединение

Близко к `AllFunctionPathMustHaveReturn`, `FunctionShouldHaveReturn`, `TooManyReturns`. Общий CFG/control-flow diagnostics слой уже используется и должен развиваться.

## Вывод

Это сильная CFG-диагностика; дальнейшее качество зависит от richer control-flow semantics.
