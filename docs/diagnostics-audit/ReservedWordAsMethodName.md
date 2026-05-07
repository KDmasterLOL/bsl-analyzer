# ReservedWordAsMethodName

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит процедуры и функции, названные зарезервированными словами BSL.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/reserved_word_as_method_name.rs`
- `crates/ide-diagnostics/src/hir_dispatch.rs`

## Как реализовано

HIR lowering определяет reserved method names и передает имя/range в handler. Handler создает blocker diagnostic.

## Что покрыто

Покрыты русские и английские ключевые слова, процедуры и функции, нормальные имена не срабатывают.

## Пробелы и ограничения

Нет rename fix. Качество зависит от списка reserved keywords в HIR/parser.

## Может ли инфраструктура улучшить качество

Да. Нужен общий keyword service и rename action для метода с обновлением вызовов.

## Возможное объединение

Близко к `ReservedParameterNames` и naming diagnostics. Можно объединить lookup keywords/reserved names.

## Вывод

Диагностика точная, но исправление требует project-wide rename.
