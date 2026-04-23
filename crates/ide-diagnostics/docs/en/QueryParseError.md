# Query text parsing error (QueryParseError)

## Description

This diagnostic reports SDBL query texts that contain syntax errors and cannot be parsed correctly.

## Examples

Incorrect

```bsl
Query.Text = "SELECT Field
             |FROM Catalog.Items AS";
```

Correct

```bsl
Query.Text = "SELECT Field
             |FROM Catalog.Items AS Items";
```

## Sources

* [Standard: Working with queries (RU). Formatting query texts](https://its.1c.ru/db/v8std#content:437:hdoc)
* [v8std: QueryParseError](https://v8std.ru/diagnostics/bslls/QueryParseError/)
