# WrongHttpServiceHandler

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет обработчики методов HTTP-сервиса: обработчик должен быть задан, существовать в модуле и иметь ровно один параметр.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/wrong_http_service_handler.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/WrongHttpServiceHandler.md`

## Как реализовано

Metadata handler работает только для `HTTPServiceModule`, перебирает `http_service.all_methods()`, берет `method.handler()`, ищет метод в `symbol_tree` и проверяет количество параметров. Для неверного количества параметров range ставится на имя метода, для отсутствующего handler - в начало файла.

## Что покрыто

Покрыты пустой handler, handler с несуществующим именем, handler с неверным числом параметров и валидный handler с одним параметром.

## Пробелы и ограничения

Нет проверки, что handler является функцией/процедурой нужного типа и возвращает корректный HTTP-ответ. Нет точного range в metadata для пустого/неверного имени обработчика.

## Может ли инфраструктура улучшить качество

Да. Нужен source mapping HTTP-service metadata, type/signature contract для handler и проверка возвращаемого значения.

## Возможное объединение

Сильный кандидат на объединение с `WrongWebServiceHandler`: обе diagnostics проверяют metadata-declared handler against symbol tree. Можно сделать общий handler-resolution framework с разными contract adapters.

## Вывод

Базовая проверка HTTP handler работает, но качество UX и полнота контракта зависят от metadata source mapping и сигнатурного анализа.
