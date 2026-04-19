# Missed postfix "Client" (CommonModuleNameClient)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Client common modules should make their execution side explicit in the module
name.

These modules are intended for logic that runs only on the client side and are
identified by the client availability flags in metadata. According to the common
module naming rules, a non-global client common module should include the
`Client` / `Клиент` postfix in its name.

Global modules are the exception: they follow a different naming pattern and do
not require the `Client` postfix.

## Examples

Valid names: `FilesClient`, `CommonClient`, `StandardSubsystemsClient`

Invalid names for non-global client modules: `Files`, `Common`

## Sources

Primary source: [Standard: rules for creating common modules (RU)](https://its.1c.ru/db/v8std#content:469:hdoc:2.3)

Secondary source: [v8std.ru: #std469](https://v8std.ru/std/469/)

Additional references:
- [v8std.ru: CommonModuleNameClient](https://v8std.ru/diagnostics/bslls/CommonModuleNameClient/)
- [v8std.ru: common-module-name-client](https://v8std.ru/diagnostics/v8-code-style/common-module-name-client/)
