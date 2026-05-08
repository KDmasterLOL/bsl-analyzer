# RewriteMethodParameter

Статус: `done`, `needs-code-work`
Track 1 closure: scope-included, no code change (already uses `ctx.by_value_params` per plan §4.5) — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Находит переприсваивание параметра метода до его осмысленного использования.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/rewrite_method_parameter.rs`
- `<v8std mirror>/docs/diagnostics/bslls/RewriteMethodParameter.md`

## Как реализовано

HIR передает candidate assignment. Handler находит тело метода, statement range, reaching definitions и проверяет, что до присваивания доходит только исходное определение параметра. Дополнительно исключает использование параметра в RHS и предыдущее meaningful use.

## Что покрыто

Покрыты by-value параметры, self-assign исключения, RHS-use и использование в предыдущих statements/branches по HIR-обходу.

## Пробелы и ограничения

Сложные alias/dataflow случаи и межпроцедурные эффекты не покрываются. Нет fix для введения локальной переменной.

## Может ли инфраструктура улучшить качество

Да. Dataflow уже используется; следующий шаг - точнее path-sensitive use-before-overwrite и extract-local code action.

## Возможное объединение

Близко к `SelfAssign`, `UnusedParameters`, `FunctionOutParameter`. Общий parameter/dataflow analyzer будет полезен.

## Вывод

Правило зрелее простых синтаксических проверок, но требует безопасного refactoring fix.
