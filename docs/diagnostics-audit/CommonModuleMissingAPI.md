# CommonModuleMissingAPI

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Общие модули и модули менеджера с методами должны явно иметь программный
интерфейс: экспортные методы и API-область. Правило связано со структурой
модуля из `#std455`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_missing_api.rs`
- `crates/ide-diagnostics/docs/ru/CommonModuleMissingAPI.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleMissingAPI.md`
- `docs/legal/diagnostics/CommonModuleMissingAPI.md`
- `<v8std mirror>/docs/std/455.md`,
  `<v8std mirror>/docs/diagnostics/bslls/CommonModuleMissingAPI.md`

## Как реализовано

AST traversal ищет любые процедуры/функции, любой `Экспорт` и любую область с
именем `ПрограммныйИнтерфейс`, `Public`, `СлужебныйПрограммныйИнтерфейс` или
`Internal`. Если методы есть, но нет export или API region, diagnostic ставится
на весь модуль.

## Что покрыто

Тесты проверяют валидный модуль, отсутствие export, отсутствие API region,
модуль без методов и игнорирование неподходящих module types.

## Пробелы и ограничения

- Проверяется только наличие export и API region где-то в модуле. Не
  проверяется, что экспортные методы действительно находятся внутри API region.
- Нет разделения public/internal API и требований к описанию методов.
- Используется raw AST, хотя рядом есть `RegionTree`, `ItemTree` и правила
  `NonExportMethodsInApiRegion`, `PublicMethodsDescription`.
- Diagnostic один и тот же для двух разных причин: нет export или нет области.
- Нет quick-fix для создания области или перемещения метода.

## Инфраструктурные улучшения

Нужен общий `module_api_layout` анализ: топовые области, методы внутри областей,
export flag, docs и module type. Он пригодится также для `CachedPublic`,
`DuplicateRegion`, `NonExportMethodsInApiRegion`, `PublicMethodsDescription`.

## Возможное объединение

Внешне лучше оставить отдельным правилом. Внутренне его стоит объединить с
регионными/API diagnostics через общий анализ структуры модуля, иначе разные
правила будут по-разному понимать "API область".

## Вывод

Сейчас правило ловит грубую проблему, но не доказывает корректную структуру
API. Главный следующий шаг - перейти от "есть где-то" к связи method -> region.

