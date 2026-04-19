# Identical expressions on both sides of an operator (IdenticalExpressions)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic detects two suspicious patterns:

1. **The same expression appears on both sides of an operator**. For operators
   such as `<`, `>`, `<=`, `>=`, `=`, `<>`, `AND`, `OR`, `-`, or `/`, this
   usually points to a copy-paste or attention error.

2. **The same expression is repeated in a preprocessor-split logical chain**.
   If an expression is broken by `#If/#Else/#EndIf` or similar directives and
   ends up repeating the same condition or operand, the conditional split is
   likely meaningless or incorrect.

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

## Sources

No direct 1C standard is used as the normative basis for this diagnostic.
It is a generic suspicious-pattern rule implemented locally in `bsl-analyzer`.
