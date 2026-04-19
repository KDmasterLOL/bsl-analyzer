# Provenance: FunctionNameStartsWithGet

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic follows directly from official 1C naming guidance.

Primary sources:

- ITS / v8std `#std647`: names of procedures and functions, section `6.1`
- public diagnostic mapping on `v8std.ru`

The underlying rule is straightforward: a function name should describe the
returned value, so the prefix `Получить` is redundant.

## Audit result

### Production code

Current implementation is split between local HIR lowering and a small adapter
in `crates/ide-diagnostics/src/handlers/function_name_starts_with_get.rs`.

This favors permissive treatment:

- the rule comes from a public 1C standard;
- the implementation is a simple local name-prefix check;
- current scope is narrower than the standard and only covers function names
  that start with Russian `Получить`.

### Documentation

Local RU/EN documentation was rewritten during this audit to cite official and
public 1C sources directly.

### Tests

Current tests are small inline Rust scenarios in the local handler test module.
They cover local behavior such as case-insensitive matching, excluding
procedures, and ignoring English `Get...` names.

## Remaining caveats

- earlier repository history may still contain closer wording to upstream docs;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`FunctionNameStartsWithGet` is a strong permissive candidate because the rule is
explicitly standards-based and the current code/tests are local.
