# MissingCodeTryCatchEx

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит `Попытка...Исключение...КонецПопытки`, где блок `Исключение` пустой.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_code_try_catch_ex.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/MissingCodeTryCatchEx.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/499.md`

## Как реализовано

HIR-обход по всем телам методов и коду модуля. Для `Stmt::Try` проверяется `except.is_empty()`. Если `commentAsCode=true`, обработчик через AST fallback ищет комментарии в `EXCEPT_CLAUSE` и может считать их содержимым блока.

## Что покрыто

Покрыты вложенные try/catch, module-level code и настройка `commentAsCode`.

## Пробелы и ограничения

Комментарий как код - спорная эвристика: комментарий может объяснять намеренный swallow, но не делает обработку исключения. Нет проверки качества кода в `Исключение`: один пустой `Сообщить` или бессмысленный `Возврат` уже подавит диагностику.

## Может ли инфраструктура улучшить качество

Да. Нужна отдельная классификация catch-body actions: логирование, повторный `ВызватьИсключение`, rollback, ignored exception. Это пересекается с exception-handling семейством.

## Возможное объединение

Близко к `TryNumber`, `UsageWriteLogEvent`, `BeginTransactionBeforeTryCatch`, `CommitTransactionOutsideTryCatch`, `WrongUseOfRollbackTransactionMethod`. Можно строить общий exception handling analyzer.

## Вывод

Диагностика хорошо ловит самый грубый случай, но качество обработки исключений шире, чем “пустой/не пустой блок”.
