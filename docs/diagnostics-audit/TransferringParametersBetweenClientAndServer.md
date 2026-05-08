# TransferringParametersBetweenClientAndServer

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит server methods, вызываемые с клиента, где параметры передаются по ссылке, но не изменяются, поэтому стоит добавить `Знач`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/transferring_parameters_between_client_and_server.rs`
- `<v8std mirror>/docs/diagnostics/bslls/TransferringParametersBetweenClientAndServer.md`

## Как реализовано

Собирает серверные методы `&НаСервере` / `&НаСервереБезКонтекста`, by-ref параметры и проверяет наличие direct local вызова из `&НаКлиенте` метода по `call_summary`. Затем в теле серверного метода ищет присваивания параметрам; если параметр не меняется, diagnostic ставится на имя параметра.

## Что покрыто

Покрыты локальные client-to-server вызовы, процедуры и функции, присваивания в ветках/циклах/try через рекурсивный обход statements.

## Пробелы и ограничения

Покрыты только direct local calls и присваивания вида `Парам = ...`. Мутация через `.поле`/`[idx]`, передача в by-ref helper или вызов мутирующего метода на параметре не учитывается.

## Может ли инфраструктура улучшить качество

Да. Нужен interprocedural client/server call graph и alias/effect analysis параметров.

## Возможное объединение

Близко к `ServerCallsInFormEvents`, `ServerSideExportFormMethod`, `UnusedParameters`, `RewriteMethodParameter`. Общий parameter/effect analyzer нужен.

## Вывод

Правило использует call graph, но пока ограничено прямыми локальными вызовами и простым анализом присваиваний.
