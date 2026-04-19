# Deprecated ManagedForm type (DeprecatedTypeManagedForm)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->
The legacy type name `ManagedForm` should be replaced with `ClientApplicationForm`.

Using the new name keeps the code aligned with current platform terminology and makes type checks easier to read and maintain.

## Examples

```bsl
If TypeOf(Form) = Type("ManagedForm") Then
    Return;
EndIf;
```

```bsl
If TypeOf(Form) = Type("ClientApplicationForm") Then
    Return;
EndIf;
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->

* [Platform 8.3.16 changelog (RU)](https://dl03.1c.ru/content/Platform/8_3_16_1148/1cv8upd_8_3_16_1148.htm)
