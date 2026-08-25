# Deprecated platform API (DeprecatedPlatformApi)

## Description

Reports calls and references to 1C:Enterprise platform APIs that are marked as deprecated by the platform documentation.

The diagnostic covers deprecated global functions, global methods, platform types, and platform attributes that have a known replacement. Message text names the specific deprecated API and keeps the replacement suggested by the platform fact registry.

`DeprecatedMethodCall` is a separate diagnostic for project source methods deprecated by documentation comments. `GetFormMethod` is also separate and can be reported together with this diagnostic for `GetForm()` / `ПолучитьФорму()`.

## Examples

Incorrect:

```bsl
OperationDate = CurrentDate();
Position = Find("abcdef", "cd");
Form = GetForm("Form");
Description = DetailErrorDescription(ErrorInfo());
```

Correct:

```bsl
OperationDate = CurrentSessionDate();
Position = StrFind("abcdef", "cd");
OpenForm("Form");
Description = ErrorProcessing.DetailErrorDescription(ErrorInfo());
```

The error-processing manager is reached through the `ErrorProcessing` global
property. `ErrorProcessingManager` is the type of that property and cannot be
written in code.

## Sources

There is no direct 1C standard behind this diagnostic: the rule covers any
deprecated platform API, and the platform documentation is what defines the set.
The numbers below back individual deprecated facilities, not the rule itself.

Related public context:

- [v8std: Work in different time zones](https://v8std.ru/std/643/)
- [v8std: Opening forms](https://v8std.ru/std/404/)
- 1C:Enterprise platform changelogs for deprecated APIs.
