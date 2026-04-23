# All public methods must have a description (PublicMethodsDescription)

## Description

This diagnostic reports exported methods that do not have a documentation
comment.

The public rationale comes from the 1C recommendations for describing
procedures and functions. In the current project, the default behavior is
narrower than “all exported methods”: by default the diagnostic checks only
methods in the `ПрограммныйИнтерфейс` / `Public` region. If
`checkAllRegion = true`, it checks exported methods regardless of region.

## Examples

Incorrect:

```bsl
#Область Public

Function GetRate(Currency, Date) Export
EndFunction

#EndRegion
```

Correct:

```bsl
#Область Public

// Returns the exchange rate for the specified date.
Function GetRate(Currency, Date) Export
EndFunction

#EndRegion
```

## Sources

- Source: [1C standard: Description of procedures and functions (#std453)](https://its.1c.ru/db/v8std#content:453:hdoc)
- Secondary reference: [v8std.ru: PublicMethodsDescription](https://v8std.ru/diagnostics/bslls/PublicMethodsDescription/)
