# Missed postfix "Cached" (CommonModuleNameCached)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Common modules with repeated-use return values should make that behavior obvious
in their name.

According to the common-module naming rules, modules that reuse return values
should include a caching postfix:

- `Cached` / `ПовтИсп` for the server-side variant;
- `ClientCached` / `КлиентПовтИсп` for the client-side variant.

The postfix makes it clear that a method call may return a reused value instead
of recalculating the result on every invocation.

## Examples

Valid names: `FilesClientCached`, `UsersInternalCached`

Invalid names for cached modules: `FilesClient`, `UsersInternal`

## Sources

Primary source: [Standard: rules for creating common modules (RU)](https://its.1c.ru/db/v8std#content:469:hdoc:3.2.3)

Secondary source: [v8std.ru: #std469](https://v8std.ru/std/469/)

Additional references:
- [v8std.ru: CommonModuleNameCached](https://v8std.ru/diagnostics/bslls/CommonModuleNameCached/)
- [v8std.ru: common-module-name-cached](https://v8std.ru/diagnostics/v8-code-style/common-module-name-cached/)
