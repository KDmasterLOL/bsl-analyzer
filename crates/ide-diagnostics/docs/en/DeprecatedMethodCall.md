# Deprecated methods should not be used (DeprecatedMethodCall)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

If a procedure or function is marked as deprecated in its documentation comment, new code should stop using it and switch to the recommended replacement.

A deprecation marker means that the method is kept only for backward compatibility and can be removed in future versions of the configuration.

Calls to deprecated methods from other deprecated methods are allowed. This makes it possible to preserve compatibility layers while the codebase is being migrated.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

```bsl
// Deprecated. Use CalculateAmountNew().
Function CalculateAmount()
    Return 0;
EndFunction

Function Total()
    Return CalculateAmount(); // Diagnostic is reported here
EndFunction

Function CalculateAmountNew()
    Return 0;
EndFunction
```

```bsl
// Deprecated. Use CalculateAmountNew().
Function CalculateAmount()
    Return 0;
EndFunction

// Compatibility wrapper may still call another deprecated method.
// Deprecated. Use TotalNew().
Function Total()
    Return CalculateAmount();
EndFunction

Function TotalNew()
    Return CalculateAmountNew();
EndFunction
```

## Sources

* Standard: [Procedures and functions description (RU)](https://its.1c.ru/db/v8std/content/453/hdoc)
* [CWE-477 Use of Obsolete Function](http://cwe.mitre.org/data/definitions/477.html)
