# UnknownPreprocessorSymbol

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This rule follows directly from the public semantics of the BSL preprocessor. Conditional compilation directives accept only platform-defined symbols. Using an unknown symbol is a language error in practical terms, because the directive no longer expresses a valid platform condition.

The idea is therefore not specific to `bsl-language-server`. It is a direct consequence of how `#Если` / `#If` works in BSL.

## Public basis

- BSL preprocessor semantics and the documented set of platform symbols used in conditional compilation.

## Implementation audit notes

Current Rust implementation is simple and local:

- it walks syntax nodes,
- selects `PRE_SYMBOL` nodes,
- checks each symbol against `utils::preprocessor_symbols`,
- reports a diagnostic for unknown entries.

This is a straightforward syntax-level validator and does not depend on parser/SDBL provenance concerns.

## Conclusion

`UnknownPreprocessorSymbol` looks like a strong permissive candidate. The rule comes from public language semantics, and the current implementation is local, simple, and not tied to copied documentation or SDBL-specific infrastructure.
