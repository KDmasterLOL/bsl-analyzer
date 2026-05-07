# CommonModuleNameGlobalClient

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Глобальный клиентский общий модуль не должен дополнительно содержать
`Клиент`/`Client`; достаточно `Глобальный`/`Global`. Основание - `#std469`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_name_global_client.rs`
  (через макрос `define_common_module_name_check!`,
  `name_should_contain: false` — инвертированная логика)
- `crates/ide-diagnostics/src/common_module_helpers.rs`
- `crates/ide-diagnostics/docs/ru/CommonModuleNameGlobalClient.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleNameGlobalClient.md`
- `docs/legal/diagnostics/CommonModuleNameGlobalClient.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/469.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/CommonModuleNameGlobalClient.md`

## Как реализовано

Predicate: `global && is_client(...)`. Helper настроен инверсно:
`name_should_contain = false`, поэтому diagnostic появляется, если имя
содержит `клиент` или `client`.

## Что покрыто

Есть тесты на global client с English keyword, без keyword и с Russian keyword.

## Пробелы и ограничения

- `contains` может дать false positive, если `client` входит не как postfix, а
  как часть другого слова.
- Нет проверки, что global postfix при этом присутствует; это оставлено
  `CommonModuleNameGlobal`, но совместный UX не описан.
- Нет тестов на non-global client module с `Client` - правило должно молчать.

## Инфраструктурные улучшения

Нужен tokenizer/postfix parser для имени общего модуля, чтобы различать
реальный postfix и подстроку внутри доменного имени.

## Возможное объединение

Логически объединяется с `CommonModuleNameGlobal` в один global naming policy,
но отдельный diagnostic code полезен для точного сообщения.

## Вывод

Правило ловит типовой лишний postfix, но substring-проверка делает его более
шумным, чем нужно.

