# ThisObjectAssign

Статус: `done`, `needs-code-work`
Track 1 closure: scope-included, no code change (non-shadowable contract preserved per plan §4.4) — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Запрещает прямое присваивание в `ЭтотОбъект` / `ThisObject`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/this_object_assign.rs`
- `<v8std mirror>/docs/diagnostics/bslls/ThisObjectAssign.md`

## Как реализовано

HIR lowering находит прямое lvalue `ЭтотОбъект`; handler создает blocker diagnostic.

## Что покрыто

Покрыты русское/английское имя и case-insensitive форма. Правило ограничено `CommonModule` и `FormModule` (metadata `modules`). Присваивание в свойство `ЭтотОбъект.Реквизит` не диагностируется.

## Пробелы и ограничения

Нет fix: правильное действие обычно зависит от намерения, например использовать `ЗначениеВРеквизитФормы`.

## Может ли инфраструктура улучшить качество

Да. Можно предлагать context-aware quick actions для формы, если известен form attribute.

## Возможное объединение

Близко к `ReadOnlyPropertyAssignment`, `RedundantAccessToObject`, `SelfAssign`. Общий lvalue/read-only property analyzer полезен.

## Вывод

Точная blocker-ошибка, но автоматическое исправление требует контекста формы/объекта.
