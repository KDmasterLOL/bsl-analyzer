# Reserved word used as procedure/function name (ReservedWordAsMethodName)

## Description

Procedure and function names cannot match BSL reserved words. The 1C platform will reject such code with a compilation error: "Procedure name expected".

Reserved words include: `Procedure`, `Function`, `If`, `Then`, `Else`, `For`, `Each`, `In`, `While`, `Do`, `Return`, `Try`, `Except`, `Raise`, `Var`, `New`, `Execute`, `Export`, `Val`, `True`, `False`, `Undefined`, `Null`, `Not`, `And`, `Or`, `Async`, `Await`, `Goto`, `Continue`, `Break`, `AddHandler`, `RemoveHandler` and other language keywords.

## Examples

Incorrect:

```bsl
&AtClient
Procedure Execute(Command)
    // ...
EndProcedure
```

Correct:

```bsl
&AtClient
Procedure ExecuteCommand(Command)
    // ...
EndProcedure
```
