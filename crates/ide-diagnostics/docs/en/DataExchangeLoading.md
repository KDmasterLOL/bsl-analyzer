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

## Parameters

- `guardWrappers` — a list of `Module.Function` entries recognised as an
  equivalent of the `DataExchange.Load` check (the function body is a
  condition derived from `DataExchange.Load`). Default:
  `["ЗарплатаКадры.ОтключитьБизнесЛогикуПриЗаписи"]`. A configured list
  replaces the default entirely. A wrapper call counts only inside a guard
  condition whose then-branch returns — same as the literal check.
- `findFirst` — when `true`, only the first executable statement of the
  handler is searched for the guard (default `false` — the whole body).

## Sources

- [ITS: Using DataExchange.Load in object event handlers (RU)](https://its.1c.ru/db/v8std#content:773)
- [ITS: BeforeWrite handler (RU)](https://its.1c.ru/db/v8std#content:464)
- [ITS: OnWrite handler (RU)](https://its.1c.ru/db/v8std#content:465)
- [ITS: BeforeDelete handler (RU)](https://its.1c.ru/db/v8std#content:752)
- [v8std: #std773 Using DataExchange.Load in object event handlers](https://v8std.ru/std/773/)
- [v8std: data-exchange-load](https://v8std.ru/diagnostics/v8-code-style/data-exchange-load/)
