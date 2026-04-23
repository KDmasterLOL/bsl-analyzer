# Using external code tools (UsingExternalCodeTools)

## Description

In application solutions, loading or creating external code artifacts requires special care because such code is outside the main configuration and may bypass normal review and delivery controls.

The current implementation reports direct use of APIs such as:

- `ExternalDataProcessors.Create/Connect`
- `ExternalReports.Create/Connect`
- `ConfigurationExtensions.Create`

### Restrictions

The current implementation does not distinguish server and client execution context, so the diagnostic may report both.

## Examples

Incorrect:

```bsl
ExternalDataProcessors.Connect("PathToProcessing", False);
ExternalReports.Create("ReportName");
```

Safer approach:

```bsl
// Use reviewed and built-in functionality instead of loading external code directly.
```

## Sources

* [Restriction on execution of "external" code (RU)](https://its.1c.ru/db/v8std#content:669:hdoc)
* [v8std: UsingExternalCodeTools](https://v8std.ru/diagnostics/bslls/UsingExternalCodeTools/)
