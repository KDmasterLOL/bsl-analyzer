# DeprecatedMethodCall provenance

## Assessment

`DeprecatedMethodCall` is a good candidate for `MIT OR Apache-2.0`.

The rule does not depend on a unique algorithm or expression from `bsl-language-server`. It follows from a common maintenance practice: once a method is marked as deprecated in its documentation comment, new code should avoid calling it and use the documented replacement instead.

The current implementation in `bsl-analyzer` is local and HIR-based:

- the handler resolves local and qualified calls through project symbol tables;
- it reads method documentation metadata through local APIs;
- it suppresses diagnostics when the caller itself is deprecated;
- it builds diagnostics using local message formatting and local test fixtures.

## Source basis

- 1C standard on documenting procedures and functions: <https://its.1c.ru/db/v8std/content/453/hdoc>
- CWE-477 "Use of Obsolete Function": <http://cwe.mitre.org/data/definitions/477.html>

These sources support the rule concept. They do not require reuse of `bsl-language-server` code or prose.

## Residual risk

Residual risk is low.

- The handler currently looks structurally independent.
- Test scenarios are simple and generic.
- The main cleanup needed here was documentation wording, not algorithmic rewrite.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket. No clean-room rewrite appears necessary based on the current code and tests.
