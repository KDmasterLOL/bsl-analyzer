# Nested constructors with parameters in structure declaration (NestedConstructorsInStructureDeclaration)

## Description

This diagnostic reports structure declarations that pass other constructors with
parameters directly as property values.

Such code is valid, but it quickly becomes hard to read when one `Structure` or
`FixedStructure` constructor contains several nested constructors. It is usually
clearer to create nested values separately and then pass ready variables into
the outer structure.

## Examples

Incorrect:

```bsl
Parameters = New Structure(
    "CheckMode, UpdateMode",
    New Structure("Document", "Check"),
    New Structure("Document", "Update")
);
```

Correct:

```bsl
CheckSettings = New Structure("Document", "Check");
UpdateSettings = New Structure("Document", "Update");

Parameters = New Structure(
    "CheckMode, UpdateMode",
    CheckSettings,
    UpdateSettings
);
```

## Sources

- Secondary reference: [v8std.ru: NestedConstructorsInStructureDeclaration](https://v8std.ru/diagnostics/bslls/NestedConstructorsInStructureDeclaration/)
