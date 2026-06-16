# Unused local variable (UnusedLocalVariable)

## Description

Local variables that are declared or assigned but never read should be removed.

Such variables are dead code. They make the method harder to read and often remain after incomplete refactoring.

A variable is considered unused when its name never appears in a read position anywhere in the method body (assignment targets and `Перем` declarations are not reads). `Перем`/loop/bare-assignment locals are checked at every nesting level.

## Parameters

- `analyzeForLoopVariables` (boolean, default `true`) — report `For` counters (`For Counter = ... To ...`) that are never read inside the loop. Set to `false` to skip them: such a counter cannot simply be deleted, so some projects treat the report as noise. `For Each` variables and all other locals are unaffected.

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
