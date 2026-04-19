# DuplicateStringLiteral provenance

## Assessment

`DuplicateStringLiteral` is a good candidate for `MIT OR Apache-2.0`.

The rule is a generic maintainability check. It is not derived from a unique 1C standard and does not depend on any project-specific expression from `bsl-language-server`.

The underlying idea is straightforward: repeated string literals are harder to change safely and often indicate copy-paste or a missing named abstraction.

The current implementation in `bsl-analyzer` is local:

- it walks the local syntax tree and groups string literals by normalized text;
- it applies local configuration for threshold, case sensitivity, scope, minimum length, and excluded methods;
- it uses project-specific CST shapes for `CALL_EXPR` and `NEW_EXPR`;
- test scenarios are small and generic.

## Source basis

No direct normative 1C standard is required for this diagnostic.

This is a general code-smell rule based on ordinary maintainability concerns.

## Residual risk

Residual risk is low.

- the algorithm is simple and local;
- the main cleanup needed here was wording in documentation and comments;
- examples and tests are generic and do not appear to rely on protected upstream text.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
