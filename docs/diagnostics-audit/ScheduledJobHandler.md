# ScheduledJobHandler

Статус: `done`, `needs-code-work`
Track 1 closure: D `637a6279`, M `691a751c` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Проверяет обработчики регламентных заданий из метаданных.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/scheduled_job_handler.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/ScheduledJobHandler.md`

## Как реализовано

Запускается из `SessionModule`, загружает конфигурацию, обходит scheduled jobs. Проверяет пустой handler, формат, наличие common module, server flag, наличие/экспортность метода, пустое тело метода, наличие параметров у предопределённого задания и дублирующее использование обработчиков.

## Что покрыто

Покрыты основные metadata mistakes (8 проверок: EmptyHandler/MissingModule/NonServerModule/MissingMethod/NonExportMethod/MethodWithParameters для предопределённых/EmptyMethod/DuplicateHandler). Method lookup идет через symbol tree common module, тело — через `module_bodies_for(module_id)`.

## Пробелы и ограничения

Диапазон synthetic/session-module; нет точных XML ranges. Если source common module не найден, проверка метода пропускается. Signature handler проверяется ограниченно.

## Может ли инфраструктура улучшить качество

Да. Нужны metadata ranges, project diagnostics и общий handler-signature validator.

## Возможное объединение

Близко к `MissingEventSubscriptionHandler`, `WrongHttpServiceHandler`, `WrongWebServiceHandler`. Общий metadata handler validation engine очевиден.

## Вывод

Проверка полезная и достаточно широкая, но UX упирается в metadata infrastructure.
