# Source code parse error (ParseError)

## Description

This diagnostic reports syntax fragments that the current BSL parser could not
parse correctly.

In implementation terms the rule is simple: every non-empty `ERROR` node in the
syntax tree becomes a `ParseError` diagnostic. This means the diagnostic covers
different kinds of parse failures, including broken language syntax, malformed
preprocessor usage, unterminated strings, and other parser-level errors.

So while preprocessor misuse is one common source of such errors, the current
diagnostic is broader than a single standard scenario.

## Examples

Incorrect:

```bsl
Procedure Example()
    If Not Then
    EndIf;
EndProcedure
```

```bsl
Procedure Example()
    Value = "unterminated string
EndProcedure
```

## Sources

- Related public guidance: [1C standard: Use of compilation directives and preprocessor instructions (#std439)](https://its.1c.ru/db/v8std#content:439:hdoc)
