# Method parameters description are missing (MissingParameterDescription)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
The description of an export method (procedure or function) with parameters should document those parameters correctly.

For non-export methods, this diagnostic does not require adding a `Parameters` section when the comment only explains the method purpose. If a `Parameters` section is already present, its content is checked against the signature regardless of whether the method is exported.

Diagnostic detects typical errors:

- Missing `Parameters` section for an export method with parameters
- Absence of a description of some of the parameters, indicating for which parameter the description was not found
- The presence in the description of parameters that are absent in the method signature (which could remain from refactoring)
- Duplicate parameter descriptions
- Incorrect order of parameter descriptions

The current implementation also skips hyperlink-style documentation comments such as `See OtherMethod()`.

### Strict mode

Set the diagnostic parameter `allowShortDescriptionParameters` to
`false` to additionally require prose description after the type for
each parameter. With strict mode on, `Параметр1 - Строка` (type only)
is flagged as missing the explanation; `Параметр1 - Строка - первое слагаемое`
or a structured `Структура:` block with sub-fields is accepted. Default
is `true` to preserve compatibility with the BSL idiom of type-only
parameters. Mirrors `MissingReturnedValueDescription`'s
`allowShortDescriptionReturnValues` knob.

> **Behavioural change:** the `Параметры:` / `Parameters:` keywords are now recognised only at the start of a comment line. The previous parser also matched the keyword anywhere in free-form text, which occasionally produced false positives. If existing comments had `parameters:` occurring mid-sentence and implicitly opening a section, after the upgrade those cases must be rewritten as explicit section headers.

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
