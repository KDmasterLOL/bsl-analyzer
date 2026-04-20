# If...Then...ElseIf... chains should end with Else (IfElseIfEndsWithElse)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports `If` chains that contain one or more `ElseIf` branches
but do not end with `Else`.

The rule is based on a defensive-programming style: when a chain already has
multiple explicit alternatives, an `Else` branch makes the handling of the
remaining cases explicit. In practice it can either process unexpected values or
document why no action is required.

## Examples

Incorrect:

```bsl
If TypeOf(InputParameter) = Type("Structure") Then
    Result = FillByStructure(InputParameter);
ElsIf TypeOf(InputParameter) = Type("Document.Ref.MajorDocument") Then
    Result = FillByDocument(InputParameter);
EndIf;
```

Correct:

```bsl
If TypeOf(InputParameter) = Type("Structure") Then
    Result = FillByStructure(InputParameter);
ElsIf TypeOf(InputParameter) = Type("Document.Ref.MajorDocument") Then
    Result = FillByDocument(InputParameter);
Else
    Raise "Parameter of invalid type passed";
EndIf;
```

## Sources

No direct normative 1C standard is used as the basis for this diagnostic.
It is a local defensive-programming rule implemented in `bsl-analyzer`.
