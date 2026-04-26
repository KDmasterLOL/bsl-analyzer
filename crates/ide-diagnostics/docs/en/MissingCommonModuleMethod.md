# Calling a missing common module method (MissingCommonModuleMethod)

## Description

This diagnostic reports calls to common module methods that cannot be resolved
as exported methods of the referenced module.

Typical cases:

- the common module does not contain the requested method;
- the method exists, but it is not exported;
- source code for the target common module is unavailable, so its public API
  cannot be confirmed.

The diagnostic does not trigger when the left side of the qualified call is a
local variable or a parameter that shadows the common module name.

## Examples

Incorrect:

```bsl
Процедура Тест()
    ЦеноваяПолитика.РассчитатьСкидку(Сумма);
КонецПроцедуры
```

```bsl
Процедура Тест()
    ОбщегоНазначения.ВнутреннийМетод();
КонецПроцедуры
```

Correct:

```bsl
Процедура Тест()
    ЦеноваяПолитика.ПолучитьСкидку(Сумма);
КонецПроцедуры
```

## Sources

- Secondary reference: [v8std.ru: MissingCommonModuleMethod](https://v8std.ru/diagnostics/bslls/MissingCommonModuleMethod/)
