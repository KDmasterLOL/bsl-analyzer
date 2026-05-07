# ReservedParameterNames

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Запрещает параметры метода, имена которых входят в конфигурируемый список зарезервированных имен.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/reserved_parameter_names.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/ReservedParameterNames.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/454.md`

## Как реализовано

Через `item_tree` обходятся параметры функций/процедур. Конфиг `reservedWords` читается как массив строк, сравнение case-insensitive по полному имени.

## Что покрыто

Покрыты несколько слов, функции/процедуры, case-insensitive совпадение и отсутствие partial match.

## Пробелы и ограничения

По умолчанию список пустой, поэтому правило ничего не делает без настройки. Нет rename fix.

## Может ли инфраструктура улучшить качество

Да. Нужен дефолтный список из стандарта/платформы и rename refactoring для параметров.

## Возможное объединение

Близко к `ReservedWordAsMethodName`, `BadWords`, `CommonModuleNameWords`. Общий naming policy layer был бы полезен.

## Вывод

Механизм есть, но без дефолтного набора reserved words покрытие зависит от конфигурации пользователя.
