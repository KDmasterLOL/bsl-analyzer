# Unrecommended Common Module Name (CommonModuleNameWords)

## Description

Common module names should describe the subsystem or mechanism implemented by
the module. Generic words such as `Procedures`, `Functions`, `Handlers`,
`Module`, or `Functionality` do not explain the module purpose and make
navigation through the configuration harder.

This diagnostic reports common module names that contain such generic words.
The default word list is configurable, but the built-in values follow the 1C
common module naming rules.

## Examples

Incorrect:

```bsl
CommonProceduresAndFunctions
DocumentProcessingModule
```

Correct:

```bsl
DocumentProcessing
PartnerDataExchange
```

## Sources

- [ITS: Common module naming rules, section 3.1 (RU)](https://its.1c.ru/db/v8std#content:469:hdoc:3.1)
- [v8std: #std469 Common module naming rules](https://v8std.ru/std/469/)
- [v8std: bslls CommonModuleNameWords](https://v8std.ru/diagnostics/bslls/CommonModuleNameWords/)
