# DeprecatedTypeManagedForm provenance

## Assessment

`DeprecatedTypeManagedForm` is a good candidate for `MIT OR Apache-2.0`.

The rule follows from a platform-level rename of a built-in type name. It does not depend on a unique diagnostic idea from `bsl-language-server`.

The current implementation in `bsl-analyzer` is local:

- the deprecated type usage is detected during local AST to HIR lowering;
- the diagnostic handler only formats and emits the message;
- test cases are short and generic and do not rely on borrowed fixtures.

## Source basis

- platform changelog documenting the newer type name: <https://dl03.1c.ru/content/Platform/8_3_16_1148/1cv8upd_8_3_16_1148.htm>

This source is sufficient to justify the rule concept and recommended replacement.

## Residual risk

Residual risk is low.

- the diagnostic is based on built-in platform terminology;
- the implementation is structurally simple and local;
- no clean-room rewrite appears necessary.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
