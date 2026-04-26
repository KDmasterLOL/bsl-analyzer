# The function always returns the same primitive value (FunctionReturnsSamePrimitive)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
If every return branch of a function yields the same primitive literal, the return value usually carries no useful information. Such code is often clearer as a procedure or as a function with actually distinct return cases.

The current implementation is narrower than that general idea. It reports only functions where:

- there is more than one `Return`;
- all collected return values are primitive literals of the same kind and value;
- the function is not treated as an attachable function.

Returning the same variable or expression is not covered by this rule.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

Bad:
```bsl
Function CheckString(Val RowTable)

    If ItsGoodString(RowTable) Then
        ActionGood();
        Return True;
    ElsIf ItsNodBadString(RowTable) Then
        ActionNoBad();
        Return True;
     Else
        Return True;
    EndIf;

EndFunction
```

Good:
```bsl
Procedure CheckString(Val RowTable)

    If ItsGoodString(RowTable) Then
        ActionGood();
    ElsIf ItsNodBadString(RowTable) Then
        ActionNoBad();
    Else
        ActionElse();
    EndIf;

EndProcedure
```

## Nuances

Attachable functions excluded from the scan. Example:
```bsl
Function Attachable_RandomAction(Command)

    If ValueIsFilled(CurrentDate) Then
        Return Undefined;
    EndIf;

    Return Undefined;

EndFunction
```

## Sources
- [v8std.ru: FunctionReturnsSamePrimitive (RU)](https://v8std.ru/diagnostics/bslls/FunctionReturnsSamePrimitive/)
