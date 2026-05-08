# SelfInsertion

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит вставку коллекции в саму себя через `Добавить` / `Add` и `Вставить` / `Insert`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/self_insertion.rs`
- `<v8std mirror>/docs/diagnostics/bslls/SelfInsertion.md`

## Как реализовано

HIR lowering определяет локальные self-insertion паттерны; handler создает diagnostic по диапазону вызова.

## Что покрыто

Покрыты массивы и структуры/соответствия с русскими и английскими методами, разные объекты не срабатывают. Receiver сравнивается через общий `exprs_are_equal` — поля, индексы и method-call цепочки совпадают по структуре case-insensitive.

## Пробелы и ограничения

Срабатывание ограничено локальным эквивалентным выражением. Aliases (разные привязки к одному объекту) не отслеживаются. Нет fix для удаления вызова.

## Может ли инфраструктура улучшить качество

Да. Нужна alias-aware expression identity и safe delete-statement fix.

## Возможное объединение

Близко к `DuplicatedInsertionIntoCollection`, `SelfAssign`, `DeletingCollectionItem`. Общий collection misuse analyzer был бы полезен.

## Вывод

Правило ловит очевидный runtime/performance bug, но без alias analysis покрытие неполное.
