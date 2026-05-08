# FormDataToValue

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

`ДанныеФормыВЗначение()` в методах с контекстом формы нежелателен; `#std409`
рекомендует более безопасные способы вроде `РеквизитФормыВЗначение()`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/form_data_to_value.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/FormDataToValue.md`
- `docs/legal/diagnostics/FormDataToValue.md`
- `<v8std mirror>/docs/std/409.md`

## Как реализовано

HIR lowering ловит global и qualified calls `ДанныеФормыВЗначение` /
`FormDataToValue`, если текущий метод не помечен `БезКонтекста`.

## Что покрыто

Тесты проверяют qualified/global calls, server annotation, no-context
annotations, English variants и case-insensitive behavior.

## Пробелы и ограничения

- Не проверяется module type формы; безконтекстность определяется по annotation.
- Qualified calls ловятся по method name, без доказательства типа receiver.
- Нет quick-fix, потому что замена требует знания реквизита формы и типа.

## Может ли инфраструктура улучшить качество

Нужен execution/form context service и type-aware receiver resolution. Для
fix-плана нужен metadata-aware анализ реквизитов формы.

## Возможное объединение

Внутренне с `GetFormMethod`, `UsingSynchronousCalls`, `UsingModalWindows` как
form/platform API policy. Внешне оставить отдельным из-за стандарта `#std409`.

## Вывод

Детектор покрывает основной паттерн, но без type/context metadata остается
эвристическим.

