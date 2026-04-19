# Missing "Global" Postfix (CommonModuleNameGlobal)

## Description

Global common modules should make that role explicit in the module name.
According to the 1C naming rules, a common module with the `Global` flag must
end with the `Глобальный` or `Global` postfix. The `Client` postfix is not
used for this module type.

This naming convention makes global modules easy to recognize in metadata and
reduces confusion with ordinary client, server, or client-server common
modules.

## Examples

Incorrect:

```bsl
ОбновлениеИнформационнойБазы
StandardSubsystems
```

Correct:

```bsl
ОбновлениеИнформационнойБазыГлобальный
StandardSubsystemsGlobal
```

## Sources

- [ITS: Common module naming rules, section 3.2.1 (RU)](https://its.1c.ru/db/v8std#content:469:hdoc:3.2.1)
- [v8std: #std469 Common module naming rules](https://v8std.ru/std/469/)
- [v8std: common-module-name-global](https://v8std.ru/diagnostics/v8-code-style/common-module-name-global/)
