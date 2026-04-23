# Method parameters description are missing (MissingParameterDescription)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
The description of a method (procedure or function) should be formatted correctly to help programmers use the functionality correctly.

If a method contains parameters, then its documentation comment should describe them in the parameter section and keep the same order as in the signature.

Diagnostic detects typical errors:

- Lack of description of all parameters
- Absence of a description of some of the parameters, indicating for which parameter the description was not found
- The presence in the description of parameters that are absent in the method signature (which could remain from refactoring)
- Duplicate parameter descriptions
- Incorrect order of parameter descriptions

The current implementation also skips hyperlink-style documentation comments such as `See OtherMethod()`.

## Examples

### Incorrect

```bsl
// Writes data to a file.
Procedure WriteToFile(FilePath, Data, Encoding) Export
```

### Correct

```bsl
// Writes data to a file.
//
// Parameters:
//   FilePath - String - full path to the file
//   Data - String - content to write
//   Encoding - String - file encoding, for example "UTF-8"
Procedure WriteToFile(FilePath, Data, Encoding) Export
```

## Sources
- [Standard: Procedures and functions description (RU)](https://its.1c.ru/db/v8std#content:453:hdoc)
- [v8std.ru: MissingParameterDescription (RU)](https://v8std.ru/diagnostics/bslls/MissingParameterDescription/)
