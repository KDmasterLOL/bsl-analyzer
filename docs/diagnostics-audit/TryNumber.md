# TryNumber

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Запрещает использовать `Попытка` как способ приведения к числу через `Число()` / `Number()`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/try_number.rs`
- `<v8std mirror>/docs/diagnostics/bslls/TryNumber.md`
- `<v8std mirror>/docs/std/499.md`

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

## Закрыто Track 2

**Phase D §2 audit (2026-05):** out-of-scope для Track 2. Правило
ограничивает количество try-блоков в методе и не пересекается с
catch-body classifier (`MissingCodeTryCatchEx`) или Begin-before-Try
паттерном.

## Закрыто Track 3

**Phase C C2 (commit `COMMIT_SHA`, 2026-05-10):** добавлены fixtures
`test_try_with_mixed_body_still_flags_number_snapshot` и
`test_number_inside_if_in_try_snapshot` для пробела "Нет анализа того,
действительно ли исключение перехватывает только conversion". Snapshot
фиксирует текущую эвристику: любой global `Число`/`Number` в try-body
эмитит diagnostic, включая вложенный `Если` и смешанный try-body.
