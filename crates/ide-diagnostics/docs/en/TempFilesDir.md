# TempFilesDir() method call (TempFilesDir)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
For temporary files, the platform recommends using `GetTempFileName()` / `ПолучитьИмяВременногоФайла()`. Files created that way remain under platform control and are easier to clean up correctly.

The current diagnostic reports direct calls to `TempFilesDir()` / `КаталогВременныхФайлов()`, because that API often leads to manual construction of temporary file paths that are easier to leak.
## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

Incorrect:

```bsl
Catalog = TempFilesDir();
FileName = String(New UUID) + ".xml";
TempFile = Catalog + FileName;
Data.Write(TempFile);
```

Correct:

```bsl
TempFile = GetTempFileName("xml");
Data.Write(TempFile);
```

To create a temporary directory, it is also recommended to build it from a value returned by `GetTempFileName()` (except for special platform-specific cases such as the web client).

Incorrect:

```bsl
ArchFile = New ZipFileReader(FileName);
ArchCatalog = TempFilesDir()+"main_zip\";
CreateDirectory(ArchCatalog);
ArchFile.ExtractAll(ArchCatalog);
```

Correct:

```bsl
ArchFile = New ZipFileReader(FileName);
ArchCatalog = GetTempFileName() + "\main_zip\";
CreateDirectory(ArchCatalog);
ArchFile.ExtractAll(ArchCatalog);
```

## Sources

- [#std542: Filesystem access from configuration code (RU)](https://its.1c.ru/db/v8std#content:542:hdoc)
