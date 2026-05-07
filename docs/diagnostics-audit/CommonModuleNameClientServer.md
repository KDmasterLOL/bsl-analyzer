# CommonModuleNameClientServer

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Клиент-серверный общий модуль должен иметь `КлиентСервер` или `ClientServer`
в имени. Основание - `#std469`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_name_client_server.rs`
  (через макрос `define_common_module_name_check!`)
- `crates/ide-diagnostics/src/common_module_helpers.rs`
- `crates/ide-diagnostics/docs/ru/CommonModuleNameClientServer.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleNameClientServer.md`
- `docs/legal/diagnostics/CommonModuleNameClientServer.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/469.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/CommonModuleNameClientServer.md`

## Как реализовано

Использует predicate `is_client_server`: `server`, `external_connection`,
client flags и без `server_call`. Имя ищется через `contains`.

## Что покрыто

Есть два metadata-теста: без keyword и с keyword.

## Пробелы и ограничения

- Нет проверки постфикса, только substring.
- Нет тестов на граничные комбинации флагов: server call включен,
  external connection выключен, ordinary app support выключен.
- Не проверяется конфликт с другими postfix policies, например
  `ClientServerCached`.
- Diagnostic не объясняет найденный тип модуля и expected flags.

## Инфраструктурные улучшения

Использовать общий `CommonModuleKind::ClientServer` и общий matcher цепочки
постфиксов.

## Возможное объединение

Один из самых очевидных кандидатов на data-driven объединение с
`CommonModuleNameClient`, `ServerCall`, `Global`, `Cached`.

## Вывод

Базовое правило есть, но без строгого postfix matcher оно не полностью
соответствует формулировке стандарта.

