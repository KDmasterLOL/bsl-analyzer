# UseLessForEach

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит `Для каждого` / `For Each`, где iterator не используется осмысленно в теле цикла.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/useless_for_each.rs`
- `<v8std mirror>/docs/diagnostics/bslls/UseLessForEach.md`

## Как реализовано

HIR lowering находит useless foreach и передает имя iterator/range. Handler дополнительно пропускает случай, если iterator name совпадает с module-level variable.

## Что покрыто

Покрыты неиспользованный iterator, использование в аргументе метода, RHS присваивания, присваивание самому iterator, доступ к свойствам, использование в условии и вызов метода у iterator. Прямой вызов имени итератора (`Итератор()`) использованием не считается.

## Пробелы и ограничения

Название кода нестандартное (`UseLessForEach` вместо `UselessForEach`). Пропуск по module-level variable имени может скрыть реальные нарушения.

## Может ли инфраструктура улучшить качество

Да. Нужен binding-aware анализ iterator usage вместо эвристики по имени.

## Возможное объединение

Близко к `UnusedLocalVariable`, `UnusedParameters`, `UselessTernaryOperator`. Общий unused/useless analyzer нужен.

## Вывод

Полезная ошибка, но стоит исправить naming/API debt и усилить binding analysis.
