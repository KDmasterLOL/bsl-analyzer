# Missed postfix "FullAccess" (CommonModuleNameFullAccess)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Privileged common modules should make their elevated execution mode explicit in
the module name.

When the `Privileged` flag is enabled, the module code runs without regular
permission checks. According to the common-module naming rules, such modules
should include the `FullAccess` / `ПолныеПрава` postfix in their name.

That postfix acts as a visible warning for maintainers and reviewers: calls into
the module execute with full rights and therefore deserve extra care.

## Examples

Valid names: `FilesFullAccess`, `UpdateDatabaseFullAccess`

Invalid names for privileged modules: `Files`, `UpdateDatabase`

## Sources

Primary source: [Standard: rules for creating common modules (RU)](https://its.1c.ru/db/v8std#content:469:hdoc:3.2.2)

Secondary source: [v8std.ru: #std469](https://v8std.ru/std/469/)

Additional references:
- [v8std.ru: CommonModuleNameFullAccess](https://v8std.ru/diagnostics/bslls/CommonModuleNameFullAccess/)
- [v8std.ru: common-module-name-full-access](https://v8std.ru/diagnostics/v8-code-style/common-module-name-full-access/)
