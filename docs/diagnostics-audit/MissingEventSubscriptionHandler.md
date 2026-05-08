# MissingEventSubscriptionHandler

Статус: `done`, `needs-code-work`
Track 1 closure: D `637a6279`, M `691a751c` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Проверяет обработчики подписок на события из метаданных: обработчик должен быть корректно указан, находиться в существующем серверном общем модуле и быть экспортным методом.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_event_subscription_handler.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/MissingEventSubscriptionHandler.md`

## Как реализовано

Диагностика запускается только для `Configuration/Ext/SessionModule.bsl`. Она загружает конфигурацию, обходит `event_subscriptions()`, парсит handler string, проверяет наличие common module, флаг server, наличие метода и `Экспорт`.

## Что покрыто

Покрыты пустой обработчик, неверный формат, отсутствующий модуль, модуль не server, отсутствующий метод и неэкспортный метод.

## Пробелы и ограничения

Diagnostic range синтетический, потому что нарушение лежит в metadata XML, а запуск привязан к session module. Если source file общего модуля не найден, проверка метода пропускается. Нет проверки сигнатуры обработчика события.

## Может ли инфраструктура улучшить качество

Да. Нужны source ranges в metadata XML, project-level diagnostics без привязки к session module и signature model для event handlers.

## Возможное объединение

Близко к `ScheduledJobHandler`, `WrongHttpServiceHandler`, `WrongWebServiceHandler`, `CommandModuleExportMethods`. Можно сделать общий metadata handler validation engine.

## Вывод

Правило закрывает важную проектную ошибку, но сейчас ограничено местом запуска и отсутствием точных metadata ranges.
