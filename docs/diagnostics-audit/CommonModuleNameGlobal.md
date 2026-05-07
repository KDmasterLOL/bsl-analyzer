# CommonModuleNameGlobal

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Глобальный общий модуль должен иметь `Глобальный` или `Global` в имени.
Основание - `#std469`, раздел 3.2.1.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_name_global.rs`
  (через макрос `define_common_module_name_check!`)
- `crates/ide-diagnostics/src/common_module_helpers.rs`
- `crates/ide-diagnostics/docs/ru/CommonModuleNameGlobal.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleNameGlobal.md`
- `docs/legal/diagnostics/CommonModuleNameGlobal.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/469.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/CommonModuleNameGlobal.md`

## Как реализовано

Predicate проверяет `m.is_global()`. Имя ищется через `contains`.

## Что покрыто

Тесты покрывают global без keyword, Russian keyword, English keyword и
non-global.

## Пробелы и ограничения

- Нет строгой проверки постфикса.
- Не закреплена совместная работа с `CommonModuleNameGlobalClient`: global
  client module может получить два diagnostics или один в зависимости от имени.
- Нет подсказки про то, что `Клиент` для глобального клиентского модуля
  избыточен.

## Инфраструктурные улучшения

Общий common-module-name engine должен уметь выдавать несколько связанных
нарушений по одному имени, но группировать их в понятный результат.

## Возможное объединение

Сильный кандидат на внутреннее объединение с `GlobalClient`; фактически это
две стороны одного global naming policy.

## Вывод

Базовые кейсы покрыты. Улучшение качества зависит от общего matcher'а и
координации с `CommonModuleNameGlobalClient`.

