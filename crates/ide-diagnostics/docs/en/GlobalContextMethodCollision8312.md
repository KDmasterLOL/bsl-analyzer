# Global context method names collision (GlobalContextMethodCollision8312)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Starting with platform version `8.3.12`, 1C added built-in global context
methods for bitwise operations. If the configuration already contains custom
functions with the same names, these names collide with the platform API.

Such functions should be renamed or removed, and their call sites should be
migrated to the built-in platform methods.

Russian variant|English variant
:-: | :-:
ПроверитьБит|CheckBit
ПроверитьПоБитовойМаске|CheckByBitMask
УстановитьБит|SetBit
ПобитовоеИ|BitwiseAnd
ПобитовоеИли|BitwiseOr
ПобитовоеНе|BitwiseNot
ПобитовоеИНе|BitwiseAndNot
ПобитовоеИсключительноеИли|BitwiseXor
ПобитовыйСдвигВлево|BitwiseShiftLeft
ПобитовыйСдвигВправо|BitwiseShiftRight

The diagnostic checks both Russian and English variants because both are part of
the public platform API.

## Examples
```bsl
Функция ПобитовоеИ(Значение1, Значение2)
    Возврат Значение1 И Значение2;
КонецФункции
```

```bsl
// Use the built-in platform method instead
Результат = ПобитовоеИ(255, 15);
```

## Sources
* Primary source: [Migrating configurations to 1C:Enterprise 8.3 without 8.2 compatibility mode (RU)](https://its.1c.ru/db/metod8dev#content:5293:hdoc:pereimenovaniya_metodov_i_svojstv)
* Secondary source: [v8std.ru: GlobalContextMethodCollision8312](https://v8std.ru/diagnostics/bslls/GlobalContextMethodCollision8312/)
