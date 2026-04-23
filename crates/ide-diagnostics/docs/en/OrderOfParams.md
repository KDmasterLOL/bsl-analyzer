# Order of parameters in method (OrderOfParams)

## Description

This diagnostic reports procedures and functions where parameters with default
values are declared before required parameters.

The rationale comes from the 1C recommendations for procedure and function
parameters: optional parameters should follow mandatory ones. Violating this
order makes signatures harder to read and creates confusion at call sites.

## Examples

Incorrect:

```bsl
Function CalculateDiscount(Percent = 5, SaleAmount)
EndFunction
```

Correct:

```bsl
Function CalculateDiscount(SaleAmount, Percent = 5)
EndFunction
```

## Sources

- Source: [1C standard: Parameters of procedures and functions (#std640)](https://its.1c.ru/db/v8std#content:640:hdoc)
- Secondary reference: [v8std.ru: OrderOfParams](https://v8std.ru/diagnostics/bslls/OrderOfParams/)
