# Executing of external code in a common module on the server (ExecuteExternalCodeInCommonModule)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->

This diagnostic reports `Execute` and `Eval` usage inside common modules that run in a risky context: on the server, through external connection, or in an ordinary client application when that mode is enabled in the configuration.

In such modules, dynamic code execution is a security hotspot because the executed string can be influenced by external input and run with elevated access to the application environment.

Managed client-only common modules are not reported by this diagnostic.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

```bsl
Function RunExpression(CodeText) Export
    Return Eval(CodeText);
EndFunction
```

```bsl
Procedure RunAlgorithm(CodeText) Export
    CommonPurpose.ExecuteInSafeMode(CodeText);
EndProcedure
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->


* [Restrictions on the use of Execute and Eval on the server (RU)](https://its.1c.ru/db/v8std/content/770/hdoc)
* [v8std.ru: #std770](https://v8std.ru/std/770/)
