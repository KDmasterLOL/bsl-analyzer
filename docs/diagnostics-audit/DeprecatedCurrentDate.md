# DeprecatedCurrentDate

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Глобальный `ТекущаяДата()` / `CurrentDate()` считается устаревшим из-за
неоднозначности времени; рекомендуется `ТекущаяДатаСеанса()` /
`CurrentSessionDate()`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deprecated_current_date.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/DeprecatedCurrentDate.md`
- `docs/legal/diagnostics/DeprecatedCurrentDate.md`

## Как реализовано

HIR lowering эмитит diagnostic только для global call `IDENT`, не для
qualified method call. Handler формирует bilingual message.

## Что покрыто

Тесты есть на русское/английское имя, object method exclusion,
case-insensitive calls и несколько процедур.

## Пробелы и ограничения

- Нет quick-fix замены имени.
- Не учитывается compatibility/platform version.
- Не различается серверный/клиентский контекст, хотя рекомендация может
  отличаться.
- Список deprecated global methods размазан между несколькими diagnostics.

## Инфраструктурные улучшения

Перевести на общий deprecated global method registry с context-aware
replacement и quick-fix.

## Возможное объединение

Можно внутренне объединить с `DeprecatedFind`, `DeprecatedMessage`,
`DeprecatedMethods8310/8317`. Внешний код лучше оставить для точной severity и
документации.

## Вывод

Реализация аккуратно исключает method calls, но нуждается в auto-fix и общем
deprecated API registry.

