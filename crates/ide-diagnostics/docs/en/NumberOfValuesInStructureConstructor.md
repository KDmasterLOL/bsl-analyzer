# Limit on the number of property values passed to the structure constructor (NumberOfValuesInStructureConstructor)

## Description

This diagnostic reports `Structure` and `FixedStructure` constructors that
receive too many property values directly in the constructor call.

The general rationale comes from the 1C guidance on using `Structure`: long
constructor calls are hard to read because the reader must mentally match the
property list with the corresponding value positions. In many cases it is
clearer to create an empty structure and then fill it with `Insert`.

By default the diagnostic allows up to `3` values in the constructor, but the
limit can be changed with `maxValuesCount`.

## Examples

Incorrect:

```bsl
Settings = New Structure(
    "Organization, Period, Warehouse, Currency, Responsible",
    SelectedOrganization,
    CurrentDate(),
    MainWarehouse,
    AccountingCurrency,
    CurrentUser);
```

Correct:

```bsl
Settings = New Structure;
Settings.Insert("Organization", SelectedOrganization);
Settings.Insert("Period", CurrentDate());
Settings.Insert("Warehouse", MainWarehouse);
Settings.Insert("Currency", AccountingCurrency);
Settings.Insert("Responsible", CurrentUser);
```

## Sources

- Source: [1C standard: Using objects of type Structure (#std693)](https://its.1c.ru/db/v8std#content:693:hdoc)
- Secondary reference: [v8std.ru: NumberOfValuesInStructureConstructor](https://v8std.ru/diagnostics/bslls/NumberOfValuesInStructureConstructor/)
