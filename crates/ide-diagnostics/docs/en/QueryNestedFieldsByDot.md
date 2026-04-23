# Getting objects nested fields data by dot in database query text (QueryNestedFieldsByDot)

## Description

This diagnostic reports dereference of reference fields through dot inside query
text.

The public rationale is performance-related: such expressions may lead to
implicit joins and less predictable query execution. In practice it is often
clearer and more efficient to fetch related data through explicit joins.

The current implementation covers several forms:

- ordinary nested field access in `SELECT`, `WHERE`, and `JOIN`;
- nested field access inside virtual table parameters;
- dereference after `CAST` / `ВЫРАЗИТЬ`.

## Examples

Incorrect:

```bsl
ВЫБРАТЬ
    Продажи.Контрагент.Наименование КАК КонтрагентНаименование
ИЗ
    Документ.Продажи КАК Продажи
```

Correct:

```bsl
ВЫБРАТЬ
    Контрагенты.Наименование КАК КонтрагентНаименование
ИЗ
    Документ.Продажи КАК Продажи
    ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Контрагенты КАК Контрагенты
    ПО Продажи.Контрагент = Контрагенты.Ссылка
```

## Sources

- Related public guidance: [Dereference of composite-type reference fields in the query language (#std654)](https://its.1c.ru/db/v8std/content/654/hdoc)
- Secondary reference: [v8std.ru: QueryNestedFieldsByDot](https://v8std.ru/diagnostics/bslls/QueryNestedFieldsByDot/)
