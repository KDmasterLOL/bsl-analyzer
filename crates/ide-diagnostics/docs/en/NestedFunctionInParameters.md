# Initialization of method and constructor parameters by calling nested methods (NestedFunctionInParameters)

## Description

This diagnostic reports nested function calls and parameterized constructors
used directly as arguments of other calls and constructors.

The rule is aimed at readability. Long chains of nested calls are harder to
scan, debug, and step through. In practice it is often clearer to split the
expression into several intermediate variables.

Compact one-line expressions may still be acceptable. The diagnostic therefore
has configuration that allows one-line calls and a small allowlist of method
names such as `NStr` and `PredefinedValue`.

## Examples

Incorrect:

```bsl
Attachments.Insert(
    AttachedFile.Description,
    New Picture(GetFromTempStorage(AttachedFiles.GetFileData(AttachedFile.Ref))));
```

Correct:

```bsl
FileData = AttachedFiles.GetFileData(AttachedFile.Ref);
PictureData = GetFromTempStorage(FileData);
Attachments.Insert(AttachedFile.Description, New Picture(PictureData));
```

## Sources

- Source: [1C standard: Parameters of procedures and functions (#std640)](https://its.1c.ru/db/v8std#content:640:hdoc)
- Secondary reference: [v8std.ru: NestedFunctionInParameters](https://v8std.ru/diagnostics/bslls/NestedFunctionInParameters/)
