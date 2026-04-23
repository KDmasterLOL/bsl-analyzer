# Magic numbers (MagicNumber)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

A magic number is a hard-coded numeric literal whose meaning is not obvious from
the code around it.

Such values make code harder to read and maintain. Prefer replacing them with a
named variable or constant that explains the intent.

The current implementation is configurable. By default it ignores some widely
used values and several structural contexts such as:

- default parameter values;
- structure and correspondence inserts;
- structure constructors and property assignments;
- some constructor calls such as number/string qualifiers;
- array indexes when `allowMagicIndexes` is enabled.

## Examples

Invalid:

```bsl
Function WeightInKilograms(WeightInGrams)
    Return WeightInGrams / 1000;
EndFunction
```

Better:

```bsl
Function WeightInKilograms(WeightInGrams)
    GramsPerKilogram = 1000;
    Return WeightInGrams / GramsPerKilogram;
EndFunction
```

## Exceptions

Some numeric literals are intentionally not reported when their context is
treated as structurally meaningful by the current implementation:

```bsl
Structure = New Structure;
Structure.Insert("Width", 800);
Structure.Insert("Height", 600);

Structure2 = New Structure("Field1, Field2", 5, 15);

StructureWithFields = New Structure("MyVariable, AnotherField");
StructureWithFields.MyVariable = 20;
StructureWithFields.AnotherField = 50;

Correspondence = New Correspondence;
Correspondence.Insert("Code", 123);
Correspondence.Insert(1980, "Olympics in Moscow");
```

## Sources

This diagnostic has no direct normative 1C standard source.

Related public context:

* [v8std.ru / bslls / MagicNumber](https://v8std.ru/diagnostics/bslls/MagicNumber/)
