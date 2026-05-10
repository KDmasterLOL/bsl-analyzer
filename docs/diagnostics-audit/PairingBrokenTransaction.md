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

## Закрыто Track 2

**Phase D §2 audit (2026-05):** out-of-scope для Track 2. Существующий
CFG-based анализ парности транзакций сохранён без изменений; общий
transaction-shape analyzer — отдельный будущий трек.

## Закрыто Track 3

**Phase C C2 (commit `COMMIT_SHA`, 2026-05-10):** добавлен fixture
`test_interprocedural_begin_commit_are_not_paired_snapshot` для пробела
"Анализ ограничен телом метода; межпроцедурные begin/commit не
моделируются". Snapshot фиксирует текущее method-local поведение: begin
и commit в разных процедурах считаются двумя локальными нарушениями.
Настоящие interprocedural transaction effects оставлены для Track 6.
