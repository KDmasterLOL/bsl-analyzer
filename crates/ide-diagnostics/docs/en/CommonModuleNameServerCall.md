# Missing "ServerCall" Postfix (CommonModuleNameServerCall)

## Description

Common modules that are intended for server calls from client code should make
that role explicit in the module name. According to the 1C naming rules, such
modules should include the `ВызовСервера` or `ServerCall` postfix.

This naming convention makes it clear that the module exposes a server-side
entry point for client code and distinguishes it from other server or shared
common modules.

## Examples

Incorrect:

```bsl
УправлениеДоступомСервер
UserAccessServer
```

Correct:

```bsl
УправлениеДоступомВызовСервера
UserAccessServerCall
```

## Sources

- [ITS: Common module naming rules, section 2.2 (RU)](https://its.1c.ru/db/v8std#content:469:hdoc:2.2)
- [v8std: #std469 Common module naming rules](https://v8std.ru/std/469/)
- [v8std: common-module-name-server-call](https://v8std.ru/diagnostics/v8-code-style/common-module-name-server-call/)
