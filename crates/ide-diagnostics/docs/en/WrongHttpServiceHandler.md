# Missing handler for http service (WrongHttpServiceHandler)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic validates metadata assigned to HTTP service methods.

It reports three cases:

- the handler name is empty;
- the named handler is missing in the current HTTP service module;
- the handler exists, but it declares a parameter list different from the expected single request parameter.

The current implementation is metadata-based. It checks handler names declared in HTTP service metadata and resolves them in the current module symbol tree.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
Missing handler in metadata
```bsl
// The HTTP service method has no assigned handler name.
```

Valid handler
```bsl
Function StorageGETRequest(Request)
    Return ModuleRequests.Get(Request);
EndFunction
```

Handler with the wrong number of parameters
```bsl
Function StorageGETRequest(Request, Additional)
    Return ModuleRequests.Get(Request);
EndFunction
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->
<!-- Примеры источников

* Source: [Standard: Modules (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
* Useful information: [Refusal to use modal windows (RU)](https://its.1c.ru/db/metod8dev#content:5272:hdoc)
* Источник: [Cognitive complexity, ver. 1.4](https://www.sonarsource.com/docs/CognitiveComplexity.pdf) -->
- [Developers guide 8.3.20. Internet service mechanisms (RU)](https://its.1c.ru/db/v8320doc#bookmark:dev:TI000000783)
- [Configuration guidelines. Web services and HTTP services (RU)](https://its.1c.ru/db/metod8dev/browse/13/-1/1989/2565/2567/2590)
- [v8std.ru: WrongHttpServiceHandler (RU)](https://v8std.ru/diagnostics/bslls/WrongHttpServiceHandler/)
