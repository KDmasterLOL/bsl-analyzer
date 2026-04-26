# Metadata object names must not exceed the allowed length (MetadataObjectNameLength)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
Metadata object names should not exceed 80 characters.

Very long names are harder to use in code and may also cause practical problems
when exporting or processing configuration files.

The current implementation checks the configured maximum length for:

- common modules;
- metadata objects with modules;
- registers;
- session-module analysis for metadata objects without modules.

## Examples

Invalid:

```text
VeryLongCatalogNameThatExceedsTheMaximumAllowedLengthAndCausesExportIssuesInConfiguration
```

Correct:

```text
NomenclatureGroup
```

## Sources

* [Standard: Name, synonym, comment (RU)](https://its.1c.ru/db/v8std#content:474:hdoc:2.3)
* [Public mirror: v8std.ru / #std474](https://v8std.ru/std/474/)
