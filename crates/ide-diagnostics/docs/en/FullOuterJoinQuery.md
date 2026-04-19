# Using of "FULL OUTER JOIN" in queries (FullOuterJoinQuery)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->
Avoid `FULL OUTER JOIN` in queries when the same result can be expressed in a simpler form.

According to the 1C standard, this construct can significantly degrade performance in client-server deployments with PostgreSQL, especially when it appears multiple times in one query.

In many cases the query can be rewritten with `UNION ALL` and `LEFT JOIN`. The standard also allows exceptions when the query cannot reasonably be rewritten without `FULL OUTER JOIN`.
## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
```bsl
Procedure Test1()

    Query = New Query;
    Query.Text = "SELECT
                   |    Goods.Product AS Product,
                   |    ISNULL(SalesPlan.Sum, 0) AS PlanSum,
                   |    ISNULL(SalesActual.Sum, 0) AS ActualSum
                   |FROM
                   |    Goods AS Goods
                   |        LEFT JOIN SalesPlan AS SalesPlan
                   |            FULL OUTER JOIN SalesActual AS SalesActual // Should trigger here
                   |            ON SalesPlan.Product = SalesActual.Product
                   |        ON Goods.Product = SalesPlan.Product";

EndProcedure
```
## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->

* [Restricting the use of FULL OUTER JOIN in queries (RU)](https://its.1c.ru/db/v8std/content/435/hdoc)
* [Administrator's Guide: PostgreSQL specifics (RU)](https://its.1c.ru/db/metod8dev/content/1556/hdoc)
* [v8std.ru: #std435](https://v8std.ru/std/435/)
