# WrongUseOfRollbackTransactionMethod

Статус: `done`, `needs-code-work`
Track 1 closure: scope-included, no code change (kept syntactic in `hir-def/body/lower` per plan §1.8) — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что `ОтменитьТранзакцию()` / `RollbackTransaction()` вызывается первым оператором в блоке `Исключение`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/wrong_use_of_rollback_transaction_method.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/hir-def/src/body/lower/diagnostics.rs` (`is_global_rollback_transaction_call`, `check_rollback_transaction_in_try`)
- `<v8std mirror>/docs/diagnostics/bslls/WrongUseOfRollbackTransactionMethod.md`
- `<v8std mirror>/docs/std/783.md`

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

## Закрыто Track 2

**Phase D §2 audit (2026-05):** out-of-scope для Track 2. Master plan §2
ограничен `BeginTransactionBeforeTryCatch` + `MissingCodeTryCatchEx`;
правила про `ОтменитьТранзакцию` положение и парность — отдельный
будущий трек по transaction-shape анализу.

## Закрыто Track 3

**Phase C C2 (commit `COMMIT_SHA`, 2026-05-10):** добавлены fixtures
`test_first_rollback_without_local_transaction_snapshot` и
`test_nested_try_body_rollback_snapshot` для пробела "не связывает
RollbackTransaction с конкретной парой BeginTransaction/CommitTransaction
и не анализирует вложенные транзакционные сценарии глубоко". Первый
snapshot фиксирует позиционный scope диагностики; второй закрепляет
текущее поведение на вложенном `Попытка`.
