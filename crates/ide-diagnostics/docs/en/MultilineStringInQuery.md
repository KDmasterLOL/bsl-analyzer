# Multi-line literal in query (MultilineStringInQuery)

## Description

This diagnostic reports suspicious multi-line string literals inside query text.

In practice such literals usually appear by accident when double quotes are
escaped incorrectly. A common case is using `""` where SDBL expects `""""` for
an empty string literal. As a result, part of the query text is interpreted as
one long string constant instead of normal query syntax.

## Examples

Incorrect:

```bsl
Query = New Query;
Query.Text = "SELECT
|   OrderGoods.Cargo AS Cargo,
|   ISNULL(OrderGoods.Cargo.Code, "") AS CargoCode,
|   ISNULL(OrderGoods.Cargo.Name, "") AS CargoName
|FROM
|   Document.Order.Goods AS OrderGoods";
```

Correct:

```bsl
Query = New Query;
Query.Text = "SELECT
|   OrderGoods.Cargo AS Cargo,
|   ISNULL(OrderGoods.Cargo.Code, """") AS CargoCode,
|   ISNULL(OrderGoods.Cargo.Name, """") AS CargoName
|FROM
|   Document.Order.Goods AS OrderGoods";
```

## Sources

- Secondary reference: [v8std.ru: MultilineStringInQuery](https://v8std.ru/diagnostics/bslls/MultilineStringInQuery/)
