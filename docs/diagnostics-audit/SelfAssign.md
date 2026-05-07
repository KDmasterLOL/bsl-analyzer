# SelfAssign

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит присваивание переменной или поля самому себе.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/self_assign.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/SelfAssign.md`

## Как реализовано

HIR lowering определяет self-assignment и передает range; handler создает simple diagnostic.

## Что покрыто

Покрыты case-insensitive имена и field self-assign вроде `Структура.Чтото = СтруКтура.ЧТото`. `exprs_are_equal` также сравнивает `QualifiedPath`, `Index`, `MethodCall`/`Call` (аргументы игнорируются).

## Пробелы и ограничения

Нет fix для удаления строки. Aliases (разные привязки к одному объекту) не отслеживаются.

## Может ли инфраструктура улучшить качество

Да. Нужна нормализация lvalue/rvalue expressions и safe delete-statement fix.

## Возможное объединение

Близко к `RewriteMethodParameter`, `SelfInsertion`, `IdenticalExpressions`. Общий expression-equivalence helper полезен.

## Вывод

Простой и точный bug pattern; улучшение - в расширении equivalence и auto-fix.
