# The region should not be empty (EmptyRegion)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->
Module code should not contain empty top-level or nested regions.

An empty region does not improve module structure and usually appears after code was moved away, deleted, or never implemented.

Regions that contain only comments are also treated as empty.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
```bsl
#Region EmptyRegion
#EndRegion
```

```bsl
#Region Helpers
Procedure Run()
EndProcedure
#EndRegion
```

## Sources

* [Standard: Module structure (RU)](https://its.1c.ru/db/v8std/content/455/hdoc)
* [v8std.ru: #std455](https://v8std.ru/std/455/)
