# TimeoutsInExternalResources

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что внешние ресурсы создаются с явным таймаутом.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/timeouts_in_external_resources.rs`
- `<v8std mirror>/docs/diagnostics/bslls/TimeoutsInExternalResources.md`
- `<v8std mirror>/docs/std/748.md`

## Как реализовано

HIR-обход `Expr::New` для `FTPConnection`, `HTTPConnection`, `WSDefinitions`, `WSProxy`, `InternetMailProfile`. У каждого типа своя позиция параметра таймаута. Если конструктор без таймаута, проверяется последующее присваивание свойства таймаута целевой переменной.

## Что покрыто

Покрыты прямые и string constructors, русские/английские имена, настройка `analyzeInternetMailProfileZeroTimeout`.

## Пробелы и ограничения

Поиск последующего таймаута не path-sensitive и зависит от простого имени переменной. Aliases и helper methods не учитываются.

## Может ли инфраструктура улучшить качество

Да. Нужен resource initialization analysis с alias tracking и проверкой “таймаут установлен до использования”.

## Возможное объединение

Близко к `InternetAccess`, `FileSystemAccess`, `MissingTemporaryFileDeletion`: общий resource-safety analyzer.

## Вывод

Правило покрывает важный runtime-риск, но требует более строгой модели жизненного цикла объекта.
