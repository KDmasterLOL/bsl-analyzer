# Using the Deprecated Method `Find` (DeprecatedFind)

## Description

The global method `Find()` / `Найти()` is deprecated. For string search, use
`StrFind()` / `СтрНайти()` instead.

This is especially important because the old global name is ambiguous: it can
be confused with collection methods that legitimately use `.Find()` on a
specific object.

## Examples

Incorrect:

```bsl
If Find(Employee.Name, "Boris") > 0 Then
EndIf;
```

Correct:

```bsl
If StrFind(Employee.Name, "Boris") > 0 Then
EndIf;
```

## Sources

- [1C developer guidance: migrating to 8.3, method/property renames (RU)](https://its.1c.ru/db/content/metod8dev/src/developers/platform/metod/i8105293.htm)
