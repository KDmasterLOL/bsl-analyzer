# WrongDataPathForFormElements

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит элементы формы, у которых путь к данным не разрешился и в metadata представлен с префиксом `~`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/wrong_data_path_for_form_elements.rs`
- `<v8std mirror>/docs/diagnostics/bslls/WrongDataPathForFormElements.md`
- `<v8std mirror>/docs/std/467.md`

## Как реализовано

Metadata dispatch запускается только для `FormModule`. Handler берет `metadata.form`, перебирает `elements_with_wrong_data_path()` и создает diagnostic. Range сейчас ставится в начало файла `[0..min(9, len)]`.

## Что покрыто

Покрыты формы с одним или несколькими элементами с некорректным `DataPath`; не-form модули и корректные paths игнорируются.

## Пробелы и ограничения

Главное ограничение - нет точного range в XML/форме: diagnostic ставится в начало модуля, что плохо для UX. Правило зависит от уже подготовленного metadata parser и не проверяет путь самостоятельно. Metadata декларирует модули `FormModule` + `ManagedApplicationModule`, но handler фильтрует только `FormModule` - расхождение между декларацией и runtime.

## Может ли инфраструктура улучшить качество

Да. Нужно хранить source range элемента формы/DataPath в metadata и уметь диагностировать именно `.form`/XML, а не начало `.bsl` модуля.

## Возможное объединение

Близко к metadata consistency diagnostics, но не к BSL-правилам. Можно объединить инфраструктуру range mapping с `WrongHttpServiceHandler` и `WrongWebServiceHandler`, где тоже есть metadata-derived ошибки.

## Вывод

Логика правила корректная, но без точного location качество диагностики ограничено.
