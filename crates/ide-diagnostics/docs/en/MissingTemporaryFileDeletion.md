# Missing temporary file deletion after using (MissingTemporaryFileDeletion)

## Description

This diagnostic checks temporary files created with `GetTempFileName()` / `ПолучитьИмяВременногоФайла()`.

The public rationale comes from 1C guidance: temporary files should be removed explicitly after use instead of relying on cleanup at the next platform start.

The current implementation is a local HIR + CFG check:

- if `GetTempFileName()` is assigned to a variable, the diagnostic looks for a reachable later deletion or move call that uses the same variable;
- if `GetTempFileName()` is used inline, the diagnostic always reports it because cleanup cannot be tracked reliably;
- the set of cleanup methods is configurable through `searchDeleteFileMethod`.

By default the diagnostic treats these methods as cleanup:

- `УдалитьФайлы` / `DeleteFiles`
- `НачатьУдалениеФайлов` / `BeginDeletingFiles`
- `ПереместитьФайл` / `MoveFile`

Custom global, common-module, or manager-module methods can be added through the configuration regex.

## Examples

### Correct

```bsl
TempFileName = GetTempFileName("xml");
Data.Write(TempFileName);
DeleteFiles(TempFileName);
```

### Incorrect

```bsl
TempFileName = GetTempFileName("xml");
Data.Write(TempFileName);
```

### Incorrect inline usage

```bsl
Write(GetTempFileName("xml"));
```

## Sources

- [File system access from application code - Standard 1C (RU)](https://its.1c.ru/db/v8std#content:542:hdoc)
- [v8std.ru: MissingTemporaryFileDeletion](https://v8std.ru/diagnostics/bslls/MissingTemporaryFileDeletion/)
- [v8std.ru: missing-temporary-file-deletion](https://v8std.ru/diagnostics/v8-code-style/missing-temporary-file-deletion/)
