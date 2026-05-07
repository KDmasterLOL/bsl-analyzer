# DenyIncompleteValues

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Измерения регистров должны иметь флаг `Запрет незаполненных значений`, чтобы
не накапливать некорректные записи. Это metadata-level diagnostic.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deny_incomplete_values.rs`
- `crates/ide-diagnostics/docs/ru/DenyIncompleteValues.md`
- `docs/legal/diagnostics/DenyIncompleteValues.md`

## Как реализовано

`from_metadata()` берет `metadata.register`, проходит dimensions и диагностирует
каждое измерение с `deny_incomplete_values = false`. Диапазон ставится в начало
файла `[0, min(9, file_len))`.

## Что покрыто

Тесты покрывают одно измерение без флага, с флагом, несколько измерений,
non-register metadata, disabled diagnostic и форматирование имени регистра.

## Пробелы и ограничения

- Diagnostic range не указывает на конкретное измерение в metadata XML/BSL,
  только на начало файла.
- Не учитываются исключения, когда пустые значения допустимы по модели данных.
- `activated_by_default = false`, но это нужно сверить с документацией и
  expectations пользователя.
- Нет quick-fix для изменения metadata property.

## Инфраструктурные улучшения

Нужны ranges для metadata properties и project-level metadata diagnostics,
которые могут открывать конкретный XML/property вместо module source.

## Возможное объединение

Близко к metadata-quality diagnostics, но внешне объединять не нужно. Внутренне
можно использовать общий metadata property checker.

## Вывод

Семантика простая, но UX слабый из-за отсутствия точного range и metadata fix.

