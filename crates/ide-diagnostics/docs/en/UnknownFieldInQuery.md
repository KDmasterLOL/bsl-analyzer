# Unknown field in a query table (UnknownFieldInQuery)

## Description

This diagnostic reports a query that references a field which does not exist on its metadata table.

To stay false-positive safe, it fires only for tables whose field model is provably complete (resolved reference objects and the main register table). It stays silent on incomplete models, including register virtual tables, temporary tables, subqueries, and objects that resolve through an extension.

The check validates only the first hop: in `T.Field.SubField` only `Field` is checked against the table.

## Examples

Reference to a missing field on a register:
```sdbl
SELECT
    T.NoSuchField AS Value
FROM
    AccumulationRegister.Goods AS T
```

## Sources

* [ITS: query language fields](https://its.1c.ru/db/pubqlang)
