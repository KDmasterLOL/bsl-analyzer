# Missed postfix "ClientServer" (CommonModuleNameClientServer)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Client-server common modules should make their mixed execution context explicit
in the module name.

These modules are intended for code that is available both on the client and on
the server without using the `ServerCall` pattern. According to the common
module naming rules, such modules should include the
`ClientServer` / `КлиентСервер` postfix in the name.

That postfix helps distinguish client-server modules from purely client,
server-side, or server-call modules.

## Examples

Valid names: `FilesClientServer`, `CommonClientServer`, `UsersClientServer`

Invalid names for client-server modules: `Files`, `Common`

## Sources

Primary source: [Standard: rules for creating common modules (RU)](https://its.1c.ru/db/v8std#content:469:hdoc:2.4)

Secondary source: [v8std.ru: #std469](https://v8std.ru/std/469/)

Additional references:
- [v8std.ru: CommonModuleNameClientServer](https://v8std.ru/diagnostics/bslls/CommonModuleNameClientServer/)
- [v8std.ru: common-module-name-client-server](https://v8std.ru/diagnostics/v8-code-style/common-module-name-client-server/)
