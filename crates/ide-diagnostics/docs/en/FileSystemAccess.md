# File system access (FileSystemAccess)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->
This diagnostic is a security review tool for code that accesses the file system.

Reading, writing, copying, moving, or deleting files can affect confidentiality, integrity, and availability of data. For that reason, every such code path should be reviewed manually for authorization, path validation, lifecycle of temporary files, and safe interaction between client and server.

The current implementation reports a conservative set of file-system-related constructors and methods.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
```bsl
Text = New TextReader(FilePath, TextEncoding.UTF8);     // review required
Text = New TextWriter(FilePath, TextEncoding.UTF8);     // review required

ValueToFile(FilePath, PersonalData);                    // review required
CopyFile(FilePath, OtherFilePath);                      // review required
MoveFile(FilePath, OtherFilePath);                      // review required
DeleteFiles(FilePath);                                  // review required
CreateDirectory(DirectoryName);                         // review required
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->
<!-- Примеры источников

* Source: [Standard: Modules (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
* Useful information: [Refusal to use modal windows (RU)](https://its.1c.ru/db/metod8dev#content:5272:hdoc)
* Источник: [Cognitive complexity, ver. 1.4](https://www.sonarsource.com/docs/CognitiveComplexity.pdf) -->
* [File system access from configuration code (RU)](https://its.1c.ru/db/v8std/content/542/hdoc)
* [Application launch security (RU)](https://its.1c.ru/db/v8std/content/774/hdoc)
* [Safe mode - Developer's Guide (RU)](https://its.1c.ru/db/v8323doc#bookmark:dev:TI000000186)
* [v8std.ru: #std542](https://v8std.ru/std/542/)
