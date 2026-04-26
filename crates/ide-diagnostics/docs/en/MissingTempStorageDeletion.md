# Missing temporary storage data deletion after using (MissingTempStorageDeletion)

## Description

This diagnostic checks `GetFromTempStorage()` / `ПолучитьИзВременногоХранилища()` calls that do not have a matching later `DeleteFromTempStorage()` / `УдалитьИзВременногоХранилища()` call in the same method or module-level body.

The current implementation is narrower than the full lifecycle guidance around temporary storage. It works as a local structural check:

- it finds every read from temporary storage;
- it looks for a later delete call in the same body;
- it compares the first call argument structurally, not by raw text.

That structural comparison allows cases such as `Result.AddressResult` to match correctly.

The implementation does not attempt to prove broader storage-lifetime correctness or model every reusable-storage scenario from platform recommendations.

## Examples

### Correct

```bsl
Procedure LoadData(Address)
    Data = GetFromTempStorage(Address);
    ProcessData(Data);
    DeleteFromTempStorage(Address);
EndProcedure
```

### Incorrect

```bsl
Procedure LoadData(Address)
    Data = GetFromTempStorage(Address);
    ProcessData(Data);
EndProcedure
```

## Sources

- [Long-term operations on the server, part 3.1 - Standard 1C (RU)](https://its.1c.ru/db/v8std#content:642:hdoc)
- [Minimizing the number of server calls, part 7.3 - Standard 1C (RU)](https://its.1c.ru/db/v8std#content:487:hdoc)
- [Temporary Storage Engine - Developer's Guide (RU)](https://its.1c.ru/db/v8319doc#bookmark:dev:TI000000810)
- [v8std.ru: MissingTempStorageDeletion (RU)](https://v8std.ru/diagnostics/bslls/MissingTempStorageDeletion/)
