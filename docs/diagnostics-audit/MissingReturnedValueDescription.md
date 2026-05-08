# MissingReturnedValueDescription

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет описание возвращаемого значения функций и запрещает return-section у процедур.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_returned_value_description.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `<v8std mirror>/docs/diagnostics/bslls/MissingReturnedValueDescription.md`
- `<v8std mirror>/docs/std/453.md`

## Как реализовано

Handler берет method docs, пропускает hyperlink docs и проверяет функции/процедуры отдельно. Экспортная функция с существующим doc-comment должна иметь return section. Конфиг `allowShortDescriptionReturnValues` управляет строгостью описания типов.

## Что покрыто

Покрыты функции с комментариями без return section, пустые return descriptions, процедура с return description, а также строгий режим, где все типы должны иметь описание.

## Пробелы и ограничения

Экспортная функция без комментария целиком не диагностируется здесь, чтобы не дублировать `PublicMethodsDescription`; если последняя отключена в конфиге, кейс «нет комментария вовсе» вообще не покрывается ни одной диагностикой. Нет генерации return-section и нет проверки соответствия фактических return expressions описанному типу.

## Может ли инфраструктура улучшить качество

Да. Совмещение docs model с type inference позволило бы проверять не только наличие описания, но и его соответствие возвращаемым значениям.

## Возможное объединение

Близко к `MissingParameterDescription` и `PublicMethodsDescription`. Внутренне стоит объединить документационный движок; снаружи отдельный код полезен для точной настройки severity.

## Вывод

Диагностика хорошо отделяет missing docs от no-comment случая, но дальнейшее качество требует связи с inferred return types.
