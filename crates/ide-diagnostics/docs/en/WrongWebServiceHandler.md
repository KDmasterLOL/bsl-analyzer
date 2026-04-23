# Wrong handler for web service (WrongWebServiceHandler)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic validates handler names assigned to Web service operations in metadata.

It reports two cases:

- the operation has no handler name;
- the named handler cannot be found in the current Web service module.

The current implementation is metadata-based. It resolves handler names declared in Web service metadata against the current module symbol tree. It does not validate the handler body or compare parameter lists.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
Missing handler in metadata
```bsl
// The Web service operation has no assigned handler name.
```

Valid handler
```bsl
Function FillCatalogs(MobileDeviceID, MessageExchange)
    Return MobileOrders.FillCatalogs(MobileDeviceID, MessageExchange);
EndFunction
```

## Sources
- [Developers guide 8.3.20. Internet service mechanisms (RU)](https://its.1c.ru/db/v8320doc#bookmark:dev:TI000000783)
- [Configuration guidelines. Web services and HTTP services (RU)](https://its.1c.ru/db/metod8dev/browse/13/-1/1989/2565/2567/2590)
- [v8std.ru: WrongWebServiceHandler (RU)](https://v8std.ru/diagnostics/bslls/WrongWebServiceHandler/)
