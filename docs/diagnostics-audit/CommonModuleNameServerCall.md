# CommonModuleNameServerCall

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Общий модуль для вызова с клиента должен иметь postfix
`ВызовСервера`/`ServerCall`. Основание - `#std469`, раздел 2.2.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_name_server_call.rs`
  (через макрос `define_common_module_name_check!`)
- `crates/ide-diagnostics/src/common_module_helpers.rs`
- `crates/ide-diagnostics/docs/ru/CommonModuleNameServerCall.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleNameServerCall.md`
- `docs/legal/diagnostics/CommonModuleNameServerCall.md`
- `<v8std mirror>/docs/std/469.md`,
  `<v8std mirror>/docs/diagnostics/bslls/CommonModuleNameServerCall.md`

## Как реализовано

Predicate использует `is_server_call`: `server_call && server &&
!external_connection && !client_*`. Имя ищется через `contains`.

## Что покрыто

Тесты есть для server-call без keyword, с English keyword и с Russian keyword.

## Пробелы и ограничения

- Нет строгой проверки окончания имени.
- Нет тестов на конфликтные флаги, например `server_call = true` вместе с
  `external_connection = true`.
- Нет проверки экспортных методов server-call API и запрета мутабельных типов,
  хотя `#std469` упоминает это рядом.

## Инфраструктурные улучшения

Общий `CommonModuleKind` может служить входом не только для naming, но и для
future server-call API checks.

## Возможное объединение

Внутренне объединить с остальными name policies. С отдельными API/parameter
проверками лучше не смешивать, потому что это другой behavioral surface.

## Вывод

Реализация покрывает основной metadata-сигнал, но пока не использует более
богатый контекст server-call модулей.

