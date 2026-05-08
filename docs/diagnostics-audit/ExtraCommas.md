# ExtraCommas

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Лишние запятые в конце списка аргументов ухудшают читаемость и могут быть
ошибкой. Связанный стандартный контекст - `#std640`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/extra_commas.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/ExtraCommas.md`
- `docs/legal/diagnostics/ExtraCommas.md`
- `<v8std mirror>/docs/std/640.md`

## Как реализовано

HIR lowering находит trailing comma patterns в argument list и эмитит
`BodyDiagnostic::ExtraCommas`. Handler возвращает простой diagnostic.

## Что покрыто

Тесты покрывают одинарную и множественные trailing commas, qualified calls,
conditions, валидные empty аргументы внутри списка и empty call.

## Пробелы и ограничения

- Сообщение содержит грамматическую ошибку: "для параметры".
- Нет quick-fix удаления лишних запятых.
- Проверяется только call arg list; нужно сверить constructors/indexers, если
  синтаксис допускает похожие случаи.

## Может ли инфраструктура улучшить качество

Подключить syntax-aware edit builder и formatter tests.

## Возможное объединение

Внутренне с `EmptyStatement` и `IncorrectLineBreak` как formatting/syntax smell
rules. Внешне оставить отдельным.

## Вывод

Покрытие хорошее. Нужно исправить сообщение и добавить fix.

