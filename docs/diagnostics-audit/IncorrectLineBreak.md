# IncorrectLineBreak

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Некорректные переносы выражений ухудшают читаемость. Основание - `#std444`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/incorrect_line_break.rs`
- `crates/ide-diagnostics/docs/ru/IncorrectLineBreak.md`
- `docs/legal/diagnostics/IncorrectLineBreak.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/444.md`

## Как реализовано

AST token pass группирует tokens по строкам. На конце строки запрещены
арифметические операторы и `И`/`ИЛИ`; в начале строки запрещены `)`, `;`, а
`,` с содержимым после нее диагностируется отдельно. Многострочные строки
после operator line пропускаются.

## Что покрыто

Тесты проверяют operators at line end, line-start symbols, comma-at-start,
multi-line strings и negative cases.

## Пробелы и ограничения

- Часть сообщений на английском.
- Нет quick-fix переноса operator/comma.
- Token policy зашита в коде и отделена от formatter.
- Не все правила `#std444` моделируются, например сложные вызовы/параметры.

## Может ли инфраструктура улучшить качество

Подключить formatter policy table и text-edit builder для автоматического
переноса.

## Возможное объединение

Внутренне с formatting diagnostics (`MissingSpace`, `LineLength`,
`OneStatementPerLine`). Внешне отдельный код оставить.

## Вывод

Правило покрывает базовые переносы, но должно быть ближе к formatter и иметь
фиксы.

