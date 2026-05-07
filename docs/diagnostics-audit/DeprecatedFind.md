# DeprecatedFind

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Глобальный `Найти()` / `Find()` устарел с 8.3.6; для строк нужно использовать
`СтрНайти()` / `StrFind()`, а для коллекций - методы коллекций.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deprecated_find.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/DeprecatedFind.md`
- `docs/legal/diagnostics/DeprecatedFind.md`

## Как реализовано

HIR lowering диагностирует только global `IDENT` calls. Qualified calls
`Массив.Найти()` исключаются на уровне формы callee.

## Что покрыто

Тесты покрывают русское/английское имя, collection method exclusion,
case-insensitive calls и module-level call.

## Пробелы и ограничения

- Replacement `СтрНайти` корректен для строк, но без type inference нельзя
  доказать, что аргументы строковые.
- Нет quick-fix.
- Compatibility mode есть (`8.3.6`), но общий registry устаревших методов не
  используется.

## Инфраструктурные улучшения

Deprecated registry должен хранить conditional replacement: если args/string
types известны - `СтрНайти`, иначе generic warning.

## Возможное объединение

Внутренне объединить с другими deprecated global calls. Внешне код стоит
оставить отдельным, потому что это популярная миграционная проблема.

## Вывод

Правило точное по форме вызова, но рекомендация без type info может быть
слишком уверенной.

