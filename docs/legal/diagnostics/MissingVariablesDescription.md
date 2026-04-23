# MissingVariablesDescription provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is grounded in public 1C guidance on module structure and documenting module variables. The idea that module-level variables should have comments explaining their purpose is standards-based, not specific to any upstream project.

## Public sources

- `#std455` Module structure.
- Public `v8-code-style` guidance for the variable-description section.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and syntax-driven. It:

- iterates only top-level `ModItem::Variable` entries from the item tree;
- maps each variable back to its `VAR_DEF` syntax node;
- accepts both trailing comments and header comments above the declaration;
- handles annotations by excluding them from the description-search window;
- expands the reported range for exported variables to include `Экспорт` when present.

Local variables inside methods are intentionally outside the scope of this rule.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were corrected to match the real implementation: this is about module-level variables, not every variable in the file.
- Existing tests are local and cover plain variables, export variables, header comments, trailing comments, annotation layouts, local-variable exclusion, and the larger regression fixture.
