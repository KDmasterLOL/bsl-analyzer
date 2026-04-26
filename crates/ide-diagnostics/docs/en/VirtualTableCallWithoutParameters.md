# Virtual table call without parameters (VirtualTableCallWithoutParameters)

## Description
When using virtual tables in queries, relevant filters should be passed through the virtual table parameters.

The current implementation reports calls where the virtual table is used without parameters or with empty parameter slots, for example:

- no parentheses at all
- empty parentheses `()`
- both parameter positions empty `(, )`
- a required trailing parameter left empty such as `(&Period, )`

This is a conservative performance-oriented rule intended to catch calls that are likely to read too much data before filtering.

## Examples
Reported:
```bsl
Query.Text = "SELECT
| Good
|FROM
| AccumulationRegister.MyGoods.Turnovers()";
```

Preferred:

```bsl
Query.Text = "SELECT
| Good
|FROM
| AccumulationRegister.MyGoods.Turnovers(, Warehouse = &Warehouse)";
```

## Sources
* Standard: [Using virtual tables (RU)](https://its.1c.ru/db/v8std#content:657:hdoc)
* Standard: [Effective use of the virtual table «Turnovers» (RU)](https://its.1c.ru/db/v8std#content:733:hdoc)
* 1C Recommendation: [Using the Condition parameter when accessing a virtual table (RU)](https://its.1c.ru/db/metod8dev/content/5457/hdoc)
