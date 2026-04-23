# Using FindByName, FindByCode and FindByNumber (UsingFindElementByString)

## Description

This diagnostic reports calls to `FindByDescription`, `FindByCode`, and `FindByNumber` when a hardcoded string or numeric literal is passed directly.

Such code is tied to data from a specific database and may stop working after deployment to another environment. It is also a common sign of test or temporary code left in production logic.

The current implementation primarily detects direct literal arguments and empty calls such as `FindByDescription()`.

## Examples

Incorrect:
```bsl
Position = Catalogs.Positions.FindByName("Senior Accountant");
```
or
```bsl
Position = Catalogs.Positions.FindByCode("00-0000001");
```

or

```bsl
Object = Documents.Invoice.FindByNumber("0000-000001", CurrentDate());
```

Acceptable use:
```bsl
Catalogs.Currencies.FindByCode(CurrentData.CurrencyCodeDigital);
```
```bsl
Catalogs.BankClassifier.FindByCode(BankDetails.BIK);
```

```bsl
Documents.Invoice.FindByNumber(Number);
```

## Sources

* [v8std: UsingFindElementByString](https://v8std.ru/diagnostics/bslls/UsingFindElementByString/)
