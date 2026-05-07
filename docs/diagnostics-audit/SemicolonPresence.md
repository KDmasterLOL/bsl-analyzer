# SemicolonPresence

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет наличие точки с запятой в конце выражений.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/semicolon_presence.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/SemicolonPresence.md`

## Как реализовано

HIR lowering передает range statement без `;`. Handler создает diagnostic и fix, который вставляет `;` в `range.end()`.

## Что покрыто

Покрыты обычные expressions, `Возврат` перед `КонецЕсли`, отсутствие срабатывания на метки и suppress при parse-error случаях.

## Пробелы и ограничения

Fix зависит от корректного `range.end()`. Нет объединения с форматтером, поэтому массовое исправление отдельно от style formatting.

## Может ли инфраструктура улучшить качество

Да. Formatter/code action layer может применять fix пачкой и не конфликтовать с переносами строк.

## Возможное объединение

Близко к `OneStatementPerLine`, `MissingSpace`, `IncorrectLineBreak`. Это часть formatter diagnostics.

## Вывод

Хорошая auto-fixable style diagnostic; лучшее развитие - интеграция с общим форматтером.
