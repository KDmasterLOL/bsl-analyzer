# There are identical sub-expressions to the left and to the right of the "foo" operator (IdenticalExpressions)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

The diagnostic detects two types of issues:

1. **Identical expressions on both sides of an operator** — if there is an operator (<, >, <=, >=, =, <>, AND, OR, -, /) in the program text with identical subexpressions on both sides, this is most likely a logic error.

2. **Repeated expressions in code split by preprocessor directives** — if an expression is split by preprocessor instructions (#If/#Else/#EndIf) and contains identical code in different branches, this may indicate a copy-paste error.

## Examples

### Identical expressions on both sides of an operator

```bsl
If Summ <> 0 AND Summ <> 0 Then

    // TODO

EndIf;
```

In this case, the `AND` operator is surrounded by identical subexpressions `Summ <> 0` and it allows us to detect an error made through inattention. The correct code that will not look suspicious to the analyzer looks in the following way:

```bsl
If Summ <> 0 AND SummNDS <> 0 Then

    // TODO

EndIf;
```

OR

```bsl
If Summ <> 0 Then

    // TODO

EndIf;
```

### Repeated expressions in code with preprocessor

```bsl
SchemaAvailableForEditing =
#If Client Then
    SchemaAvailableForEditing
#Else
    SchemaAvailableForEditing
#EndIf
    AND AccessRight("Edit", Metadata.Catalogs.Products);
```

Here both preprocessor branches use the same expression `SchemaAvailableForEditing`, which makes conditional compilation meaningless. The correct version:

```bsl
SchemaAvailableForEditing =
#If Client Then
    SchemaAvailableForEditingOnClient
#Else
    SchemaAvailableForEditingOnServer
#EndIf
    AND AccessRight("Edit", Metadata.Catalogs.Products);
```
