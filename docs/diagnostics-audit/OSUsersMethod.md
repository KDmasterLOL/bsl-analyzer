# OSUsersMethod

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит вызовы глобального метода `ПользователиОС()` / `OSUsers()` как потенциально опасный доступ к пользователям ОС.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/os_users_method.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/OSUsersMethod.md`

## Как реализовано

Срабатывание приходит из HIR lowering; handler только создает simple diagnostic по диапазону вызова.

## Что покрыто

Покрыты русское и английское имя, case-insensitive вызовы. Не срабатывает на ссылку без вызова и квалифицированный вызов `МойМодуль.ПользователиОС()`.

## Пробелы и ограничения

Сообщение на английском и довольно общее. Нет анализа контекста: сервер/клиент, права, аудит, легитимные admin tools.

## Может ли инфраструктура улучшить качество

Да. Нужна классификация security context и более точное сообщение с рекомендацией альтернативы.

## Возможное объединение

Близко к `PrivilegedModuleMethodCall`, `ExecuteExternalCode`, `ExternalAppStarting`, `FileSystemAccess`, `InternetAccess`. Общий security-hotspot framework был бы полезен.

## Вывод

Правило технически простое и точное по вызову, но требует лучшего сообщения и контекстной оценки риска.
