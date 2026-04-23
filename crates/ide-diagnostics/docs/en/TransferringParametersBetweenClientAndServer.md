# Transferring parameters between the client and the server (TransferringParametersBetweenClientAndServer)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

When control is transferred from the client to the server, parameters are copied. If a server method parameter is declared without `Val` / `Знач`, the value may also be copied back to the client when control returns.

If such a parameter is never reassigned inside the server method, this reverse transfer is usually unnecessary and creates extra traffic.

The current implementation reports server methods when all of these conditions hold:

- the method has `&AtServer` or `&AtServerNoContext`;
- it is directly called from an `&AtClient` method in the same module;
- the parameter is not marked with `Val`;
- the parameter is not assigned inside the server method body.

## Examples 
<!-- This section contains examples for which the diagnostics work, and you can also give an example of how to fix the situation -->

```bsl
&AtClient
Procedure ShowBalance(Command)
    Result = GetBalance(CurrentItem, CurrentWarehouse);
EndProcedure

&AtServerNoContext
Function GetBalance(Item, Warehouse)
    Return AccumulationRegisters.Stock.GetBalance(Item, Warehouse);
EndFunction
```

Recommended:

```bsl
&AtClient
Procedure ShowBalance(Command)
    Result = GetBalance(CurrentItem, CurrentWarehouse);
EndProcedure

&AtServerNoContext
Function GetBalance(Val Item, Val Warehouse)
    Return AccumulationRegisters.Stock.GetBalance(Item, Warehouse);
EndFunction
```

## Sources 

- [1C ITS article: call with transfer of control from client to server (RU)](https://its.1c.ru/db/v8318doc#bookmark:dev:TI000000153)
- [#std487: Minimizing server calls and traffic (RU)](https://its.1c.ru/db/v8std#content:487:hdoc)
