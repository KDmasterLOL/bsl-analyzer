# Limit number of optional parameters in method (NumberOfOptionalParams)

## Description

This diagnostic reports procedures and functions that declare too many optional
parameters.

The general rationale comes from the 1C recommendations for procedure and
function parameters: a large number of optional arguments makes calls harder to
read and easier to misuse. In practice it becomes difficult to understand which
values are intentionally passed and which ones are just skipped through default
positions.

By default the diagnostic allows up to `3` optional parameters, but the limit
can be changed with `maxOptionalParamsCount`.

## Examples

Incorrect:

```bsl
Procedure CreateItem(Name, Goods, Units, Weight, Check = True, Archive = False, Notify = True, Validate = True)
EndProcedure
```

Correct:

```bsl
Procedure CreateItem(Name, Goods, Params = Undefined)
EndProcedure
```

## Sources

- Source: [1C standard: Parameters of procedures and functions (#std640)](https://its.1c.ru/db/v8std#content:640:hdoc)
- Secondary reference: [v8std.ru: NumberOfOptionalParams](https://v8std.ru/diagnostics/bslls/NumberOfOptionalParams/)
