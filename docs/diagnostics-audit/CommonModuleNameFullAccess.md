# CommonModuleNameFullAccess

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Привилегированный общий модуль должен иметь `ПолныеПрава` или `FullAccess` в
имени. Основание - `#std469`; по смыслу это security hotspot.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_name_full_access.rs`
  (через макрос `define_common_module_name_check!`)
- `crates/ide-diagnostics/src/common_module_helpers.rs`
- `crates/ide-diagnostics/docs/ru/CommonModuleNameFullAccess.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleNameFullAccess.md`
- `docs/legal/diagnostics/CommonModuleNameFullAccess.md`
- `<v8std mirror>/docs/std/469.md`,
  `<v8std mirror>/docs/diagnostics/bslls/CommonModuleNameFullAccess.md`

## Как реализовано

Predicate проверяет `m.is_privileged()`, затем helper ищет keyword через
`contains`.

## Что покрыто

Есть тесты на privileged без keyword, с English keyword и Russian keyword.

## Пробелы и ограничения

- Проверяется substring, а не postfix.
- Нет тестов на сочетание с другими postfix policies: `Глобальный`,
  `ПовтИсп`, `КлиентСервер`.
- Нет связи с actual security diagnostics, которые ловят privileged/safe mode
  behavior в коде.
- Сообщение не объясняет, что именно включен flag `privileged`.

## Инфраструктурные улучшения

Общий postfix parser может валидировать порядок уточняющих постфиксов и
показывать ожидаемое canonical имя.

## Возможное объединение

Внутренне объединяется с `CommonModuleName*`; с security diagnostics сливать
не надо, но можно добавить общий tag/сводку по privileged-risk.

## Вывод

Правило полезное, но сейчас проверяет только наличие слова. Для security
hotspot желательно более точное сообщение и строгая проверка имени.

