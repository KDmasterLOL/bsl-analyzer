# Assigning aliases to selected fields in a query (AssignAliasFieldsInQuery)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic checks fields inside subqueries and requires aliases to be written explicitly with the `AS` keyword.

Explicit aliases make query results more stable and easier to read. If a field is selected without an alias, the platform derives the result column name automatically. That derived name may change after a metadata rename, or it may simply be less obvious than the name expected by the code that reads the query result.

This is especially important for composite expressions such as `Items.Supplier.Name`, where the generated name may not match the developer's intention.

The diagnostic also reports implicit aliases without `AS`, for example `Items.Price SalePrice`. Asterisk fields (`*`, `Table.*`) are ignored. Fields from secondary `UNION` parts are not checked.

## Examples

```bsl   
Query = New Query;
Query.Text =
"SELECT
|   Items.Article, // Incorrect
|   Items.Article AS ItemArticle, // Correct
|   Items.Price SalePrice // Incorrect: alias without AS
|FROM
|   Catalog.Products AS Items // Source alias is ignored
|
|UNION ALL
|
|SELECT
|   Services.Article, // Ignored
|   Services.Article, // Ignored
|   Services.Price // Ignored
|FROM
|   Catalog.Services AS Services";

Query1 = New Query;
Query1.Text =
"SELECT
|   Data.ItemName AS ItemName
|FROM
|   (SELECT
|       Items.Description ItemName // Incorrect: missing AS
|   FROM
|       Catalog.Products AS Items) AS Data";
```

## Sources
Source: [Making query text](https://its.1c.ru/db/v8std#content:437:hdoc)
