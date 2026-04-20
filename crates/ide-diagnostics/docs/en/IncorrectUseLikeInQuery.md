# Incorrect use of 'LIKE' (IncorrectUseLikeInQuery)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

According to `#std726`, the query operator `LIKE` / `ПОДОБНО` should use only:

- a constant string literal as a pattern;
- a query parameter as a pattern.

Do not build the pattern by calculations or concatenation inside the query
text. Using a field reference or another calculated expression on the pattern
side can lead to different behavior on different DBMSs.

## Examples
Allowed:

```sdbl
Field LIKE "123%"
```

```sdbl
Field LIKE &Pattern
```

Not allowed:

```sdbl
Field LIKE "123" + "%"
Field LIKE &Pattern + "%"
Field LIKE Table.Template
```

Instead of building the wildcard inside the query:

```bsl
Query = New Query("
|SELECT
|    Goods.Ref
|FROM
|    Catalog.Goods AS Goods
|WHERE
|    Goods.Country.Description LIKE &NameTemplate + "_"
|");

Query.SetParameter("NameTemplate", "FU");
```

prepare the value before passing the parameter:

```bsl
Query = New Query("
|SELECT
|    Goods.Ref
|FROM
|    Catalog.Goods AS Goods
|WHERE
|    Goods.Country.Description LIKE &NameTemplate
|");

Query.SetParameter("NameTemplate", "FU_");
```

## Sources

* Primary source: [ITS / v8std #std726: Features of using LIKE in queries (RU)](https://its.1c.ru/db/v8std#content:726:hdoc)
* Secondary source: [v8std.ru: #std726](https://v8std.ru/std/726/)
