# Usage of complex expressions in the "If" condition (IfConditionComplexity)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports `If` and `ElseIf` conditions that contain too many
boolean operations.

The default threshold in `bsl-analyzer` is `3`. When a condition becomes more
complex than that, it is usually easier to read and maintain if you extract
part of the logic into a helper function or an intermediate variable with a
meaningful name.

## Examples

Bad:

```bsl
If Id = "Expr1"
    Or Id = "Expr2"
    Or Id = "Expr3"
    Or Id = "Expr4"
    Or Id = "Expr5"
    Or Id = "Expr6"
    Or Id = "Expr7"
    Or Id = "Expr8"
    Or Id = "Expr9" Then

   doSomeWork();

EndIf; 
```

Good:

```bsl
If IsCorrectId(Id) Then
   doSomeWork();
КонецЕсли;

Function IsCorrectId(Id)

    Return Id = "Expr1"
            Or Id = "Expr2"
            Or Id = "Expr3"
            Or Id = "Expr4"
            Or Id = "Expr5"
            Or Id = "Expr6"
            Or Id = "Expr7"
            Or Id = "Expr8"
            Or Id = "Expr9";

EndFunction
```

## Sources

No direct 1C standard is used as the normative basis for this diagnostic.
It is a local maintainability rule with a configurable threshold.
