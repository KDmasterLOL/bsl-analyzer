# EmptyCodeBlock provenance

## Assessment

`EmptyCodeBlock` is a good candidate for `MIT OR Apache-2.0`.

The rule is a generic suspicious-code check: empty branches and loops usually indicate unfinished logic or an editing mistake. This idea is not specific to `bsl-language-server` and does not rely on a unique 1C standard.

The current implementation in `bsl-analyzer` is local:

- empty control-flow blocks are detected during local AST to HIR lowering;
- the diagnostic handler only maps the HIR diagnostic into the IDE layer;
- the tests are short and generic and reflect the implemented behavior directly.

## Source basis

No direct normative 1C standard is required for this diagnostic.

This is a general code-quality rule based on ordinary control-flow hygiene.

## Residual risk

Residual risk is low.

- the algorithmic idea is generic;
- the implementation is tightly coupled to the local lowering pipeline;
- no borrowed prose or fixtures were necessary to justify the rule.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
