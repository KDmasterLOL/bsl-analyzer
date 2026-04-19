# EmptyStatement provenance

## Assessment

`EmptyStatement` is a good candidate for `MIT OR Apache-2.0`.

The rule is a generic code-quality check: a standalone semicolon is usually a typo or leftover syntax noise. This idea is not tied to a unique 1C standard and does not depend on any specific expression from `bsl-language-server`.

The current implementation in `bsl-analyzer` is local:

- the diagnostic is emitted during local AST to HIR lowering when an `EMPTY_STMT` node is encountered;
- nearby parse errors suppress the report through local lowering logic;
- the IDE layer adds a local quick-fix that removes the extra semicolon.

## Source basis

No direct normative 1C standard is required for this diagnostic.

This is a general code-hygiene rule.

## Residual risk

Residual risk is low.

- the rule is generic and obvious;
- the implementation is tightly coupled to the local parser/lowering pipeline;
- current tests are short and generic.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
