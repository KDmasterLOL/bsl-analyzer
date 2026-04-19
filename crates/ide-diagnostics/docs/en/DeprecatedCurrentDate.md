# Using the Deprecated Method `CurrentDate` (DeprecatedCurrentDate)

## Description

`CurrentDate()` / `ТекущаяДата()` returns the date and time of the machine where
the code is currently executed. In client-server systems and service
deployments, that can lead to incorrect results when the server timezone does
not match the user session timezone.

On the server, use `CurrentSessionDate()` / `ТекущаяДатаСеанса()` instead. In
client code, `CurrentDate()` should also be avoided so that client and server
time calculations stay consistent.

When the Standard Library is available, client code can use
`GeneralPurposeClient.SessionDate()` / `ОбщегоНазначенияКлиент.ДатаСеанса()`.

## Examples

Server-side:

Incorrect:

```bsl
OperationDate = CurrentDate();
```

Correct:

```bsl
OperationDate = CurrentSessionDate();
```

Client-side:

Incorrect:

```bsl
OperationDate = CurrentDate();
```

Correct:

```bsl
OperationDate = GeneralPurposeClient.SessionDate();
```

## Sources

- [ITS: Work in different time zones (RU)](https://its.1c.ru/db/v8std/content/643/hdoc)
- [v8std: #std643 Work in different time zones](https://v8std.ru/std/643/)
