# Methods should not have too many return statements (TooManyReturns)

## Description

Too many `Return` statements make a method harder to read, refactor, and debug.

This diagnostic reports procedures and functions whose number of return statements exceeds the configured limit. The default limit in this project is `3`.

## Examples

Bad example

```bsl
Function Example(Condition)
    If Condition = 1 Then
        Return "Accepted";
    ElsIf Condition = 2 Then
        Return "Rejected";
    ElsIf Condition = 3 Then
        Return "Deferred";
    EndIf;
    Return "Unknown";
EndFunction
```

Better

```bsl
Function Example(Condition)
    Result = "Unknown";
    If Condition = 1 Then
        Result = "Accepted";
    ElsIf Condition = 2 Then
        Result = "Rejected";
    ElsIf Condition = 3 Then
        Result = "Deferred";
    EndIf;
    Return Result;
EndFunction
```

## Sources

* [Sonar rule S1142: Methods should not have too many return statements](https://rules.sonarsource.com/java/RSPEC-1142)
