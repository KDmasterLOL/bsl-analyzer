# Insert a collection into itself (SelfInsertion)

## Description

Inserting a collection into itself creates a circular reference. That usually leads to broken traversal, corrupted runtime state, or other unpredictable behavior.

The current implementation is intentionally narrow and local:

- it reports direct self-insertion patterns such as `Array.Add(Array)` and `Structure.Insert(..., Structure)`;
- it covers both Russian and English method names for these collection operations;
- unrelated calls that merely pass the same object somewhere else are not reported by this rule.

## Examples

### Incorrect

```bsl
Items = New Array();
Items.Add(Items);
```

```bsl
Settings = New Structure();
Settings.Insert("Key", Settings);
```

### Correct

```bsl
Items = New Array();
Items.Add(Item);
```

## Sources

- [Search for circular links (RU)](https://its.1c.ru/db/metod8dev#content:5859:hdoc)
- [v8std.ru: SelfInsertion](https://v8std.ru/diagnostics/bslls/SelfInsertion/)
