# Duplicate regions (DuplicateRegion)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->
Module structure should stay predictable, so each top-level region should appear only once.

Repeated regions make navigation harder and often mean that related code was split accidentally or merged without consolidation.

The diagnostic also treats standard Russian and English region names as the same logical section. For example, `#Область ПрограммныйИнтерфейс` and `#Region Public` are considered duplicates.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

```bsl
#Область ОбработчикиСобытий
// ...
#КонецОбласти

#Region EventHandlers
// ...
#EndRegion
```

```bsl
#Область ОбработчикиСобытий
// ...
// ...
#КонецОбласти
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->


* [Standard: Module structure (RU)](https://its.1c.ru/db/v8std/content/455/hdoc)
* [v8std.ru: #std455](https://v8std.ru/std/455/)
