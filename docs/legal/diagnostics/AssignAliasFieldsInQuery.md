# Provenance: AssignAliasFieldsInQuery

## Status

Candidate for `MIT OR Apache-2.0` at the diagnostic-rule level, with an important
infrastructure caveat for the current SDBL stack.

## Why this rule exists

This diagnostic is based on the 1C standard for query formatting:

- `Оформление текстов запросов`
- ITS / v8std section `437`
- official URL: `https://its.1c.ru/db/v8std/content/437/hdoc`

The standard explicitly recommends assigning aliases to selected fields and
explicitly using the `КАК` keyword before the alias. That makes the rule itself a
standards-based requirement rather than a project-specific invention.

## Audit result

### Rule and behavior

The rule is clearly grounded in 1C guidance. In that sense, the idea of this
diagnostic is not owned by `bsl-language-server`.

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`
is expressed through the local SDBL HIR pipeline and local diagnostic mapping.

This is favorable for permissive licensing of the diagnostic layer itself:

- the handler consumes local `sdbl_hir::SdblDiagnostic` values;
- fixes are built with local edit/mapping infrastructure;
- the current file is not a literal copy of the Java visitor implementation.

### Documentation

Both local documentation files were rewritten during this audit to avoid relying on
upstream wording and examples.

### Tests

Some local tests used examples that were close to the upstream documentation style
and sample domain (`Currencies.Ref`, `AliasFieldsRef`, nested query examples).

During this audit, the most obvious sample queries were rewritten with different
entities and aliases.

## Infrastructure caveat

Unlike purely BSL/HIR diagnostics, this rule depends directly on the repository's
SDBL parsing and lowering stack. That stack still needs its own provenance review,
especially for parser/grammar-derived parts.

This means two different questions must be separated:

- the diagnostic rule and current handler may be permissive candidates;
- the parser/SDBL infrastructure underneath may still carry independent copyleft
  risk until its audit is completed.

## Conclusion

`AssignAliasFieldsInQuery` is a good candidate for a future permissive bucket at
the diagnostic level because:

- the rule is explicitly documented by 1C standards;
- the current handler is implemented using local SDBL HIR abstractions;
- the most obvious borrowed docs/examples were rewritten.

But compared with `AllFunctionPathMustHaveReturn`, this diagnostic has a stronger
dependency on still-unaudited SDBL infrastructure, so its final relicensing should
be confirmed together with the parser/SDBL audit.
