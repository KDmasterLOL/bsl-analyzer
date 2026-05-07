# TryNumber

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Запрещает использовать `Попытка` как способ приведения к числу через `Число()` / `Number()`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/try_number.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/TryNumber.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/499.md`

## Как реализовано

HIR находит вызовы `Число`/`Number` внутри try-body; handler создает diagnostic на вызов.

## Что покрыто

Покрыты русское/английское имя, case-insensitive форма и вложенные try. Вызов в `Исключение` и вне try не срабатывает.

## Пробелы и ограничения

Не предлагается альтернатива (`СтрНайти`, `Число` с предварительной проверкой, helper parser). Нет анализа того, действительно ли исключение перехватывает только conversion.

## Может ли инфраструктура улучшить качество

Да. Нужен exception intent analyzer и code action на безопасную проверку ввода.

## Возможное объединение

Близко к `MissingCodeTryCatchEx`, `UsageWriteLogEvent`, transaction/exception diagnostics.

## Вывод

Точное правило для конкретного анти-паттерна, но улучшения лежат в рекомендациях по замене.
