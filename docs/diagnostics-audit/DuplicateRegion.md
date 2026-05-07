# DuplicateRegion

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Повтор top-level областей модуля нарушает структуру из `#std455`. Русские и
английские имена стандартных областей считаются эквивалентными.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/duplicate_region.rs`
- `crates/ide-diagnostics/docs/ru/DuplicateRegion.md`
- `docs/legal/diagnostics/DuplicateRegion.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/455.md`

## Как реализовано

Handler берет `ctx.module_level_regions()` и группирует по canonical name.
Для стандартных областей русские/английские aliases приводятся к одному
ключу. Для non-standard областей используется исходное имя.

## Что покрыто

Тесты покрывают standard RU/EN duplicates, no duplicates, nested region ignore,
case-insensitive standard names и case-sensitive non-standard names.

## Пробелы и ограничения

- Diagnostic ставится на первое вхождение, хотя обычно удалять нужно второе и
  последующие.
- Список canonical regions захардкожен отдельно от `NonStandardRegion`.
- Нет quick-fix merge/delete duplicate region.
- Не учитываются module-type-specific allowed regions.

## Инфраструктурные улучшения

Общий `RegionPolicy`: standard aliases, order, duplicates, allowed-by-module,
range utilities и fix planning.

## Возможное объединение

Внутренне объединить с `NonStandardRegion`, `EmptyRegion`, `CodeOutOfRegion`,
`CommonModuleMissingAPI`. Внешние codes оставить, потому что remediation
разная.

## Вывод

Реализация эффективная и покрытая, но region policy нужно централизовать.

