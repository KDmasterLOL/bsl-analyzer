# Executing of external code on the server (ExecuteExternalCode)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->

Using `Execute` or `Eval` in server-side code is dangerous because the executed string can be influenced by input parameters and lead to arbitrary code execution on the server.

This diagnostic reports such calls in server methods of form, command, object and similar modules.

Client-only code is excluded. Common modules are covered by a separate diagnostic.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

```bsl
&AtServer
Procedure RunExpressionOnServer(CodeText)
    Execute(CodeText);
EndProcedure
```

```bsl
&AtClient
Procedure RunExpressionOnClient(CodeText)
    Execute(CodeText);
EndProcedure
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->


* [Restrictions on the use of Execute and Eval on the server (RU)](https://its.1c.ru/db/v8std/content/770/hdoc)
* [v8std.ru: #std770](https://v8std.ru/std/770/)
