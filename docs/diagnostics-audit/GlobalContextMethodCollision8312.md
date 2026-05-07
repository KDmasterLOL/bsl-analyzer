# GlobalContextMethodCollision8312

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Пользовательские методы не должны совпадать с методами глобального контекста,
добавленными в 8.3.12 для побитовых операций.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/global_context_method_collision8312.rs`
- `crates/hir-def/src/body/lower/mod.rs`
- `crates/ide-diagnostics/docs/ru/GlobalContextMethodCollision8312.md`
- `docs/legal/diagnostics/GlobalContextMethodCollision8312.md`

## Как реализовано

HIR lowering эмитит diagnostic для function/procedure names из списка bitwise
API. Handler ставит blocker diagnostic на имя метода.

## Что покрыто

Тесты проверяют все 20 русских/английских имен, prefix/suffix negative и
case-insensitive matching.

## Пробелы и ограничения

- Список зашит как `COLLISION_METHODS` в `hir-def/body/lower/mod.rs`; крейт
  `bsl-platform` / `platform_data.json` не консультируется, хотя именно он —
  канонический источник platform API.
- Нет quick-fix rename.
- Не проверяется конфликт экспортных методов common modules с global context в
  qualified/unqualified resolution отдельно.

## Может ли инфраструктура улучшить качество

Единый platform API registry по версиям: introduced globals, deprecated API,
collisions, replacements.

## Возможное объединение

Внутренне с deprecated/collision platform API registry. Внешне оставить
отдельным из-за compatibility mode `8.3.12` и blocker severity.

## Вывод

Покрытие списка хорошее. Основной долг - data-driven platform API registry и
rename support.

