# Commented out code (CommentedCode)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Modules should not keep commented-out code or temporary development markers.
Such fragments usually appear during debugging, experiments, or refactoring and
then stay in the file long after they stop being useful.

Commented code makes the module harder to read because the reader has to decide
whether the fragment is still relevant, accidentally disabled, or simply
forgotten. If the code may be needed later, version control should preserve it
instead of leaving it inline.

The diagnostic also treats developer-specific service marks as a smell when they
describe unfinished local work rather than business meaning.

## Examples

### Wrong

```bsl
Procedure BeforeDelete(Failure)
    //If Failure Then
    //    Message("Temporary check");
    //EndIf;
EndProcedure
```

Also wrong:

```bsl
Procedure BeforeDelete(Failure)
    If True Then
        // Ivanov: revisit after merge
    EndIf;
EndProcedure
```

### Correct

Remove dead fragments and temporary notes before committing the module.

## Note

A comment block is treated as commented-out code when at least one line inside
the block is recognized as executable or declaration-like code.

## Sources

Primary source: [Standard: module texts (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)

Secondary source: [v8std.ru: #std456](https://v8std.ru/std/456/)

Additional reference: [v8std.ru: CommentedCode](https://v8std.ru/diagnostics/bslls/CommentedCode/)
