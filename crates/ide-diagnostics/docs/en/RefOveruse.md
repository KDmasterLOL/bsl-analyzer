# Overuse "Reference" in a query (RefOveruse)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

When a query field already has a reference type, an additional `.Ref` / `.Ссылка` access is usually redundant. Such dereferencing may force the platform to build extra implicit joins and can make the query slower.

This diagnostic reports cases where the query accesses `.Ссылка` on a field that is already known as a reference.

## Examples

Incorrect:

```bsl
Query.Text = "SELECT
|   Files.File.Ref AS FileRef
|FROM
|   InformationRegister.InternalFiles AS Files";
```

Correct:

```bsl
Query.Text = "SELECT
|   Files.File AS FileRef
|FROM
|   InformationRegister.InternalFiles AS Files";
```

## Sources

- [#std654: Dereferencing reference fields of composite type in query language (RU)](https://its.1c.ru/db/v8std#content:654:hdoc)
- [v8std.ru: #std654](https://v8std.ru/std/654/)
