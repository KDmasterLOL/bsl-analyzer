# CommonModuleNameClient

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Неглобальный клиентский общий модуль должен иметь в имени `Клиент` или
`Client`. Основание - `#std469`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_name_client.rs`
  (через макрос `define_common_module_name_check!`)
- `crates/ide-diagnostics/src/common_module_helpers.rs`
- `crates/ide-diagnostics/docs/ru/CommonModuleNameClient.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleNameClient.md`
- `docs/legal/diagnostics/CommonModuleNameClient.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/469.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/CommonModuleNameClient.md`

## Как реализовано

Predicate: `!global && is_client(module, ordinary_app_support)`. Имя
проверяется через `contains("клиент" | "client")`.

## Что покрыто

Есть тесты на клиентский модуль без keyword, с keyword, global exception и
non-common module.

## Пробелы и ограничения

- `contains` не проверяет именно постфикс.
- `is_client` требует `client_managed_application = true`; случаи "только
  обычное приложение", которые документация допускает как исключения, не
  попадают в правило.
- Нет тестов на `ordinary_app_support = false`.
- Глобальный клиентский модуль полностью передается в
  `CommonModuleNameGlobalClient`, но порядок/совместное срабатывание не
  закреплены интеграционным тестом.

## Инфраструктурные улучшения

Нужен единый classifier типа common module с режимом strict/exception-aware,
иначе name diagnostics и invalid-type diagnostics будут по-разному трактовать
одни и те же flags.

## Возможное объединение

Кандидат на внутреннее объединение с остальными `CommonModuleName*` через
таблицу postfix policies. Внешне код лучше оставить отдельным.

## Вывод

Правило покрывает основной сценарий, но плохо моделирует допустимые исключения
и принимает слишком широкие совпадения имени.

