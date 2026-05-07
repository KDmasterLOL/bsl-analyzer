# FunctionNameStartsWithGet

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Имя функции не должно начинаться с `Получить`; по `#std647` имя должно
описывать возвращаемое значение. Правило отключено по умолчанию.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/function_name_starts_with_get.rs`
- `crates/hir-def/src/body/lower/mod.rs`
- `crates/ide-diagnostics/docs/ru/FunctionNameStartsWithGet.md`
- `docs/legal/diagnostics/FunctionNameStartsWithGet.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/647.md`

## Как реализовано

HIR lowering эмитит candidate для function names с русским prefix
`Получить`. Handler проверяет disabled state и формирует message.

## Что покрыто

Тесты проверяют русскую функцию с prefix, отсутствие prefix, case-insensitive,
procedure negative, English `Get` negative и partial match negative.

## Пробелы и ограничения

- English `Get` намеренно не проверяется, хотя проект может быть bilingual.
- Нет exceptions для устоявшихся API или generated code.
- Нет quick-fix rename, потому что нужно обновлять все references.

## Может ли инфраструктура улучшить качество

Нужен naming policy engine с language mode, exceptions и rename refactoring.

## Возможное объединение

Внутренне с function naming diagnostics (`BadWords`, `NumberOfParams`,
`OrderOfParams` только частично) через naming-policy layer. Внешне оставить.

## Вывод

Правило точное, но узкое. Основное улучшение - configurable language policy и
rename support.

