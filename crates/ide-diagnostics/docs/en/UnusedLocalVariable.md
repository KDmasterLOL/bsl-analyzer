# Unused local variable (UnusedLocalVariable)

## Description

Local variables that are declared or assigned but never read should be removed.

Such variables are dead code. They make the method harder to read and often remain after incomplete refactoring.

The current implementation uses control-flow and liveness analysis, so it is not limited to simple text matching.

## Examples

Incorrect:

```bsl
Procedure ProcessData()
    TemporaryValue = 42;
    DoMainAction();
EndProcedure
```

Correct:

```bsl
Procedure ProcessData()
    DoMainAction();
EndProcedure
```
