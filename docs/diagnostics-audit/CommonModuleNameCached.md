# CommonModuleNameCached

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Общий модуль с повторным использованием возвращаемых значений должен содержать
в имени `ПовтИсп` или `Cached`. Основание - `#std469`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_name_cached.rs`
  (через макрос `define_common_module_name_check!` из
  `crates/ide-diagnostics/src/common_module_helpers.rs`)
- `crates/ide-diagnostics/docs/ru/CommonModuleNameCached.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleNameCached.md`
- `docs/legal/diagnostics/CommonModuleNameCached.md`
- `<v8std mirror>/docs/std/469.md`,
  `<v8std mirror>/docs/diagnostics/bslls/CommonModuleNameCached.md`

## Как реализовано

Через общий helper `check_common_module_name()`: predicate проверяет
`return_values_reuse != DontUse`, затем имя ищется по `contains` среди
`повторноеиспользование`, `повтисп`, `cached`.

## Что покрыто

Есть metadata-тесты для cached без keyword, cached с keyword и not cached.

## Пробелы и ограничения

- Используется `contains`, хотя документация говорит про постфикс. Имя с
  keyword в середине будет принято.
- Нет проверки комбинированных постфиксов вроде `КлиентПовтИсп` как отдельной
  структуры имени.
- Нет тестов на русскую длинную форму `ПовторноеИспользование`.
- Нет unified explanation: какой flag включен и какой postfix ожидается.

## Инфраструктурные улучшения

Сделать `CommonModuleNamePolicy`: token/postfix matcher, language variants,
композиция постфиксов и генерация тестов.

## Возможное объединение

Внешний код можно оставить отдельным для совместимости, но все
`CommonModuleName*` явно должны стать data-driven правилами одной таблицы.

## Вывод

Покрытие базовое, но качество имени проверяется слишком мягко. Главная правка -
заменить `contains` на нормальный postfix/token matcher.

