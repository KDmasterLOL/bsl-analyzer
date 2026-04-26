# Number of parameters in method (NumberOfParams)

## Description

This diagnostic reports procedures and functions that declare too many
parameters.

The general rationale comes from the 1C recommendations for procedure and
function parameters: very large parameter lists reduce readability and make call
sites harder to understand. When a method needs many related values, it is
usually clearer to group them into a structure or another composite parameter.

By default the diagnostic allows up to `7` parameters, but the limit can be
changed with `maxParamsCount`.

## Examples

Incorrect:

```bsl
Procedure CreateDocument(Date, Number, Counterparty, Contract, Warehouse, Responsible, Organization, Comment)
EndProcedure
```

Correct:

```bsl
Procedure CreateDocument(DocumentData)
EndProcedure
```

## Sources

- Source: [1C standard: Parameters of procedures and functions (#std640)](https://its.1c.ru/db/v8std#content:640:hdoc)
- Secondary reference: [v8std.ru: NumberOfParams](https://v8std.ru/diagnostics/bslls/NumberOfParams/)
