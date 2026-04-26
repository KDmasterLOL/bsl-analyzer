# Duplicate adding or pasting a value to a collection (DuplicatedInsertionIntoCollection)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic looks for repeated insertions into the same collection when the inserted value or key is structurally identical.

In practice such code is often caused by copy-paste mistakes or by accidentally repeating the same insertion branch.

The current implementation distinguishes two method families:

- `Add` / `Добавить`: compares all arguments;
- `Insert` / `Вставить`: compares only the first argument, because the key is the important part.

It also contains several local filters and control-flow heuristics:

- some special values are allowed to repeat (`Undefined`, `Null`, `0`, empty strings, `Chars.*`);
- reassignment of the receiver or its parent path resets duplicate tracking;
- the configuration option `isAllowedMethodADD` can disable `Add` checking and leave only `Insert`.

## Examples

### Incorrect

```bsl
Items = New Array;
Items.Add(Value);
Items.Add(Value);

Params = New Structure;
Params.Insert("Company", CurrentCompany);
Params.Insert("Company", CurrentCompany);
```

### Correct

```bsl
Items = New Array;
Items.Add(Value1);
Items.Add(Value2);

Params = New Structure;
Params.Insert("Company", CurrentCompany);
Params.Insert("Contractor", CurrentContractor);
```

## Sources
- [v8std.ru: DuplicatedInsertionIntoCollection (RU)](https://v8std.ru/diagnostics/bslls/DuplicatedInsertionIntoCollection/)
