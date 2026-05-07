# ExportVariables

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Экспортные переменные модуля создают глобальное изменяемое состояние и
нежелательную связанность. Основание - `#std639`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/export_variables.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/ide-diagnostics/docs/ru/ExportVariables.md`
- `docs/legal/diagnostics/ExportVariables.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/639.md`

## Как реализовано

Module-level variables собираются в HIR `module_vars` с флагом `is_export`.
Handler создает diagnostic на имя экспортной переменной.

## Что покрыто

Тесты проверяют private variable, simple export, local variables внутри
процедуры, русские/английские keywords, несколько exported vars и commented
export.

## Пробелы и ограничения

- Нет quick-fix, потому что корректная замена требует getter/setter и обновления
  всех вызовов.
- Нет project-wide анализа фактических внешних чтений/записей.
- Сообщение говорит "глобальные переменные", хотя речь именно об экспортных
  module variables.

## Может ли инфраструктура улучшить качество

Нужен symbol index для exported vars и references, чтобы предлагать безопасный
refactoring plan.

## Возможное объединение

Близко к `GlobalVariables`/state design diagnostics, но сливать внешний код не
стоит. Внутренне можно использовать общий module-state analyzer.

## Вывод

Покрытие хорошее для синтаксического факта `Экспорт`. Улучшения касаются
сообщения и будущего refactoring support.

