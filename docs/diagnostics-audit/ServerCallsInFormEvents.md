# ServerCallsInFormEvents

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Запрещает серверные вызовы из событий формы `OnActivateRow` / `OnStartChoice`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/server_calls_in_form_events.rs`
- `<v8std mirror>/docs/diagnostics/bslls/ServerCallsInFormEvents.md`

## Как реализовано

Работает только для `FormModule`. По `call_summary` ищет form entries нужных типов, затем BFS по локальным клиентским вызовам и прямым вызовам общих модулей. Серверные методы с контекстом дают diagnostic; путь через idle handler понижает severity до `Information`.

## Что покрыто

Покрыты цепочки локальных вызовов, прямые qualified common module calls, обработчики ожидания, ограничение глубины и visited cap.

## Пробелы и ограничения

Проверяются только два event type. Межфайловый обход фактически ограничен текущим файлом, кроме проверки прямого common module target. Динамические вызовы не покрываются.

## Может ли инфраструктура улучшить качество

Да. Нужен межмодульный call graph с effects `server with context` / `server no context` и точная модель событий форм.

## Возможное объединение

Близко к `TransferringParametersBetweenClientAndServer`, `ServerSideExportFormMethod`, `UsingSynchronousCalls`. Стоит строить общий client/server-boundary analyzer.

## Вывод

Правило уже использует call graph и покрывает главные цепочки, но качество ограничено неполным межмодульным анализом.
