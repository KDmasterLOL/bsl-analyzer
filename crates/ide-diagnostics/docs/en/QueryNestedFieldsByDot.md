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

## Configuration

| Parameter      | Type   | Default | Description |
|----------------|--------|---------|-------------|
| `minPathDepth` | `int`  | `3`     | Minimum number of dot-separated parts in an ordinary `SELECT` / `WHERE` / `JOIN` column reference required to emit the diagnostic. The default `3` flags two-or-more-level dereferences such as `T.Ссылка.Поле`. Lower values widen coverage (`2` flags any single dereference like `T.Поле`); higher values narrow it. Values below `2` are clamped by the hard floor — a one-part identifier carries no dereference to flag. |

`minPathDepth` applies only to the ordinary column-ref path. Virtual-table parameter
dereferences and `CAST` / `ВЫРАЗИТЬ` member chains carry their own intrinsic threshold
in the syntax of the construct and always emit regardless of this setting.

To turn the diagnostic off entirely use the standard activation flag rather than
`minPathDepth`.

## Sources

- Related public guidance: [Dereference of composite-type reference fields in the query language (#std654)](https://its.1c.ru/db/v8std/content/654/hdoc)
- Secondary reference: [v8std.ru: QueryNestedFieldsByDot](https://v8std.ru/diagnostics/bslls/QueryNestedFieldsByDot/)
