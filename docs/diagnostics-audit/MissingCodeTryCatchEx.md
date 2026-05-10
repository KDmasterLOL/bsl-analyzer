# MissingCodeTryCatchEx

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит `Попытка...Исключение...КонецПопытки`, где блок `Исключение` молча
подавляет ошибку: пустой блок, блок без `ВызватьИсключение`/журналирующего
вызова, либо блок, единственным эффектом которого является `ОтменитьТранзакцию`
без последующего сообщения об ошибке.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_code_try_catch_ex.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `<v8std mirror>/docs/diagnostics/bslls/MissingCodeTryCatchEx.md`
- `<v8std mirror>/docs/std/499.md`

## Как реализовано

HIR-обход по всем телам методов и коду модуля. Для каждого `Stmt::Try` тело
`Исключение` классифицируется через `hir::catch_class::classify_catch_body`
(Track 2 Phase D §2.1, `crates/hir-def/src/catch_class.rs`) в один из шести
классов: `Empty`, `RaisesOnly`, `LogsOnly`, `Mixed`, `RollbackOnly`, `Silent`.
Распознавание журналирующих вызовов идёт через `Category::Logging` единого
security-реестра (Track 2 Phase A §1.1, `crates/bsl-platform/src/security/`),
без локальных whitelists. **Track 2 §2.2 (2026-05-10):** handler dispatches
по классу — `Empty`/`Silent`/`RollbackOnly` эмитят (с разными сообщениями;
`RollbackOnly` рекомендует добавить логирование/`ВызватьИсключение`),
`RaisesOnly`/`LogsOnly`/`Mixed` пропускаются как корректные пути восстановления.
Настройка `commentAsCode=true` оставлена и применяется только к ветке `Empty`
через AST fallback — HIR не хранит trivia.

## Что покрыто

Покрыты вложенные try/catch, module-level code, настройка `commentAsCode`,
все шесть классов catch-body (raises/logs/mixed/rollback-only/silent/empty),
и cross-language логи (`Сообщить`/`Message`/`ЗаписьЖурналаРегистрации`/
`WriteLogEvent` через registry).

## Пробелы и ограничения

`Silent` намеренно консервативен: неизвестный пользовательский вызов в
`Исключение` нельзя без inter-procedural анализа доказать как
re-raise/log, поэтому он считается silent swallow. Для случаев, когда
проект имеет собственные обёртки журналирования, нужна возможность
расширить registry (Track 6 — extension hook). `RollbackOnly` сообщение
не предлагает quick-fix — его реализация требует выбора между «добавить
логирование» и «добавить ВызватьИсключение», что зависит от контекста
вызывающей процедуры.

## Может ли инфраструктура улучшить качество

Track 2 §2.2 закрыл основной запрос — классификатор catch-body actions
с распознаванием логирования/raise/rollback. Дальнейшее улучшение —
inter-procedural детектор «пользовательский метод гарантированно
raise/log» (Track 6) и возможность кастомизировать Logging-registry
(Track 6).

## Возможное объединение

Близко к `TryNumber`, `UsageWriteLogEvent`, `BeginTransactionBeforeTryCatch`, `CommitTransactionOutsideTryCatch`, `WrongUseOfRollbackTransactionMethod`. Можно строить общий exception handling analyzer.

## Вывод

Диагностика хорошо ловит самый грубый случай, но качество обработки исключений шире, чем “пустой/не пустой блок”.
