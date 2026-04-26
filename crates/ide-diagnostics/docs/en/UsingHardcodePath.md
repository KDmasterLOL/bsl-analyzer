# Using hardcode file paths in code (UsingHardcodePath)

## Description

Hardcoded file and directory paths should not be stored directly in source code.

Such values are environment-specific and make the code harder to configure and reuse. This applies to both Windows and Unix-style absolute paths.

Recommended storage options:

* constants
* information registers
* catalogs, exchange plan nodes, or other metadata objects
* a dedicated module with this rule disabled as a last resort

### Nuances

The current implementation excludes strings that look like URLs with `http`, `https`, or `ftp` schemes to avoid false positives.

## Examples

Incorrect:

```bsl
EchangeFolder = "c:/exchange/dataexchange";
```

Correct:

```bsl
ExchangeFolder = Constants.ExchangeFolder.Get();
```

or

```bsl
ExchangeFolder = DataExchangeReuse.ExchangeFolder();
```

## Sources

* [v8std: UsingHardcodePath](https://v8std.ru/diagnostics/bslls/UsingHardcodePath/)
