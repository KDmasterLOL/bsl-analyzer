# UnresolvedField

Статус: `done`, `needs-code-work`
Track 1 closure: G1 `27fb95ec`, G2 `1e5230fd` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Находит обращение к полю, которого нет у известного типа receiver.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unresolved_field.rs`
- `crates/ide-diagnostics/src/hir_inference_dispatch.rs`
- `crates/hir-ty/src/infer.rs` (emit на miss field lookup, контракт молчания на Unknown/union/примитивах)
- `crates/ide/tests/infer_field_lookup.rs`, `infer_this_object.rs` (контракт молчания и narrowing)

## Как реализовано

Inference diagnostic передает `receiver_ty`, `field_name` и range. Handler формирует сообщение с display name типа.

## Что покрыто

Покрыты high-confidence typed receivers, включая metadata ref из doc comments; тесты фиксируют module-level code и методы. Контракт молчания зафиксирован тестами: Unknown receiver, union до narrowing и примитивы не диагностируются.

## Пробелы и ограничения

Неразрешенные/unknown receiver types не диагностируются. Нет suggestions для похожих полей.

## Может ли инфраструктура улучшить качество

Да. Нужны точнее type inference, metadata fields database и fuzzy field suggestions.

## Возможное объединение

Близко к `UnresolvedMethodCall`, `TypeMismatch`, `ReadOnlyPropertyAssignment`. Это typed inference diagnostics.

## Вывод

Правило должно быть точным, потому что emission консервативный. Покрытие растет вместе с inference.
