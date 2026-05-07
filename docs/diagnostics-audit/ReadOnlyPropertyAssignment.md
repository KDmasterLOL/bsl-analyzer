# ReadOnlyPropertyAssignment

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит присваивания платформенным свойствам, доступным только для чтения.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/read_only_property.rs`
- `crates/ide-diagnostics/src/hir_inference_dispatch.rs`
- `crates/hir-ty/src/infer.rs` (Stmt::Assign — Path/Field arms emитят `InferenceDiagnostic::ReadOnlyPropertyAssignment`)

## Как реализовано

Диагностика приходит из inference (две точки эмита в `Stmt::Assign`: `Expr::Path` через `form_self::resolve_form_self_property`, `Expr::Field` через `form_items::lookup_form_item_field` / `field_lookup::lookup_field`). Handler строит сообщение по типу receiver и имени свойства.

## Что покрыто

Покрыты только случаи, где тип receiver и свойство успешно разрешены по платформенной базе.

## Пробелы и ограничения

Качество зависит от актуальности платформенной базы/HBK. Нет fix, потому что правильная замена зависит от конкретного свойства.

## Может ли инфраструктура улучшить качество

Да. Нужна актуальная typed platform API database и, для некоторых свойств, mapping на корректные setter methods.

## Возможное объединение

Близко к `TypeMismatch`, `UnresolvedField`, `UnresolvedMethodCall`: typed semantic diagnostics. Общий inference diagnostics UX желателен.

## Вывод

Правило точное при наличии корректных типов, но покрытие ограничено возможностями inference и платформенной базы.
