# Using non-existent metadata in the query (QueryToMissingMetadata)

## Description

This diagnostic reports query sources that do not resolve to existing metadata objects.

Such queries usually appear after metadata was renamed or removed, or after manual query edits were made without verifying the final text.

## Examples

Reference to a missing catalog:
```sdbl
SELECT
    Items.Description AS Description
FROM
    Catalog.MissingCatalog AS Items
```

Reference to a missing register in a join:
```sdbl
SELECT
    Balances.Quantity AS Quantity
FROM
    AccumulationRegister.Balances AS Balances
    LEFT JOIN InformationRegister.MissingRegister AS Filter
    ON Filter.Item = Balances.Item
```

## Sources

* [v8std: QueryToMissingMetadata](https://v8std.ru/diagnostics/bslls/QueryToMissingMetadata/)
