# Global Module with "Client" Postfix (CommonModuleNameGlobalClient)

## Description

Global common modules already express their execution role through the
`Глобальный` or `Global` postfix. Adding the `Клиент` or `Client` postfix to
the same module name is redundant and does not match the 1C naming rules.

If a common module is global, its name should end with `Глобальный` or
`Global` without an additional client postfix.

## Examples

Incorrect:

```bsl
ОбновлениеИнформационнойБазыГлобальныйКлиент
ConfigurationUpdateGlobalClient
```

Correct:

```bsl
ОбновлениеИнформационнойБазыГлобальный
ConfigurationUpdateGlobal
```

## Sources

- [ITS: Common module naming rules, section 3.2.1 (RU)](https://its.1c.ru/db/v8std#content:469:hdoc:3.2.1)
- [v8std: #std469 Common module naming rules](https://v8std.ru/std/469/)
- [v8std: common-module-name-global-client](https://v8std.ru/diagnostics/v8-code-style/common-module-name-global-client/)
