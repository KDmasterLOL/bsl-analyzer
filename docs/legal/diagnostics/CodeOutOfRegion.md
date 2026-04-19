# Provenance: CodeOutOfRegion

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in the standard 1C module structure.

Primary source:

- ITS / v8std `#std455`: `Структура модуля`

The rule is organizational: module-level declarations and initialization code
should live inside explicit regions so that the module keeps a predictable
structure.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/code_out_of_region.rs`
uses local syntax and HIR region-tree infrastructure.

This favors permissive treatment:

- the rule is standards-based;
- the implementation is expressed through local parser/region-tree logic;
- explicit `ported from` wording was removed during this audit.

### Documentation

English documentation was rewritten during this audit to explain the rule
directly from standard module organization rather than from upstream wording.

Russian documentation already followed the same standard-based idea.

### Tests

Some local tests clearly mirrored upstream resource names and fixture structure.
During this audit, the obvious borrowed inline fixture texts were replaced with
new local variants while preserving the same behavioral coverage:

- mixed region/non-region module;
- module without regions;
- preprocessor guard case;
- standalone procedure outside region;
- standalone executable statement.

## Remaining caveats

- earlier repository history may still contain upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CodeOutOfRegion` is a good permissive candidate because:

- the rule follows from official module-structure guidance;
- the current implementation is local;
- the most obvious borrowed fixture text and docs were replaced.
