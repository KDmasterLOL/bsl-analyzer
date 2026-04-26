# Unused local method (UnusedLocalMethod)

## Description

Modules should not contain local procedures and functions that are never called.

Unused methods make the code harder to navigate and often indicate unfinished refactoring or dead logic. The diagnostic skips several categories that are expected to be called indirectly, such as exported methods, extension methods, platform handlers, and attachable methods with configured prefixes.

## Sources

* [Standard: Module texts (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
* [v8std: UnusedLocalMethod](https://v8std.ru/diagnostics/bslls/UnusedLocalMethod/)
