# Scheduled job handler (ScheduledJobHandler)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Scheduled jobs should point to real server-side handlers that can actually be executed by the platform. Broken handler links usually turn into runtime errors or background jobs that never perform useful work.

The current implementation validates scheduled jobs declared in metadata and reports problems when:

- the handler is empty or malformed;
- the referenced common module does not exist;
- the common module is not server-side;
- the handler method does not exist in that module;
- the method is not exported;
- a predefined scheduled job points to a method with parameters;
- the handler method body is empty;
- several scheduled jobs point to the same handler.

The diagnostic runs only for `SessionModule` and places all messages at the beginning of that file, because the actual problem lives in configuration metadata rather than in a specific code line.

## Examples

Correct:

```bsl
Procedure UpdateRates() Export
    // Scheduled job logic
EndProcedure
```

Incorrect:

```bsl
Procedure UpdateRates(StartDate) Export
    // Parameters are not allowed for predefined scheduled jobs
EndProcedure
```

## Sources

- [#std540: General requirements for scheduled jobs (RU)](https://its.1c.ru/db/v8std#content:540:hdoc)
- [1C developer guide: Scheduled jobs (RU)](https://its.1c.ru/db/v8322doc#bookmark:dev:TI000000794)
