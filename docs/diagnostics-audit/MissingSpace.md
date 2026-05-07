# MissingSpace

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет пробелы вокруг операторов, разделителей и ключевых слов.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_space.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/index.md`

## Как реализовано

Token-based правило с конфигами `listForCheckLeft`, `listForCheckRight`, `listForCheckLeftAndRight`, `checkSpaceToRightOfUnary`, `allowMultipleCommas`. Для ключевых слов есть отдельные политики: `AND/OR/IN/TO` с двух сторон, `EXPORT/THEN/DO` слева, `IF/ELSIF/WHILE/FOR/NOT/EACH` справа. Есть fixes на вставку пробелов.

## Что покрыто

Покрыты операторы, запятые/точки с запятой, ключевые слова и различение unary `+/-` от бинарных операторов.

## Пробелы и ограничения

Token-based подход может конфликтовать с полноценным форматтером и плохо объясняет спорные многострочные случаи. Настройки через строки списков требуют осторожности с многосимвольными операторами.

## Может ли инфраструктура улучшить качество

Да. Оптимальный путь - formatter-aware diagnostics: правило должно либо использовать тот же layout engine, либо стать thin wrapper над форматтером.

## Возможное объединение

Близко к `IncorrectLineBreak`, `LineLength`, `OneStatementPerLine`, `ConsecutiveEmptyLines`. Стоит объединить форматирующую инфраструктуру и fixes, но оставить отдельные коды для управления стилем.

## Вывод

Это одна из немногих style diagnostics с fix. Главный риск - расхождение с будущим/внешним форматтером.
