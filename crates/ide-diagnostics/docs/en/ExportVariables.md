# Ban export global module variables (ExportVariables)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Exported module variables expose mutable state outside the module and make it hard to understand who reads or changes that state.

Because of that, such variables often lead to fragile code and hard-to-reproduce bugs. In most cases it is better to use method parameters, dedicated API methods, form attributes, or `AdditionalProperties`.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

```bsl
Variable FileConversion Export;
Procedure BeforeWrite(Cancel)

  If FileConversion Then
  ...

EndProcedure

```

For object modules, the standard recommendation is to pass external parameters through `AdditionalProperties`.

```bsl
Procedure BeforeWrite(Cancel)

  If AdditionalProperties.Property("FileConversion") Then 
  ...

EndProcedure

// script that calls the procedure
FileObject.AdditionalProperties.Insert("FileConversion", True);
FileObject.Write();
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->
<!-- Примеры источников -->

* [Standard: Using variables in modules (RU)](https://its.1c.ru/db/v8std/content/639/hdoc)
* [v8std.ru: #std639](https://v8std.ru/std/639/)
