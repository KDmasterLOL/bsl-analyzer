# WrongUseOfRollbackTransactionMethod

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что `ОтменитьТранзакцию()` / `RollbackTransaction()` вызывается первым оператором в блоке `Исключение`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/wrong_use_of_rollback_transaction_method.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/hir-def/src/body/lower/diagnostics.rs` (`is_global_rollback_transaction_call`, `check_rollback_transaction_in_try`)
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/WrongUseOfRollbackTransactionMethod.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/783.md`

## Как реализовано

HIR lowering анализирует statement list и try/catch структуру: глобальный `RollbackTransaction` вне `Исключение`, внутри `Попытка` или не первым оператором в exception block эмитит diagnostic. Квалифицированные вызовы игнорируются.

## Что покрыто

Покрыты outside try-catch, rollback в try-body, rollback не первым в exception block, русское/английское имя и корректный первый rollback в exception block.

## Пробелы и ограничения

Правило не связывает `RollbackTransaction` с конкретной парой `BeginTransaction`/`CommitTransaction` и не анализирует вложенные транзакционные сценарии глубоко. Квалифицированные вызовы исключены, что правильно для платформенного метода, но может скрывать wrapper-антипаттерны.

## Может ли инфраструктура улучшить качество

Да. Нужен общий transaction control-flow analyzer вместе с `BeginTransactionBeforeTryCatch`, `CommitTransactionOutsideTryCatch` и `PairingBrokenTransaction`.

## Возможное объединение

Сильный кандидат на объединение инфраструктуры с транзакционными diagnostics. Пользовательский код можно оставить отдельным, но анализ должен быть единым, чтобы избежать противоречивых срабатываний.

## Вывод

Локальная проверка позиции rollback работает, но полноценное качество требует единого анализа жизненного цикла транзакции.
