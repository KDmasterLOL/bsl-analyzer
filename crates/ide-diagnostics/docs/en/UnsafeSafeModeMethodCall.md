# Unsafe SafeMode method call (UnsafeSafeModeMethodCall)

## Description

In 1C:Enterprise 8.3, `SafeMode()` / `БезопасныйРежим()` may return a string with the security profile name. Because of that, using the result directly as a boolean condition is unsafe.

This diagnostic reports conditions where the result of `SafeMode()` is used implicitly as `true` or `false`. The safe form is an explicit comparison with `False`.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

Incorrect:

```bsl
If SafeMode() Then
    // some logic in safe mode...
EndIf;

If Not SafeMode() Then
    // some logic in unsafe mode...
EndIf;
```

Correct:

```bsl
If SafeMode() <> False Then
    // some code
EndIf;
```

## Sources

* [SafeMode method behavior in 8.3 (RU)](https://its.1c.ru/db/metod8dev#content:5293:hdoc:izmenenie_bezopasnyjrezhim)
