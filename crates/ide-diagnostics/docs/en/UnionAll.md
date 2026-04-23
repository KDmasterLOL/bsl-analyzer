# Using keyword "UNION" in queries (UnionAll)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
In most cases, when you need to combine the results of two or more queries into a single result set, use `UNION ALL` instead of `UNION`. A plain `UNION` removes duplicate rows from the combined result and therefore requires extra processing even when duplicates are impossible by design.

Use `UNION` only when removing duplicates is a required part of the query logic.

The current implementation is intentionally conservative: it reports any `UNION` / `ОБЪЕДИНИТЬ` occurrence that does not use `ALL` / `ВСЕ`.

## Examples

Incorrect:
```bsl
SELECT
GoodsReceipt.Ref
FROM
Document.GoodsReceipt AS GoodsReceipt

UNION

SELECT
GoodsSale.Ref
FROM
Document.GoodsSale AS GoodsSale
```

Correct:

```bsl
SELECT
GoodsReceipt.Ref
FROM
Document.GoodsReceipt AS GoodsReceipt

UNION ALL

SELECT
GoodsSale.Ref
FROM
Document.GoodsSale AS GoodsSale
```

## Sources
* Link: [Development Standart: Using UNION and UNION ALL words in queries (RU)](https://its.1c.ru/db/v8std#content:434:hdoc)
