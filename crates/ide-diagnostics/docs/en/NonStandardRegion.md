# Non-standard region of module (NonStandardRegion)

## Description

This diagnostic reports module regions whose names do not belong to the
standard region set for the current module type.

The standard module structure in 1C defines which region names are allowed for
common modules, form modules, object modules, and other module kinds. Custom
region names make module structure less predictable and break the shared layout
described by the standard.

## Examples

Incorrect:

```bsl
#Region MyHelpers

Procedure Process()
EndProcedure

#EndRegion
```

Correct:

```bsl
#Region Private

Procedure Process()
EndProcedure

#EndRegion
```

## Sources

- Source: [1C standard: Module structure (#std455)](https://its.1c.ru/db/v8std#content:455:hdoc)
- Secondary reference: [v8std.ru: NonStandardRegion](https://v8std.ru/diagnostics/bslls/NonStandardRegion/)
