# Incorrect use of the method "WriteLogEvent" (UsageWriteLogEvent)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
`WriteLogEvent` / `ЗаписьЖурналаРегистрации` should be used consistently when code writes operational or error information into the event log.

The current diagnostic checks several practical rules:

- the call must contain at least five parameters;
- the second parameter, log level, must be specified explicitly;
- the fifth parameter, comment, must not be empty;
- inside `Except`, the log level should be `Error`;
- inside `Except`, the comment should contain `DetailErrorDescription(ErrorInfo())`, unless the same exception path re-raises the error.

This rule is intentionally practical rather than formal. It helps catch incomplete or misleading event-log writes, especially in exception handling paths.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

Incorrect:
```bsl
    WriteLogEvent("Event");// error
    WriteLogEvent("Event", EventLogLevel.Error);// error
    WriteLogEvent("Event", EventLogLevel.Error, , );// error
    WriteLogEvent("Event", , , , DetailErrorDescription(ErrorInfo()));

    WriteLogEvent("Event", EventLogLevel.Error, , , );// error

    Try
      ServerCode();
    Except
      WriteLogEvent("Event", EventLogLevel.Error, , ,
        ErrorDescription()); // error
      WriteLogEvent("Event", EventLogLevel.Error, , ,
        "Commentary 1"); // error
    EndTry;
```

Correct:
```bsl
    Try
      ServerCode();
    Except

      ErrorText = DetailErrorDescription(ErrorInfo());
      WriteLogEvent(NStr("en = 'Performing an operation'"), EventLogLevel.Error, , ,
         ErrorText);
    EndTry;

    Try
      ServerCode();
    Except

      ErrorText = DetailErrorDescription(ErrorInfo());
      WriteLogEvent(NStr("en = 'Performing an operation'"), EventLogLevel.Error, , ,
         ErrorText);

      Raise;
    EndTry;
```
If an outer `Try` block already writes to the event log, a nested `Try` block may only re-raise the exception:
```bsl
Процедура ЗагрузитьДанные() Экспорт
    Попытка
        ВыполнитьЗаписьДанных();
    Исключение
        ЗаписьЖурналаРегистрации(); // <- исключение подавляется с записью в ЖР
    КонецПопытки;
КонецПроцедуры

Процедура ВыполнитьЗаписьДанных()
    НачатьТранзакцию();
    Попытка
        // ...
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение; // <- вложенная попытка, запись в ЖР не требуется
    КонецПопытки;
КонецПроцедуры
```
## Sources
* [Using the event log (RU)](https://its.1c.ru/db/v8std#content:498:hdoc)
* [Catching Exceptions in Code (RU)](https://its.1c.ru/db/v8std#content:499:hdoc)
