# SetPermissionsForNewObjects

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что флаг “Устанавливать права для новых объектов” включен только у разрешенных ролей, по умолчанию `FullAccess, ПолныеПрава`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/set_permissions_for_new_objects.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/SetPermissionsForNewObjects.md`

## Как реализовано

Запускается только из `ManagedApplicationModule`, загружает конфигурацию, читает роли и сверяет `set_for_new_objects()` с конфигом `namesFullAccessRole`.

## Что покрыто

Покрыты роли с включенным флагом, allow-list ролей, отключение без metadata и проверка только managed application module.

## Пробелы и ограничения

Диапазон синтетический в начале модуля, хотя нарушение в role metadata. Сравнение allow-list без нормализации регистра.

## Может ли инфраструктура улучшить качество

Да. Нужны точные XML ranges ролей и project-level запуск metadata diagnostics.

## Возможное объединение

Близко к `ProtectedModule`, `OrdinaryAppSupport`, `SameMetadataObjectAndChildNames`: metadata project diagnostics. Security-wise рядом с `PrivilegedModuleMethodCall`.

## Вывод

Правило закрывает важный security-риск, но UX зависит от развития metadata ranges.
