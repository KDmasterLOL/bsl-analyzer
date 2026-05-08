# GetFormMethod

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

`ПолучитьФорму()` / `GetForm()` часто лучше заменить на `ОткрытьФорму()` /
`OpenForm()` согласно `#std404`. Текущая scope шире прямого текста стандарта.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/get_form_method.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/GetFormMethod.md`
- `docs/legal/diagnostics/GetFormMethod.md`
- `<v8std mirror>/docs/std/404.md`

## Как реализовано

HIR lowering ловит global и object method calls с именем `ПолучитьФорму` /
`GetForm`. Handler формирует message с рекомендацией `ОткрытьФорму()`.

## Что покрыто

Тесты проверяют global/object calls, русские/английские variants,
case-insensitive, multiple calls и negative с `ОткрытьФорму`.

## Пробелы и ограничения

- `ПолучитьФорму` не всегда заменяется на `ОткрытьФорму`: иногда нужен объект
  формы до открытия.
- Пересекается с `DeprecatedMethods8317`, где `ПолучитьФорму` тоже есть в
  deprecated replacement map.
- Нет context-aware suggestions и quick-fix.

## Может ли инфраструктура улучшить качество

Нужен platform API policy registry с приоритетами diagnostics и context-aware
replacement.

## Возможное объединение

Нужно решить дублирование с `DeprecatedMethods8317`: либо suppress по
приоритету, либо один registry item порождает один diagnostic code.

## Вывод

Детектор широкий и хорошо покрыт, но remediation может быть неверной без
контекста. Дублирование с deprecated rule требует решения.

