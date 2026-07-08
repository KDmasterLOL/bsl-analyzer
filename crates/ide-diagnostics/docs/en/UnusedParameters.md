# Unused parameter (UnusedParameters)

## Description

Methods should not declare parameters that are never used in the method body.

Unused parameters make the API harder to understand and complicate call sites. The current implementation skips several categories of methods that are expected to have fixed signatures, such as platform handlers, HTTP handlers, attachable methods, and local callbacks registered through `NotifyDescription`.

In form modules a method bound as a handler at runtime keeps its platform-fixed signature and is skipped as well: both direct `УстановитьДействие` registrations and any method named by an identifier-shaped string literal in the same module (a command created in code, a helper module fed a parameter structure). String data that coincides with a method name therefore also exempts that method's parameters.

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
