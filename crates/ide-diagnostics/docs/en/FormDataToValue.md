# FormDataToValue method call (FormDataToValue)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->
In most form-module scenarios, `FormAttributeToValue()` should be preferred over `FormDataToValue()`.

`FormAttributeToValue()` has simpler syntax because it does not require an explicit type argument. This makes the code shorter and reduces the chance of mistakes.

The current implementation reports `FormDataToValue()` calls only in methods that have form context. Methods marked `&AtServerNoContext` and `&AtClientAtServerNoContext` are not reported.
## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
```bsl
&AtServer
Procedure ProcessObject()
    DocumentObject = FormDataToValue(Object, Type("DocumentObject.SalesInvoice"));
EndProcedure
```

```bsl
&AtServer
Procedure ProcessObject()
    DocumentObject = FormAttributeToValue("Object");
EndProcedure
```
## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->

* [Using FormAttributeToValue() and FormDataToValue() (RU)](https://its.1c.ru/db/v8std/content/409/hdoc)
* [v8std.ru: #std409](https://v8std.ru/std/409/)
