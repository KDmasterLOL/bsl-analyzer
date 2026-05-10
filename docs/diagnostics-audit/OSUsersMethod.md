# OSUsersMethod

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит вызовы глобального метода `ПользователиОС()` / `OSUsers()` как потенциально опасный доступ к пользователям ОС.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/os_users_method.rs`
- `<v8std mirror>/docs/diagnostics/bslls/OSUsersMethod.md`

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

## Закрыто Track 2

**Phase A §1.6 Group A (commit `4a9a9290`, 2026-05):** hardcoded имя
заменено на `bsl_platform::security::registry` lookup
(`Category::OsUsers`). Контекстный анализ использования (передача в
ACL, сравнения с константами и т.п.) — Track 6.
