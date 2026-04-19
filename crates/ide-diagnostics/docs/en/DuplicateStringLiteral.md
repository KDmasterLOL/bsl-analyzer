# Duplicate string literal (DuplicateStringLiteral)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->

Using the same string literal many times in one method or module makes the code harder to maintain.

When the text changes, one of the occurrences can be missed. Repeated literals also often appear after copy-paste and hide the fact that the code depends on one shared value.

The duplicated text can usually be moved into a local variable, a named constant, or a helper function.

### Features of the implementation of diagnostic

- By default the diagnostic compares literals case-insensitively, so `"AAAA"` and `"AaaA"` are treated as the same text.
- The minimum analyzed literal length cannot be set below the default threshold. Short service strings such as `""`, `"0"` or `"1"` would otherwise create too many false positives.
- The repetition threshold cannot be lower than `1`.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

```bsl
Procedure Test(Param)
    Result = "Value";
    If Param = "One" Then
        Result = Result + One("Value");
    Else
        Result = Result + Two("Value");
    EndIf; 
EndProcedure
```

```bsl
Procedure Test(Param)
    StringValue = "Value";
    If Param = "One" Then
        Result = Result + One(StringValue);
    Else
        Result = Result + Two(StringValue);
    EndIf; 
EndProcedure
```

```bsl
Procedure Test2(Param)
    Result = "Value";
    If Param = "One" Then
        Result = Result + One("Value");
    Else
        Result = Result + Two("Value");
    EndIf; 
EndProcedure

Procedure Test3(Param)
    If Param = "Five" Then
        Result = Result + Five("Value");
    EndIf; 
EndProcedure
```

```bsl
Procedure Test2(Param)
    If Param = "One" Then
        Result = Result + One(StringValue());
    Else
        Result = Result + Two(StringValue());
    EndIf; 
EndProcedure

Procedure Test3(Param)
    If Param = "Five" Then
        Result = Result + Five(StringValue());
    EndIf; 
EndProcedure

Function StringValue()
   Return "Value";
EndFunction
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->
No direct standard source.
