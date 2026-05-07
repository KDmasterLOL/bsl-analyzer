# DeprecatedMethods8317

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Ловит методы обработки ошибок и `ПолучитьФорму`/`GetForm`, устаревшие в
семействе правил 8.3.17, с replacements через `МенеджерОбработкиОшибок` или
`ОткрытьФорму`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deprecated_method.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/DeprecatedMethods8317.md`
- `docs/legal/diagnostics/DeprecatedMethods8317.md`

## Как реализовано

Тот же `deprecated_method::from_hir()` выбирает code
`DeprecatedMethods8317`, если имя найдено в `get_8317_replacement()`.

## Что покрыто

Тесты покрывают русские/английские methods 8.3.17 и совместный сценарий с
8.3.10.

## Пробелы и ограничения

- `ПолучитьФорму` также имеет отдельную диагностику `GetFormMethod` (Major/ERROR
  vs Info здесь); 8317 ловит только bare-call, а `GetFormMethod` — и
  `obj.ПолучитьФорму()`. На bare-call оба правила срабатывают одновременно.
- Replacement `ОткрытьФорму` не всегда семантически эквивалентен
  `ПолучитьФорму`.
- Нет quick-fix и context-aware suggestions.
- Таблица устаревших методов в коде отделена от других deprecated diagnostics.

## Инфраструктурные улучшения

Нужна дедупликация с `GetFormMethod`: единая карточка API item может
порождать несколько diagnostics или подавлять один из них по приоритету.

## Возможное объединение

Внутренне объединить с `DeprecatedMethods8310` через registry. С
`GetFormMethod` нужно решить policy: либо одно правило является legacy alias,
либо одно suppress'ит другое.

## Вывод

Правило полезное, но есть риск дублирования вокруг `ПолучитьФорму`. Нужен
приоритет deprecated API diagnostics.

