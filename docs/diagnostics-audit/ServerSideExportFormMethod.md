# ServerSideExportFormMethod

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Запрещает экспортные методы управляемой формы, если они не помечены `&НаКлиенте`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/server_side_export_form_method.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/ServerSideExportFormMethod.md`

## Как реализовано

Проверяет только `FormModule` с `FormType::Managed`. В `item_tree` все экспортные процедуры/функции без annotation `AtClient` получают blocker diagnostic.

## Что покрыто

Покрыты функции и процедуры формы, управляемые формы и исключение для явного `&НаКлиенте`.

## Пробелы и ограничения

Правило не отличает `&НаСервереБезКонтекста` от серверного контекстного метода: оба запрещены, если export. Нет fix для снятия `Экспорт` или добавления `&НаКлиенте`. У handler'а нет unit-тестов — поведение покрыто только косвенно через runner.

## Может ли инфраструктура улучшить качество

Да. Нужен form-specific quick fix с выбором между удалением export и переносом API на клиент.

## Возможное объединение

Близко к `ServerCallsInFormEvents` и `TransferringParametersBetweenClientAndServer`. Это часть client/server policy для форм.

## Вывод

Простая и точная проверка по metadata формы. Улучшать стоит remediation, а не detection.
