# Incorrect expression line break (IncorrectLineBreak)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports several line-break patterns that conflict with the
expression-wrapping style used in 1C standards and in the current formatter
policy of `bsl-analyzer`.

The current implementation checks:

- arithmetic operators and logical operators `AND` / `OR` at the end of a line;
- closing parenthesis `)` or semicolon `;` at the start of a line;
- a comma at the start of a line when it is followed by meaningful content.

For multiline string continuation, the rule deliberately does **not** report
`+` at line end when the next line starts with a string literal or `|`.

## Examples

Incorrect:

```bsl
AmountDocument = AmountWithoutDiscount +
                 AmountManualDiscounts +
                 AmountAutomaticDiscount;
```

Correct:

```bsl
AmountDocument = AmountWithoutDiscount 
    + AmountManualDiscounts 
    + AmountAutomaticDiscount;
```

Logical operators should also move to the start of the next line:

```bsl
If Condition1
    Or Condition2 Then
EndIf;
```

```bsl
If Condition1 Or
    Condition2 Then
EndIf;
```

```bsl
Names.Add(
    Name,
    Synonym);
```

```bsl
Names.Add(
    Name,
    Synonym
);
```

## Sources

* Primary source: [ITS / v8std #std444: Wrap expressions (RU)](https://its.1c.ru/db/v8std#content:444:hdoc)
* Secondary source: [v8std.ru: #std444](https://v8std.ru/std/444/)
