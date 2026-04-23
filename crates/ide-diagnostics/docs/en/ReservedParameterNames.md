# Reserved parameter names (ReservedParameterNames)

## Description

If a procedure or function parameter has the same name as a reserved platform identifier, that parameter hides the original name in the local scope. In practice this often leads to confusing code and broken access to system enumeration names or other reserved identifiers.

The current implementation is fully configuration-driven:

- it checks only procedure and function parameters;
- it compares parameter names against the configured `reservedWords` list;
- matching is case-insensitive;
- matching is exact, not partial.

If `reservedWords` is empty, the diagnostic produces no findings.

### Example configuration

```json
{
  "diagnostics": {
    "ReservedParameterNames": {
      "reservedWords": ["FormGroupType", "FormFieldType"]
    }
  }
}
```

## Sources

- [Procedure and function parameters - Standard 1C (RU)](https://its.1c.ru/db/v8std/content/640/hdoc)
- [Rules for generating variable names - Standard 1C (RU)](https://its.1c.ru/db/v8std#content:454:hdoc)
- [v8std.ru: ReservedParameterNames](https://v8std.ru/diagnostics/bslls/ReservedParameterNames/)
