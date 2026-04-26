# Using 'SELECT TOP' without 'ORDER BY' (SelectTopWithoutOrderBy)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Using `TOP N` / `ПЕРВЫЕ N` without explicit ordering can lead to nondeterministic results. The returned rows may change across DBMSs, platform versions, or different copies of the same database.

The current implementation reports these cases:

- `TOP N` inside `UNION`, because ordering is applied after the union result;
- `TOP N` with `N > 1` and no `ORDER BY`;
- `TOP 1` or `TOP 0` with no `ORDER BY` and no `WHERE`, unless this case is disabled by configuration.

## Examples

Incorrect:

```bsl
Query.Text = "SELECT TOP 10
|   Contractors.Ref
|FROM
|   Directory.Contractors AS Contractors";
```

Correct:

```bsl
Query.Text = "SELECT TOP 10
|   Contractors.Ref
|FROM
|   Directory.Contractors AS Contractors
|ORDER BY
|   Contractors.Code";
```

## Sources

- [#std412: Ordering query results (RU)](https://its.1c.ru/db/v8std#content:412:hdoc)
- [v8std.ru: #std412](https://v8std.ru/std/412/)
