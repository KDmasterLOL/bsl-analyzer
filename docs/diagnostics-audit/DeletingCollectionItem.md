# DeletingCollectionItem

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Удаление элементов из коллекции во время `Для Каждого` по этой же коллекции
может пропускать элементы или ломать обход.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deleting_collection_item.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/hir-def/src/body/lower/diagnostics.rs`
- `crates/ide-diagnostics/docs/ru/DeletingCollectionItem.md`
- `docs/legal/diagnostics/DeletingCollectionItem.md`

## Как реализовано

Lowering запоминает коллекцию текущего `ForEach`. При вызове
`collection.Delete(...)` / `Удалить(...)` на совпадающем receiver эмитится
diagnostic. Есть исключение: если delete сразу сопровождается выходом из loop
(`Break`/`Return`), паттерн считается безопасным.

## Что покрыто

Тесты покрывают same/different collection, global `Удалить`, English,
case-insensitive chained field, nested block, parenthesized arg и safe exit
cases.

## Пробелы и ограничения

- Сравнение receiver зависит от структурного совпадения HIR expression; alias
  коллекции и параметры не отслеживаются dataflow'ом.
- Не все safe exits равнозначны: `Return` внутри вложенной процедуры невозможен,
  но `Break` внутри вложенного цикла может относиться не к тому loop.
- Не распознаются методы удаления через wrappers или indexed receiver aliases.
- Нет quick-fix, потому что безопасный вариант зависит от порядка коллекции.

## Инфраструктурные улучшения

Расширить loop-context stack: id цикла, безопасный exit именно из этого цикла,
aliases коллекций. Это пригодится и другим loop diagnostics.

## Возможное объединение

С `DuplicatedInsertionIntoCollection` и `SelfInsertion` можно разделить общий
collection-mutation analyzer. Внешние коды лучше оставить разными.

## Вывод

Одна из лучше покрытых HIR diagnostics в этой пачке. Главный будущий шаг -
alias/dataflow support.

