# RedundantAccessToObject

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит избыточное обращение к текущему объекту через `ЭтотОбъект` или имя текущего модуля/менеджера.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/redundant_access_to_object.rs`
- `crates/hir-def/src/body/lower/expr.rs` (lowering эмитит `ThisObject` и `ThreeLevel`)
- `crates/hir-ty/src/infer.rs` (inference эмитит `RedundantAccessToObjectTwoLevel`, конвертируется в `TwoLevel` в `hir_inference_dispatch.rs`)
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/RedundantAccessToObject.md`

## Как реализовано

`ThisObject` и `ThreeLevel` эмитит lowering (`hir-def/body/lower/expr.rs`), `TwoLevel` (CommonModule self-call) — inference после `user_common_module_exists`. Handler валидирует kind по metadata: object/form/recordset для `ЭтотОбъект`, common module с `ReturnValueReuse::DontUse`, manager module с совпадением MDO type/name.

## Что покрыто

Покрыты `ThisObject`, двухуровневый common-module self access и трехуровневый manager access. Есть конфиги `checkObjectModule`, `checkFormModule`, `checkRecordSetModule`.

## Пробелы и ограничения

Без metadata многие кандидаты подавляются. Нет fix для удаления префикса. Кэшируемые common modules исключаются, потому что полный путь может быть нужен.

## Может ли инфраструктура улучшить качество

Да. Нужны точные metadata contexts и auto-fix удаления безопасного префикса.

## Возможное объединение

Близко к `ThisObjectAssign` и `SelfAssign` только по работе с self/reference expressions. Лучше общий helper нормализации self-access. TwoLevel-ветка идёт через inference и фактически семейство typed inference diagnostics (`TypeMismatch`, `UnresolvedField`, `UnresolvedMethodCall`, `ReadOnlyPropertyAssignment`) — стоит держать общий contract молчания на Unknown receiver.

## Вывод

Хорошая semantic диагностика с осторожными guard-условиями, но ей не хватает safe fix.
