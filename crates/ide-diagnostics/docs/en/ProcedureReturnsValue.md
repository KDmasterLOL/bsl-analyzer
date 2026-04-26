# Procedure should not return value (ProcedureReturnsValue)

## Description

This diagnostic reports `Return` statements with values inside procedures.

In BSL, only functions may return a value. A procedure may use `Return;` to
exit early, but it must not return an expression.

## Examples

Incorrect:

```bsl
Procedure GetName()
    Return "Test";
EndProcedure
```

Correct:

```bsl
Function GetName()
    Return "Test";
EndFunction
```

```bsl
Procedure StopEarly()
    Return;
EndProcedure
```
