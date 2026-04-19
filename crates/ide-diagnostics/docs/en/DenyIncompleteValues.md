# Deny Incomplete Values for Register Dimensions (DenyIncompleteValues)

## Description

Register dimensions are often expected to contain meaningful values. When an
empty or incomplete dimension value is accepted, the register can accumulate
records that are hard to interpret, validate, or use in reports.

The platform provides a built-in metadata flag for this purpose:
`DenyIncompleteValues`. When the flag is enabled for a dimension, the platform
itself enforces the completeness check and removes the need for ad-hoc runtime
validation.

This diagnostic reports register dimensions where that flag is disabled. It
applies to information, accumulation, accounting, and calculation registers.

False positives are possible when empty dimension values are intentionally
allowed by the business model.

## Sources

- [ITS: Fill check and write check in applied solutions (RU)](https://its.1c.ru/db/pubv8devui#content:225:1)
- [1C Developer Guide: Properties of an information register dimension (RU)](https://its.1c.ru/db/v8323doc#bookmark:dev:TI000000349)
- [1C Developer Guide: Properties of an accumulation register dimension (RU)](https://its.1c.ru/db/v8323doc#bookmark:dev:TI000000363)
