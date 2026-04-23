# One statement per line (OneStatementPerLine)

## Description

This diagnostic reports multiple statements placed on the same line.

The general rationale comes from the 1C module-text formatting guidance:
separating statements by lines improves readability and makes debugging easier.
The current implementation is strict: if several statements start on the same
line, they are reported unless they are excluded by the lowering pipeline
(preprocessor cases, empty statements, or parse-error cases).

## Examples

Incorrect:

```bsl
Total = 0; If Quantity > 0 Then Total = Price * Quantity; EndIf;
```

Correct:

```bsl
Total = 0;
If Quantity > 0 Then
    Total = Price * Quantity;
EndIf;
```

## Sources

- Source: [1C standard: Module texts (#std456)](https://its.1c.ru/db/v8std#content:456:hdoc)
- Secondary reference: [v8std.ru: OneStatementPerLine](https://v8std.ru/diagnostics/bslls/OneStatementPerLine/)
