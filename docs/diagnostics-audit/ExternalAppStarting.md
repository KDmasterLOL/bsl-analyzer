# ExternalAppStarting

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Запуск внешних приложений и системных команд требует security review.
Основание - `#std774` и связанный `#std669`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/external_app_starting.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/hir-def/src/body/lower/diagnostics.rs`
- `crates/ide-diagnostics/docs/ru/ExternalAppStarting.md`
- `docs/legal/diagnostics/ExternalAppStarting.md`
- `<v8std mirror>/docs/std/774.md`

## Как реализовано

HIR lowering проверяет global и qualified method calls по спискам внешнего
запуска: `КомандаСистемы`, `ЗапуститьПриложение`, async variants,
`ФайловаяСистемаКлиент.ЗапуститьПрограмму`, `ОткрытьФайл` и др. Handler
ставит security hotspot diagnostic.

## Что покрыто

Тесты покрывают глобальные методы, run program через файловую систему,
открытие проводника/файла, async launch, `ЗапуститьСистему`, object calls,
similar-name negative и English variants.

## Пробелы и ограничения

- Список API зашит в HIR diagnostics, отдельно от `FileSystemAccess` и
  `ExecuteExternalCode`.
- Qualified calls проверяются в основном по последнему method name; тип receiver
  не всегда доказывается.
- Нет анализа аргументов: literal `calc.exe`, user input, allowlist paths.
- Нет quick-fix, потому что это security review, а не механическая замена.

## Может ли инфраструктура улучшить качество

Security API registry с категориями `process_start`, `filesystem`,
`external_code`, аргументами и optional allowlist.

## Возможное объединение

Внутренне объединить с `FileSystemAccess`, `ExecuteExternalCode`,
`InternetAccess` через registry. Внешние коды полезно оставить раздельными по
категориям риска.

## Вывод

Список покрывает много реальных API, но без type/arg analysis это скорее
security hotspot, чем доказанная уязвимость.


## Закрыто Track 2

**Phase A §1.6 Group A (commit `4a9a9290`, 2026-05):** локальный whitelist
external-app API заменён на `bsl_platform::security::registry` lookup
(`Category::ExternalApp`). Argument/taint-analysis — Track 6 (registry
уже несёт `Role::Cmd`/`Role::Path` для будущей taint-pipeline).
