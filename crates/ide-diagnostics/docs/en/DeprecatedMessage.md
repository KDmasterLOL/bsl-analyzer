# Restriction on Using the Deprecated Method `Message` (DeprecatedMessage)

## Description

For user-facing notifications, the recommended approach is to use the
`UserMessage` object (`СообщениеПользователю`) rather than the global
`Message()` / `Сообщить()` method.

`UserMessage` supports richer interaction with the UI, including linking a
message to a specific form field. The old global method should not be used in
new code.

When the Standard Library is available, it is recommended to use the helper
procedures `CommonUse.MessageToUser()` / `ОбщегоНазначения.СообщитьПользователю()`
or the corresponding client helper.

## Examples

Incorrect:

```bsl
Message("Customer name is required");
```

Correct:

```bsl
UserMessage = New UserMessage;
UserMessage.Text = "Customer name is required";
UserMessage.Field = "CustomerName";
UserMessage.Message();
```

## Sources

- [ITS: Restriction on using the method `Message` (RU)](https://its.1c.ru/db/v8std#content:418:hdoc)
- [v8std: #std418 Restriction on using the method `Сообщить`](https://v8std.ru/std/418/)
