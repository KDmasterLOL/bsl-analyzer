# DeprecatedTypeManagedForm

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

`Тип("УправляемаяФорма")` / `Type("ManagedForm")` устарел с 8.3.14; нужно
использовать `ФормаКлиентскогоПриложения` /
`ClientApplicationForm`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deprecated_type_managed_form.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/DeprecatedTypeManagedForm.md`
- `docs/legal/diagnostics/DeprecatedTypeManagedForm.md`

## Как реализовано

HIR lowering распознает global call `Тип`/`Type`, берет первый строковый
аргумент, unescape'ит двойные кавычки и сравнивает с deprecated type name.
Diagnostic ставится на string token.

## Что покрыто

Тесты проверяют русское/английское имя, обычную строку вне `Тип`, case
insensitive variants и mixed examples.

## Пробелы и ограничения

- Не покрыты константные переменные: `ИмяТипа = "УправляемаяФорма"; Тип(ИмяТипа)`.
- Нет quick-fix замены string literal.
- Проверяется только первый argument и только literal token.
- Deprecated type registry отделен от остальных deprecated API.

## Инфраструктурные улучшения

Добавить constant folding для простых string constants и общий registry
deprecated type names.

## Возможное объединение

Внутренне объединить с platform deprecated registry, но оставить отдельный код:
это не method call, а deprecated type literal.

## Вывод

Реализация точная для literal case и хорошо покрыта. Следующий шаг -
quick-fix и registry.

