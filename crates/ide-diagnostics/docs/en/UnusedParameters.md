# Unused parameter (UnusedParameters)

## Description

Methods should not declare parameters that are never used in the method body.

Unused parameters make the API harder to understand and complicate call sites. The current implementation skips several categories of methods that are expected to have fixed signatures, such as platform handlers, HTTP handlers, attachable methods, and local callbacks registered through `NotifyDescription`.

## Examples

Incorrect:

```bsl
Function AddTwoNumbers(Val FirstValue, Val SecondValue, Val UnusedParameter)
    Return FirstValue + SecondValue;
EndFunction
```

Correct:

```bsl
Function AddTwoNumbers(Val FirstValue, Val SecondValue)
    Return FirstValue + SecondValue;
EndFunction
```

## Sources

* [v8std: UnusedParameters](https://v8std.ru/diagnostics/bslls/UnusedParameters/)
