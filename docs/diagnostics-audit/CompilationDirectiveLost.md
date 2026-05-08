# CompilationDirectiveLost

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

В модулях форм и команд процедуры/функции должны иметь директиву компиляции
`&НаСервере`, `&НаКлиенте` и т.п. Основание - `#std439` и связанные материалы
v8-code-style про form-module pragma.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/compilation_directive_lost.rs`
- `crates/ide-diagnostics/docs/ru/CompilationDirectiveLost.md`,
  `crates/ide-diagnostics/docs/en/CompilationDirectiveLost.md`
- `docs/legal/diagnostics/CompilationDirectiveLost.md`
- `<v8std mirror>/docs/std/439.md`,
  `<v8std mirror>/docs/diagnostics/bslls/CompilationDirectiveLost.md`

## Как реализовано

Handler получает `module_metadata()`, применим только для `FormModule` и
`CommandModule`, затем через `ItemTree` проверяет пустой список annotations у
каждой процедуры/функции.

## Что покрыто

Тесты создают path для FormModule и проверяют методы с директивой, без
директивы, mixed case, English annotations и несколько пропусков.

## Пробелы и ограничения

- Любая annotation считается достаточной? В коде проверяется именно
  `annotations.is_empty()`, а не "есть compilation directive". Если ItemTree
  положит туда non-compilation annotation, возможен пропуск.
- Не проверяется корректность конкретной директивы для события формы/команды.
- Определение module type в тестах зависит от path heuristic.
- Нет quick-fix по выбору директивы, потому что нужен execution-context
  inference.

## Инфраструктурные улучшения

Использовать общий helper `is_compilation_directive`, как в
`CompilationDirectiveNeedLess`, и хранить annotation kinds в ItemTree. Для
quick-fix нужен анализ вызываемых API/контекста формы.

## Возможное объединение

С `CompilationDirectiveNeedLess` это две стороны одного policy: где директивы
обязательны и где запрещены. Внешне коды лучше оставить разными, внутренне
объединить module-type policy.

## Вывод

Правило хорошо встроено в ItemTree, но проверка "annotations empty" слишком
широкая. Нужна явная проверка вида annotation.

