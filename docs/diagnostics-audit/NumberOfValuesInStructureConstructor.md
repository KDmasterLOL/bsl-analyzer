# NumberOfValuesInStructureConstructor

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Ограничивает количество значений, переданных в конструктор `Структура` / `ФиксированнаяСтруктура`. Дефолт `maxValuesCount` равен 3.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/number_of_values_in_structure_constructor.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/NumberOfValuesInStructureConstructor.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/693.md`

## Как реализовано

HIR-обход по `Expr::New`; для `Structure`/`FixedStructure` считается `args.len() - 1`, где первый аргумент считается строкой ключей.

## Что покрыто

Покрыты русские/английские конструкторы, пустые структуры, структуры только с ключами, настройка лимита.

## Пробелы и ограничения

Не проверяется соответствие количества ключей и значений. Нет fix для переписывания на последовательные `Вставить`.

## Может ли инфраструктура улучшить качество

Да. Нужен parser ключевой строки структуры и code action, который развернет конструктор в несколько инструкций.

## Возможное объединение

Близко к `NestedConstructorsInStructureDeclaration` и `NumberOfParams`: общий analyzer перегруженных inline-конструкторов.

## Вывод

Правило ловит читаемостный smell, но качество можно сильно поднять проверкой ключей и автоматическим rewrite.
