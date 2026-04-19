# External applications starting (ExternalAppStarting)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->
This diagnostic reports calls that start external programs or execute OS commands directly from 1C code.

Such calls are security-sensitive because they can execute arbitrary commands, depend on untrusted input, or bypass the normal application flow.

The current implementation checks these methods:
- System
- RunSystem
- RunApp
- BeginRunningApplication
- RunAppAsync
- FileSystemsClient.RunApp and FileSystems.RunApp
- FileSystemClient.OpenExplorer
- FileSystemClient.OpenFile

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
```bsl
Procedure RunExternalTool()
    System("cmd.exe /c dir");
    RunApp("calc.exe");
EndProcedure
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->
<!-- Примеры источников

* Source: [Standard: Modules (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
* Useful information: [Refusal to use modal windows (RU)](https://its.1c.ru/db/metod8dev#content:5272:hdoc)
* Источник: [Cognitive complexity, ver. 1.4](https://www.sonarsource.com/docs/CognitiveComplexity.pdf) -->
* [Standard: Application launch security (RU)](https://its.1c.ru/db/v8std/content/774/hdoc)
* [Standard: Restriction on execution of external code (RU)](https://its.1c.ru/db/v8std/content/669/hdoc)
* [v8std.ru: #std774](https://v8std.ru/std/774/)
