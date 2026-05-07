# IfElseDuplicatedCondition

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Повтор условия в цепочке `Если`/`ИначеЕсли` делает вторую ветку недостижимой.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/if_else_duplicated_condition.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/ide-diagnostics/docs/ru/IfElseDuplicatedCondition.md`
- `docs/legal/diagnostics/IfElseDuplicatedCondition.md`

## Как реализовано

HIR lowering собирает условия веток и сравнивает их по нормализованному тексту
(whitespace удаляется, идентификаторы lowercase, литералы строк остаются
case-sensitive). Handler сообщает позицию первого вхождения.

## Что покрыто

Тесты проверяют простой дубликат, отсутствие дубликата, case-insensitive
variables и whitespace normalization.

## Пробелы и ограничения

- Нет более глубокой логической эквивалентности: `A = 1` и `1 = A` не
  схлопываются (нормализация текстовая, без сортировки операндов).
- Не ловятся включающие условия (`A > 10`, затем `A > 5`) как unreachable.
- Нет quick-fix удаления ветки или изменения условия.

## Может ли инфраструктура улучшить качество

Expression canonicalizer и optional condition implication analysis.

## Возможное объединение

Внутренне с `IdenticalExpressions` через expression equality. Внешне отдельный
код полезен, потому что проблема именно unreachable branch.

## Вывод

Хорошая точечная диагностика. Следующий уровень - implication analysis, но это
существенно сложнее.

