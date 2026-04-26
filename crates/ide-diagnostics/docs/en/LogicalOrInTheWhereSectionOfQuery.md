# Using a logical "OR" in the "WHERE" section of a query (LogicalOrInTheWhereSectionOfQuery)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports `OR` operators inside query `WHERE` clauses.

`OR` in filtering conditions often makes index usage worse and can push the DBMS
toward scan-heavy execution plans. A common rewrite is to split the logic into
separate query branches and combine them with `UNION ALL`, but only when that
preserves the original semantics.

Important: the current implementation is intentionally conservative and may
still report some cases that are acceptable from an optimization perspective.
It does not fully model the distinction between main indexed conditions and
additional conditions from the public guidance.

## Примеры
Diagnostic:

```bsl
ВЫБРАТЬ Номенклатура.Наименование
ИЗ Справочник.Номенклатура КАК Номенклатура
ГДЕ Номенклатура.Артикул = "А01" ИЛИ Номенклатура.Цена = 500
```

One possible rewrite:

```bsl
ВЫБРАТЬ Номенклатура.Наименование
ИЗ Справочник.Номенклатура КАК Номенклатура
ГДЕ Номенклатура.Артикул = "А01"

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ Номенклатура.Наименование
ИЗ Справочник.Номенклатура КАК Номенклатура
ГДЕ Номенклатура.Цена = 500
```

No diagnostic in the ideal model: same field, can be normalized to `IN`

```bsl
ГДЕ
    Таблица.Статус = &Статус1
    ИЛИ Таблица.Статус = &Статус2
```

## Sources
- [Standard: Effective Query Conditions, Clause 2 (RU)](https://its.1c.ru/db/v8std/content/658/hdoc)
- [Typical Causes of Suboptimal Query Performance and Optimization Techniques: Using Logical OR in Conditions (RU)](https://its.1c.ru/db/content/metod8dev/src/developers/scalability/standards/i8105842.htm#or)
- [Public mirror: v8std.ru / #std658](https://v8std.ru/std/658/)
