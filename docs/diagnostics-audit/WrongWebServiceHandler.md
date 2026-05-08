# WrongWebServiceHandler

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что у операции web-сервиса задан существующий обработчик.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/wrong_web_service_handler.rs`
- `<v8std mirror>/docs/diagnostics/bslls/WrongWebServiceHandler.md`

## Как реализовано

Metadata handler работает только для `WebServiceModule`, перебирает операции, берет `procedure_name()`, проверяет пустое имя и поиск метода в `symbol_tree`. Diagnostic для metadata-only ошибок ставится в начало файла.

## Что покрыто

Покрыты пустой обработчик, несуществующий обработчик, валидный обработчик и несколько операций в одном сервисе.

## Пробелы и ограничения

Нет проверки сигнатуры операции, параметров, типа функции/процедуры и возвращаемого значения. Нет точного range в metadata для имени обработчика.

## Может ли инфраструктура улучшить качество

Да. Нужен source mapping web-service metadata и contract checker по описанию операции.

## Возможное объединение

Сильный кандидат на общий framework с `WrongHttpServiceHandler`: resolver handler name -> symbol, metadata range, signature contract, message builder.

## Вывод

Текущая проверка ловит самые частые ошибки привязки, но не весь контракт web-service operation.
