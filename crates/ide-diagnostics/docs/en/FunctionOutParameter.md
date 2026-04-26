# Out function parameter (FunctionOutParameter)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
Functions should return results through `Return` instead of modifying by-reference parameters.

The current implementation reports only a narrow case:

- the enclosing routine is a `Function`;
- the parameter is passed by reference, that is, it has no `Val` / `Знач` modifier;
- the function directly assigns to that parameter name.

It does not report procedures, `Val` parameters, or assignments to fields and properties of a parameter object.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

```bsl
// Incorrect:
Function FillConnectionParameters(ServiceURL, UserName, UserPassword)
    ServiceURL = Settings.ServiceURL;
    UserName = Settings.UserName;
    UserPassword = Settings.UserPassword;
    Return True;
EndFunction

// Correct:
Function FillConnectionParameters()
    Result = New Structure;
    Result.Insert("ServiceURL", Settings.ServiceURL);
    Result.Insert("UserName", Settings.UserName);
    Result.Insert("UserPassword", Settings.UserPassword);
    Return Result;
EndFunction
```

## Sources
- [v8std.ru: FunctionOutParameter (RU)](https://v8std.ru/diagnostics/bslls/FunctionOutParameter/)
