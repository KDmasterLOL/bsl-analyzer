# NestedStatements

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Сообщает о слишком глубокой вложенности управляющих конструкций. Дефолтный лимит `maxAllowedLevel` равен 4.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/nested_statements.rs`
- `crates/ide-diagnostics/src/hir_dispatch.rs`
- `<v8std mirror>/docs/diagnostics/bslls/NestedStatements.md`

## Как реализовано

Глубина рассчитывается в HIR и приходит в `from_hir`. Handler только сравнивает `depth` с конфигом и создает diagnostic на диапазон нарушающей конструкции.

## Что покрыто

Покрыты `Если`, циклы и вложенность с участием `Попытка` по HIR-модели. Тесты проверяют ровно лимит, превышение и изменение `maxAllowedLevel`.

## Пробелы и ограничения

Не все конструкции одинаково ухудшают читаемость, но правило использует один счетчик. Нет подсказок по guard clauses, early return или extraction. Глубина не связана напрямую с `CognitiveComplexity`.

## Может ли инфраструктура улучшить качество

Да. Общий control-flow metrics layer мог бы отдавать не только depth, но и причины роста сложности, чтобы давать точные рекомендации.

## Возможное объединение

Близко к `CognitiveComplexity`, `CyclomaticComplexity`, `IfConditionComplexity`, `NestedTernaryOperator`. Публично лучше оставить отдельным простым правилом, но считать метрики в одном месте.

## Вывод

Диагностика простая и предсказуемая. Для улучшения нужны объяснимые рекомендации, а не просто число вложенности.
