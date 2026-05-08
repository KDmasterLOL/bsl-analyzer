# PairingBrokenTransaction

Статус: `done`, `needs-code-work`
Track 1 closure: foundation `819945b7`, P `5b656687` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Проверяет парность `НачатьТранзакцию` с `ЗафиксироватьТранзакцию` или `ОтменитьТранзакцию` на путях выполнения.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/pairing_broken_transaction.rs`
- `<v8std mirror>/docs/diagnostics/bslls/PairingBrokenTransaction.md`
- `<v8std mirror>/docs/std/783.md`

## Как реализовано

CFG-based DFS по телам методов. Состояние хранит уровень транзакции и стек begin-calls. Commit/Rollback уменьшают уровень. Конфиг `maxTransactionLevel` ограничивает глубину обхода.

## Что покрыто

Покрыты непарные begin, orphan commit/rollback, ветвления и циклы с учетом CFG-state deduplication.

## Пробелы и ограничения

Анализ ограничен телом метода; межпроцедурные begin/commit не моделируются. Ограничение `maxTransactionLevel` может скрыть патологические случаи.

## Может ли инфраструктура улучшить качество

Да. Нужны interprocedural effects для транзакций и связь с exception-handling diagnostics.

## Возможное объединение

Близко к `BeginTransactionBeforeTryCatch`, `CommitTransactionOutsideTryCatch`, `WrongUseOfRollbackTransactionMethod`, `MissingCodeTryCatchEx`. Нужен общий transaction/exception analyzer.

## Вывод

Это одна из более сильных диагностик: уже использует CFG. Следующий шаг - межпроцедурные эффекты и интеграция с try/catch.
