# Missing DataExchange.Load Check in an Object Event Handler (DataExchangeLoading)

## Description

Object event handlers such as `BeforeWrite`, `OnWrite`, and `BeforeDelete`
should start with a check for `DataExchange.Load` / `ОбменДанными.Загрузка`.

When an object is written during data exchange, the business logic that normally
belongs to the handler should not run again. The object is expected to be
loaded into the infobase as-is, without extra checks, recalculations, or
transformations that can distort the incoming data or break synchronization.

## Examples

Incorrect:

```bsl
Procedure BeforeWrite(Cancel)
    FillDefaultValues();
    ValidateBusinessRules();
EndProcedure
```

Correct:

```bsl
Procedure BeforeWrite(Cancel)
    If DataExchange.Load Then
        Return;
    EndIf;

    FillDefaultValues();
    ValidateBusinessRules();
EndProcedure
```

## Sources

- [ITS: Using DataExchange.Load in object event handlers (RU)](https://its.1c.ru/db/v8std#content:773)
- [ITS: BeforeWrite handler (RU)](https://its.1c.ru/db/v8std#content:464)
- [ITS: OnWrite handler (RU)](https://its.1c.ru/db/v8std#content:465)
- [ITS: BeforeDelete handler (RU)](https://its.1c.ru/db/v8std#content:752)
- [v8std: #std773 Using DataExchange.Load in object event handlers](https://v8std.ru/std/773/)
- [v8std: data-exchange-load](https://v8std.ru/diagnostics/v8-code-style/data-exchange-load/)
