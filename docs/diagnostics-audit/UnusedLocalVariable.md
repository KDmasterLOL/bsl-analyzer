# UnusedLocalVariable

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит локальные переменные, объявленные или присвоенные, но не прочитанные.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unused_local_variable.rs`
- `<v8std mirror>/docs/diagnostics/bslls/UnusedLocalVariable.md`

## Как реализовано

Использует module-level liveness и CFG. Отдельно строит набор имен атрибутов объекта/формы, которые нельзя считать локальными переменными. Проверяет методы, module-level code и явные module-level declarations.

## Что покрыто

Покрыты локальные переменные, параметры CFG/liveness, object/form attributes, стандартные свойства формы и module-level переменные.

## Пробелы и ограничения

Качество зависит от liveness, CFG и metadata. В form/object modules легко ошибиться без точного списка атрибутов и встроенных свойств.

## Может ли инфраструктура улучшить качество

Да. Улучшить symbol binding для form/object attributes и добавить safe delete/inline fixes.

## Возможное объединение

Близко к `UnusedParameters`, `UnusedLocalMethod`, `UnreachableCode`. Общий dead-code/liveness layer уже нужен.

## Вывод

Сильная dataflow-диагностика, но metadata binding для модулей форм/объектов остается ключевым ограничением.
