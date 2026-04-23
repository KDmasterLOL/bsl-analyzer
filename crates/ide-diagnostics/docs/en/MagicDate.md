# Magic dates (MagicDate)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

A magic date is a hard-coded date literal whose meaning is not obvious from the
code around it.

Such values make code harder to read and maintain. Prefer moving the date into
a named variable or returning it from a helper function with a clear semantic
name.

## Examples

Invalid:

```bsl
If CurrentDate < '20240301' Then
    ApplyOldRate = True;
EndIf;
```

Better:

```bsl
VatRateChangeDate = '20240301';
If CurrentDate < VatRateChangeDate Then
    ApplyOldRate = True;
EndIf;
```

Another good option:

```bsl
Function VatRateChangeDate()
    Return '20240301';
EndFunction

If CurrentDate < VatRateChangeDate() Then
    ApplyOldRate = True;
EndIf;
```

## Exceptions

The current implementation intentionally skips several contexts where the date
literal is treated as acceptable or structurally meaningful, for example:

- authorized dates from configuration;
- simple `Date(...)` assignments;
- return statements and default parameter values;
- structure and correspondence inserts;
- structure constructors and property assignments.

Examples:

```bsl
Structure = New Structure;
Structure.Insert("StartDate", '20250101');
Structure.Insert("EndDate", '20251231');

Structure2 = New Structure("StartDate, EndDate", '20250101', '20251231');

StructureWithFields = New Structure("StartDate, EndDate");
StructureWithFields.StartDate = '20250101';
StructureWithFields.EndDate = '20251231';

Correspondence = New Correspondence;
Correspondence.Insert("Code", '20230101');
Correspondence.Insert('19800101', "Olympics in Moscow");
```

## Sources

This diagnostic has no direct normative 1C standard source.

Related public context:

* [v8std.ru / bslls / MagicDate](https://v8std.ru/diagnostics/bslls/MagicDate/)
