# OrdinaryAppSupport

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет настройки поддержки обычного приложения в метаданных конфигурации.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/ordinary_app_support.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/OrdinaryAppSupport.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/467.md`

## Как реализовано

Запускается только при `config.ordinary_app_support=true` и только в `SessionModule`. Загружает `Configuration` и проверяет `use_managed_form_in_ordinary_application` и `use_ordinary_form_in_managed_application`.

## Что покрыто

Покрыты неверные значения двух флагов, отключение через config и отсутствие срабатывания не в session module.

## Пробелы и ограничения

Диапазон синтетический в session module, хотя нарушение находится в metadata XML. Диагностика полностью отключается глобальным флагом `ordinary_app_support`.

## Может ли инфраструктура улучшить качество

Да. Нужны project-level diagnostics с точными ranges на XML-свойствах конфигурации.

## Возможное объединение

Близко к `ProtectedModule`, `ScheduledJobHandler`, `MissingEventSubscriptionHandler`: все это metadata-backed project diagnostics, сейчас привязанные к session module.

## Вывод

Правило полезно как проектная проверка, но UX ограничен отсутствием точных metadata ranges.
