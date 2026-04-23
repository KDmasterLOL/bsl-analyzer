# ThisObject assign (ThisObjectAssign)

## Description

In managed form modules and common modules, `ThisObject` / `ЭтотОбъект` is a platform property, not an ordinary writable variable.

Assigning a value directly to this property is an error because the property is read-only.

This problem often appears when old code is migrated to compatibility mode `8.3.3+`: code that previously used `ThisObject` as a local variable name starts conflicting with the built-in property.

## Examples

### Incorrect
```bsl

ThisObject = FormAttributeToValue("Object");

```

### Correct

```bsl
CurrentObject = FormAttributeToValue("Object");
```

The current implementation is intentionally narrow:

- it reports only direct assignment to `ThisObject` / `ЭтотОбъект`;
- it applies only to common modules and form modules;
- property access like `ThisObject.Attribute = Value` is not reported by this rule.

## Sources

- Public platform semantics of `ThisObject` as a built-in module/form property in compatibility mode `8.3.3+`.
- [v8std.ru: ThisObjectAssign](https://v8std.ru/diagnostics/bslls/ThisObjectAssign/)
