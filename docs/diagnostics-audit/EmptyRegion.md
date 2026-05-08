# EmptyRegion

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Пустые области не добавляют структуру модулю и противоречат смыслу областей из
`#std455`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/empty_region.rs`
- `crates/hir-def/src/body/lower/preproc.rs`
- `crates/ide-diagnostics/docs/ru/EmptyRegion.md`
- `docs/legal/diagnostics/EmptyRegion.md`
- `<v8std mirror>/docs/std/455.md`

## Как реализовано

HIR lowering по preprocessor regions эмитит `BodyDiagnostic::EmptyRegion`.
Handler ставит diagnostic на range всей области и включает имя области в
message.

## Что покрыто

Тесты проверяют comment-only region, nested empty regions, области с переменной
или функцией, русские и английские директивы.

## Пробелы и ограничения

- Нет quick-fix удаления пустой области вместе с директивами.
- Пустая область с `TODO` или намеренным комментарием все равно считается
  пустой.
- Region policy отделен от `DuplicateRegion`, `NonStandardRegion`,
  `CodeOutOfRegion`.

## Может ли инфраструктура улучшить качество

Нужен общий `RegionPolicy` и range utilities для safe deletion, merged regions
и module-type-specific region expectations.

## Возможное объединение

Внутренне объединить с остальными region diagnostics: `DuplicateRegion`,
`NonStandardRegion`, `CodeOutOfRegion`, `NonExportMethodsInApiRegion`,
`CommonModuleMissingAPI`. Внешне код стоит оставить отдельным: remediation
отличается от duplicate/non-standard region.

## Вывод

Правило корректно ловит типовой мусор, но должно использовать общий region
слой и получить quick-fix удаления.

