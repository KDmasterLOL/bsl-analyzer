# Disable safe mode (DisableSafeMode)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
In addition to configuration code, the application solution can execute third-party program code, which can be connected in various ways (external reports and data processing, extensions, external components, etc.). The developer cannot guarantee the reliability of this code. An attacker can include various destructive actions in it that can harm user computers, servers, and data in the program.

The listed security problems are especially critical when operating configurations in the service model, because Having gained access to the service, malicious code can immediately gain access to all applications of all users of the service.

It is important to control the execution of such external code in safe mode, in exceptional cases (after verification) allowing code to be executed in unsafe mode.

The current implementation reports:

- `SetSafeMode(False)`;
- `SetSafeMode(<variable or non-true expression>)`;
- `SetSafeModeDisabled(True)`;
- `SetSafeModeDisabled(<variable or non-false expression>)`.

It does not report `SetSafeMode(True)`, `SetSafeModeDisabled(False)`, or object-qualified calls such as `Module.SetSafeMode(False)`.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
```bsl
SetSafeMode(False); // reported

Value = False;
SetSafeMode(Value); // reported

SetSafeMode(True); // not reported

SetSafeModeDisabled(True); // reported

Value = True;
SetSafeModeDisabled(Value); // reported

SetSafeModeDisabled(False); // not reported
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->
<!-- Примеры источников

* Source: [Standard: Modules (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
* Useful information: [Refusal to use modal windows (RU)](https://its.1c.ru/db/metod8dev#content:5272:hdoc)
* Источник: [Cognitive complexity, ver. 1.4](https://www.sonarsource.com/docs/CognitiveComplexity.pdf) -->
- [Developer's Guide 8.3.22: Safe operation (RU)](https://its.1c.ru/db/v8322doc#bookmark:dev:TI000000186)
- [Standard: Restriction on the execution of "external" code (RU)](https://its.1c.ru/db/v8std/content/669/hdoc)
- [Standard: Server API Security (RU)](https://its.1c.ru/db/v8std/content/678/hdoc)
- [Standard: Restrictions on the use of Execute and Eval on the server (RU)](https://its.1c.ru/db/v8std#content:770:hdoc)
- [Standard: Using Privileged Mode (RU)](https://its.1c.ru/db/v8std/content/485/hdoc)
- [v8std.ru: DisableSafeMode (RU)](https://v8std.ru/diagnostics/bslls/DisableSafeMode/)
