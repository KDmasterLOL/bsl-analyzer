# Code out of region (CodeOutOfRegion)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
Module code should be organized into regions (`#Region` / `#EndRegion` or
`#Область` / `#КонецОбласти`) according to the standard module structure.

This diagnostic reports module-level declarations and executable statements that
are placed outside any region. The goal is not to validate every nested region
name, but to enforce that the top-level module structure is explicitly grouped.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

### Correct

```bsl
#Region Private
#Region Print
// Methods code
#EndRegion
#Region Other
// Methods code
#EndRegion
#EndRegion
```

### Standard top-level region names

| RU  | EN |
| ------------- | ------------- |
| ПрограммныйИнтерфейс  | Public  |
| СлужебныйПрограммныйИнтерфейс  | Internal  |
| СлужебныеПроцедурыИФункции  | Private  |
| ОбработчикиСобытий  | EventHandlers  |
| ОбработчикиСобытийФормы  | FormEventHandlers  |
| ОбработчикиСобытийЭлементовШапкиФормы  | FormHeaderItemsEventHandlers  |
| ОбработчикиКомандФормы  | FormCommandsEventHandlers  |
| ОписаниеПеременных  | Variables  |
| Инициализация  | Initialize  |
| ОбработчикиСобытийЭлементовТаблицыФормы  | FormTableItemsEventHandlers  |

## Sources

Primary source: [Standard: Module structure (RU)](https://its.1c.ru/db/v8std#content:455:hdoc)

Secondary source: [v8std.ru: #std455 Module structure](https://v8std.ru/std/455/)

Additional reference: [v8std.ru: CodeOutOfRegion](https://v8std.ru/diagnostics/bslls/CodeOutOfRegion/)
