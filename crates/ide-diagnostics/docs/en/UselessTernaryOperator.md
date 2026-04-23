# Useless ternary operator (UselessTernaryOperator)

## Description

This diagnostic reports ternary operators that can be reduced to a simpler expression because they already encode a boolean constant pattern.

The current implementation is intentionally narrow. It reports a ternary expression when:

- the condition itself is a boolean literal;
- or both branches are boolean literals.

This means the rule does **not** try to detect every semantically redundant ternary form. It only catches a small set of obvious boolean-literal cases.

## Examples

### Useless

```Bsl
A = ?(B = 1, True, False);
```

```Bsl
A = ?(B = 0, False, True);
```

### Also reported by the current implementation

```Bsl
A = ?(B = 1, True, True);
```

### Not reported by this rule

```Bsl
A = ?(B = 0, 0, False);
```

Only one branch is boolean here, so the current detector skips it.

## Sources

- Generic simplification guidance for boolean expressions.
- [v8std.ru: UselessTernaryOperator](https://v8std.ru/diagnostics/bslls/UselessTernaryOperator/)
