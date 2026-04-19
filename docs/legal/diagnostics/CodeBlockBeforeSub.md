# Provenance: CodeBlockBeforeSub

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic follows from the standard 1C module structure.

Primary source:

- ITS / v8std `#std455`: `Структура модуля`

The rule is organizational rather than algorithmically unique: executable module
body code should come after declarations of procedures and functions.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/code_block_before_sub.rs`
uses local Rowan syntax traversal over the project's own parser output.

This supports permissive treatment:

- the rule is standards-based;
- the implementation is a straightforward local syntax walk;
- explicit `ported from` wording was removed during this audit.

### Documentation

English documentation was rewritten during this audit to explain the rule
directly from the standard module structure.

Russian documentation already expressed the same idea locally and remains aligned
with the standard.

### Tests

Current local tests are inline Rust fixtures and cover direct code blocks,
regions, variable-only headers, English syntax, and region-only method sections.

They no longer rely on a large copied upstream fixture.

## Remaining caveats

- earlier repository history may still contain wording close to upstream docs;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CodeBlockBeforeSub` is a good permissive candidate because:

- the rule comes from official module-structure guidance;
- the current implementation and tests are local;
- the obvious upstream residue in comments/docs was removed.
