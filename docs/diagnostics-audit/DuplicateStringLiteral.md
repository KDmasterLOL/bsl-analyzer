# DuplicateStringLiteral

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Повторяющиеся строковые литералы стоит выносить в именованные значения, чтобы
снизить риск несогласованных изменений.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/duplicate_string_literal.rs`
- `crates/ide-diagnostics/docs/ru/DuplicateStringLiteral.md`
- `docs/legal/diagnostics/DuplicateStringLiteral.md`

## Как реализовано

AST traversal собирает `LITERAL` nodes со string tokens. Scope - метод или весь
файл (`analyzeFile`). Настройки: `allowedNumberCopies`, `caseSensitive`,
`minTextLength`, `excludedMethods`. Diagnostic ставится на первое вхождение,
если count больше allowed.

## Что покрыто

Тесты покрывают duplicates в методе, case-insensitive grouping, min length,
threshold, excluded `Тип`/`Type`, file/method scope и часть config behavior.

## Пробелы и ограничения

- Длина считается по тексту literal вместе с кавычками, что неочевидно для
  пользователя.
- Multi-line/concatenated strings группируются по raw CST text, не по
  нормализованному значению.
- `excludedMethods` ищет ближайший `CALL_EXPR`/`NEW_EXPR`; вложенный literal в
  аргументе может исключиться шире, чем нужно.
- Нет quick-fix extract constant/variable.
- `activated_by_default = true`, хотя комментарий в коде говорит "Enabled by
  default: No" - нужно сверить metadata и docs.

## Инфраструктурные улучшения

Нужен literal value normalizer, precise call-argument relation и extract
constant code action. Конфиг должен показывать пользователю длину без кавычек
или явно документировать текущую модель.

## Возможное объединение

Близко к `MagicString`-подобным будущим правилам и `BadWords` только
поверхностно. Внешне не объединять; внутренне использовать общий literal
collector.

## Вывод

Правило достаточно функциональное, но есть рассинхрон комментария с metadata и
неочевидная модель длины/исключений.

