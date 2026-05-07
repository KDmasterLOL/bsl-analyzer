# ForbiddenMetadataName

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Metadata objects не должны называться зарезервированными словами языка
запросов вроде `Документ`, `Справочник`, `РегистрСведений`. Основание -
`#std474`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/forbidden_metadata_name.rs`
- `crates/ide-diagnostics/docs/ru/ForbiddenMetadataName.md`
- `docs/legal/diagnostics/ForbiddenMetadataName.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/474.md`

## Как реализовано

`from_metadata()` проверяет `common_module`, `mdo` (имя + реквизиты + табличные
части и их реквизиты), `register` (имя + измерения + реквизиты + ресурсы)
против захардкоженного `FORBIDDEN_NAMES` для русских и английских имен.
Отдельный `check_session_module()` проходит по всей конфигурации и проверяет
mdo без своих модулей (Constant, ExchangePlan и т.п.). Diagnostic ставится на
`MODULE_RANGE` (нулевой диапазон в начале модуля).

## Что покрыто

Тесты покрывают разные типы metadata, регистры, common modules, case-insensitive
совпадения и допустимые имена.

## Пробелы и ограничения

- Range не указывает на конкретное имя metadata object в XML.
- Список reserved names зашит вручную и может рассинхронизироваться с
  платформой.
- Не проверяются все metadata kinds из `MdoType` одинаково подробно.
- Нет quick-fix rename.

## Может ли инфраструктура улучшить качество

Нужен metadata property range/index и общий naming vocabulary source для
reserved platform/query names.

## Возможное объединение

Близко к `CommonModuleNameWords`, `MetadataObjectNameLength`,
`LatinAndCyrillicSymbolInWord`. Внутренне стоит иметь общий metadata naming
engine, внешне коды оставить разными.

## Вывод

Идея точная, но UX ограничен отсутствием range на metadata name и data-driven
словаря.

