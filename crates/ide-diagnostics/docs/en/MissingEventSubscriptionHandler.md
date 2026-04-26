# Event subscription handler missing (MissingEventSubscriptionHandler)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic validates handlers referenced by event subscriptions in configuration metadata.

The current implementation checks:

- the handler is not empty;
- the handler format is complete and contains a method name;
- the referenced common module exists;
- the common module is marked as server-side;
- the referenced method exists;
- the method is exported.

The check runs only for the `SessionModule`, and all findings are attached to the beginning of that file because the problem belongs to configuration metadata rather than to a specific BSL source line.

## Examples

### Incorrect

```bsl
// Event subscription metadata references:
// CommonModule.EventHandlers.BeforeWrite
//
// but the module or method is missing, or the method is not exported.
```

### Correct

```bsl
Procedure BeforeWriteHandler(Source, Cancel) Export
    // Handler implementation
EndProcedure

// Event subscription metadata references:
// CommonModule.EventHandlers.BeforeWriteHandler
```

## Sources
- [v8std.ru: MissingEventSubscriptionHandler (RU)](https://v8std.ru/diagnostics/bslls/MissingEventSubscriptionHandler/)
