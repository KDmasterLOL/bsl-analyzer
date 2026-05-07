# IncorrectUseLikeInQuery

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

В запросах оператор `ПОДОБНО` / `LIKE` должен использовать шаблон-строку,
параметр или допустимое выражение, а не произвольное поле. Основание -
`#std726`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/incorrect_use_like_in_query.rs`
- `crates/sdbl-hir`
- `crates/ide-diagnostics/docs/ru/IncorrectUseLikeInQuery.md`
- `docs/legal/diagnostics/IncorrectUseLikeInQuery.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/726.md`

## Как реализовано

SDBL HIR эмитит `SdblDiagnostic::IncorrectUseLikeInQuery`; handler мапит range
в BSL и возвращает generic message.

## Что покрыто

Тесты проверяют большую fixture с SELECT/JOIN/WHERE contexts, корректные
literal/parameter/function cases и некорректный column reference.

## Пробелы и ограничения

- Сообщение слишком общее: не говорит, почему конкретный operand недопустим.
- Нет quick-fix, потому что нужно выбрать параметр/литерал/функцию.
- Все качество зависит от SDBL parser и expression classifier.

## Может ли инфраструктура улучшить качество

В SDBL diagnostics хранить kind нарушения: поле справа, параметр слева с
полем справа, join condition и т.п. Handler сможет выдавать точные messages.

## Возможное объединение

Внутренне с query correctness diagnostics. Внешне отдельный код нужен для
стандарта `#std726`. Прямой пересекается с `UsingLikeInQuery`: оба правила
эмитятся из одного site в `sdbl-hir/.../predicates.rs` с одинаковым range, поэтому
column-ref LIKE триггерит обе диагностики; `IncorrectUseLikeInQuery` —
строгое подмножество и должно подавлять более общую при включении обоих.

## Вывод

Детектор покрывает много контекстов, но UX сообщения нужно сделать
предметным.

