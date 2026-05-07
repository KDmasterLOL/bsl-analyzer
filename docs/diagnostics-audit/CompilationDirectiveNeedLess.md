# CompilationDirectiveNeedLess

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

В модулях, где контекст выполнения задан самим типом модуля, директивы
компиляции у методов избыточны. Основание - `#std439`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/compilation_directive_need_less.rs`
- `crates/ide-diagnostics/docs/ru/CompilationDirectiveNeedLess.md`
- `docs/legal/diagnostics/CompilationDirectiveNeedLess.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/439.md`

## Как реализовано

Handler применим к списку module types, исключая FormModule и CommandModule.
Через `ItemTree` проходит annotations процедур/функций и диагностирует только
compilation directive kinds.

## Что покрыто

Тесты проверяют redundant directives в object module, отсутствие директив,
extension annotation `&Вместо`, command module и unknown module.

## Пробелы и ограничения

- Список applicable module types захардкожен в metadata и в `matches!`; риск
  рассинхрона при добавлении типов.
- Нет тестов на все compilation directive variants и английские варианты.
- Нет quick-fix удаления annotation.
- Нет общей policy с `CompilationDirectiveLost`, поэтому правила могут
  расходиться по спискам module types.

## Инфраструктурные улучшения

Вынести `CompilationDirectivePolicy` с состояниями `Required`, `Forbidden`,
`Allowed` по module type и annotation kind. Тогда оба правила используют одну
таблицу.

## Возможное объединение

Внутренне стоит объединить с `CompilationDirectiveLost`; внешний diagnostic
code лучше оставить отдельным, потому что remediation противоположная.

## Вывод

Реализация точнее, чем у `CompilationDirectiveLost`, потому что проверяет kind.
Основной долг - единая module-type policy и quick-fix удаления.

