# All variables declarations must have a description (MissingVariablesDescription)

## Description

Module-level variables should have a comment that explains their purpose. Without such a description, global state in the module becomes harder to understand and maintain.

The current implementation is narrower than a generic "all variables" rule:

- it checks only top-level module variable declarations (`Var` / `Перем`);
- local variables inside procedures and functions are ignored;
- a description may be written on the same line or in a header comment above the declaration;
- annotated variables are also supported, including comments placed around annotations.

## Examples

### Incorrect

```bsl
Var Context;
```

### Correct

```bsl
Var Context; // Stores the current processing context

// Stores the current processing context
Var Context;
```

## Sources

- [Module structure - Standard 1C (RU)](https://its.1c.ru/db/v8std#content:455:hdoc)
- [v8std.ru: MissingVariablesDescription](https://v8std.ru/diagnostics/bslls/MissingVariablesDescription/)
