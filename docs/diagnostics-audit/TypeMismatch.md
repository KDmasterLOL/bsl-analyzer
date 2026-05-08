# TypeMismatch

Статус: `done`, `partial-emitter`
Track 1 closure: foundation `819945b7`, H `5028602a`, G1 `27fb95ec`, G2 `1e5230fd` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Должно сообщать о несоответствии ожидаемого и фактического типа выражения.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/type_mismatch.rs`
- `crates/ide-diagnostics/src/hir_inference_dispatch.rs`
- `crates/hir-ty/src/arg_diagnostics.rs` (live emitter для аргументов вызова)
- `crates/hir-ty/src/infer.rs`

## Как реализовано

Handler формирует сообщение по `expected.display_name` / `actual.display_name`. Live emitter уже работает для аргументов вызова через `arg_diagnostics_query` (с учётом narrowing), диспатчится через `collect_arg_diagnostics`. Общая assignability для присваиваний/возвратов (M4 Task 7, `is_assignable_to`) ещё не включена — комментарий в `type_mismatch.rs` устарел.

## Что покрыто

Покрыты несовпадения типов аргументов вызова (single + overloaded paths) с применением narrowing. Не покрыты присваивания, возвраты и другие assignment-контексты.

## Пробелы и ограничения

Главное ограничение — emit активен только для аргументов вызова. После расширения нужны правила assignability для присваиваний/возвратов, optional/union types и подавление каскадных ошибок.

## Может ли инфраструктура улучшить качество

Да, но это работа в `hir-ty`: нужно реализовать `is_assignable_to`, confidence levels и dedupe с `Unresolved*`.

## Возможное объединение

Близко к `ReadOnlyPropertyAssignment`, `UnresolvedField`, `UnresolvedMethodCall`, `MismatchedArgCount`. Это семейство typed inference diagnostics.

## Вывод

Сейчас активна только для аргументов вызова. До включения общего assignability emitter (присваивания, возвраты) считать покрытие частичным.
