# MismatchedArgCount

Статус: `done`, `needs-code-work`
Track 1 closure: G1 `27fb95ec`, G2 `1e5230fd` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Сообщает о вызовах методов с неверным количеством аргументов.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/mismatched_arg_count.rs`
- `crates/ide-diagnostics/src/hir_inference_dispatch.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/index.md`

## Как реализовано

Диагностика приходит из inference layer как `InferenceDiagnostic::MismatchedArgCount`. Обработчик строит сообщение: точное число аргументов или диапазон с учетом optional parameters.

## Что покрыто

Покрыты платформенные методы, resolved qualified calls и методы с optional parameters. Если число аргументов попадает в допустимый диапазон, diagnostic не создается.

## Пробелы и ограничения

Правило зависит от разрешения callee. Неразрешенные локальные/общемодульные вызовы уйдут в другие diagnostics или не получат проверки количества. Нет fix для удаления/добавления аргументов и нет подсказки по именам параметров.

## Может ли инфраструктура улучшить качество

Да. Улучшение symbol resolution и signature database напрямую повысит покрытие. Для UX полезны signature help и quick actions “remove extra args” / “insert placeholders”.

## Возможное объединение

Близко к `MissedRequiredParameter`, `NumberOfParams`, `NumberOfOptionalParams`, `OrderOfParams`, `ExtraCommas`. Можно сделать общий call/signature validation engine, но публичные коды различают разные ошибки пользователя.

## Вывод

Качество правила определяется качеством inference и базы сигнатур. Сам handler простой и достаточный.
