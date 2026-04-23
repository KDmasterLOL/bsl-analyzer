# Ordinary application support (OrdinaryAppSupport)

## Description

This diagnostic checks configuration settings related to support for ordinary
application mode.

It validates two configuration properties:

- `UseManagedFormInOrdinaryApplication` should be `True`;
- `UseOrdinaryFormInManagedApplication` should be `False`.

The rule is based on the 1C guidance for backward compatibility and mixed-mode
operation. In this project the diagnostic runs only for `SessionModule` files
and only when the analyzer configuration explicitly enables
`ordinary_app_support`.

## Sources

- Source: [1C standard: General configuration requirements (#std467)](https://its.1c.ru/db/v8std#content:467:hdoc)
- Secondary reference: [v8std.ru: OrdinaryAppSupport](https://v8std.ru/diagnostics/bslls/OrdinaryAppSupport/)
