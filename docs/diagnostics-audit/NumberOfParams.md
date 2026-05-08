# NumberOfParams

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Ограничивает общее количество параметров метода. Дефолт `maxParamsCount` равен 7.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/number_of_params.rs`
- `<v8std mirror>/docs/diagnostics/bslls/NumberOfParams.md`
- `<v8std mirror>/docs/std/640.md`

## Как реализовано

HIR считает параметры метода и вызывает handler с count и range имени метода. Handler применяет конфиг и формирует сообщение.

## Что покрыто

Покрыты функции и процедуры, custom threshold, отсутствие параметров и значение ровно на лимите.

## Пробелы и ограничения

Нет различения обязательных/опциональных, API/internal, callbacks и handlers. Нет предложения свернуть параметры в структуру или объект.

## Может ли инфраструктура улучшить качество

Да. Улучшение возможно через классификацию типа метода и совместную работу с `NumberOfOptionalParams`/`OrderOfParams`.

## Возможное объединение

Близко к `NumberOfOptionalParams`; можно объединить реализацию счетчиков, но оставить разные rule ids.

## Вывод

Простая maintainability-метрика. Полезна как сигнал, но не объясняет оптимальный refactoring.
