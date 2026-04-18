# Parser / SDBL HIR Audit

## Scope

This note records the current provenance assessment for:

- `crates/parser`
- `crates/sdbl-hir`
- related SDBL lexer/parsing support in `crates/lexer`

## Key distinction

The SDBL language itself belongs to the 1C platform as a language/specification.
That does **not** mean an implementation of the language can automatically be
placed under `LGPL`.

For licensing purposes, these must be separated:

- the language, syntax, semantics, and standard behavior;
- the specific textual grammar files and parser implementation choices;
- the specific code written to tokenize, parse, lower, and resolve SDBL.

In copyright terms, the language idea/system is one thing, while the concrete
grammar text and implementation can still be protected expression.

## Legal framing

Useful primary references:

- U.S. Copyright Office FAQ: copyright does not protect ideas, systems, or methods
  of operation; it protects the way they are expressed.
- U.S. Copyright Office, `Computer Programs`: copyright does not extend to ideas,
  program logic, algorithms, systems, methods, concepts, or layouts.
- U.S. Copyright Office, Circular 33: copyright excludes ideas, procedures,
  processes, systems, and methods of operation, while still allowing protection
  for original expression that describes or implements them.

Practical consequence:

- implementing SDBL as a language is not itself the problem;
- copying or closely adapting `bsl-parser` grammar text or grammar structure may be.

## Local evidence

### 1. Explicit project history

Early project planning explicitly names `bsl-parser` as the grammar source:

- `docs/planning/SOURCES.md` in the initial history lists:
  - `SDBLParser.g4`
  - `SDBLLexer.g4`
  - grammar rules and tokens as source material for parser implementation

This is strong evidence that the parser was not originally developed as a
clean-room implementation.

### 2. Earlier parser comments

Early local SDBL parser code explicitly referenced upstream grammar:

- initial `crates/parser/src/grammar/sdbl.rs` contained
  `Grammar reference: SDBLParser.g4 from bsl-parser`

This was later removed from the codebase, but it remains relevant provenance
evidence.

### 3. Current parser comments still reference ANTLR grammar

Current local code still contains comments such as:

- `crates/parser/src/grammar/sdbl/select.rs`
  - `ANTLR grammar has all permutations...`
  - `In ANTLR grammar, JOIN alone defaults to INNER JOIN`

These comments do not prove copying by themselves, but they reinforce that the
ANTLR grammar was used directly as an implementation reference.

### 4. Upstream grammar licensing

The sibling `../bsl-parser` repository marks its grammar files as
`LGPL-3.0-or-later`, including:

- `src/main/antlr/SDBLParser.g4`
- `src/main/antlr/SDBLLexer.g4`
- `src/main/antlr/BSLParser.g4`
- `src/main/antlr/BSLLexer.g4`

Therefore, any close adaptation of those grammar files is a direct copyleft risk.

## Assessment by layer

### `crates/parser`

Current assessment: **high copyleft risk**

Why:

- the project history explicitly says grammar rules and tokens were taken from
  `bsl-parser`;
- local SDBL parser code historically cited `SDBLParser.g4` as grammar reference;
- the local parser reproduces a broad bilingual SDBL grammar and token space that
  appears to be derived from the ANTLR grammar, even though it is implemented in a
  completely different architecture;
- the architecture is original (`rust-analyzer`-style event parser + Rowan), but
  architecture alone does not erase derivation at the grammar-expression level.

What is favorable:

- the parser is not an ANTLR port line-by-line;
- it uses a local event-based parser design, local syntax kinds, local sink, and
  local recovery strategy;
- many implementation details are clearly original and tailored to this codebase.

Bottom line:

- **the implementation architecture is original;**
- **the grammar content is still likely derivative.**

This crate should remain in the copyleft-risk bucket until a dedicated grammar
rewrite or clean-room process is completed.

### `crates/lexer` SDBL part

Current assessment: **high copyleft risk**

Why:

- the SDBL lexer defines a large bilingual token catalog that closely tracks the
  same language surface as `SDBLLexer.g4`;
- the selection and breakdown of keywords, metadata object types, virtual tables,
  period types, and functions likely originates from the upstream grammar work.

Even if individual keywords are not protectable on their own, the concrete lexer
inventory and organization are not strong candidates for immediate permissive
relicensing without further cleanup.

### `crates/sdbl-hir`

Current assessment: **medium risk, materially more original than parser**

Why this layer looks more independent:

- it implements scope handling, semantic lowering, source maps, completion
  contexts, metadata-based type resolution, and semantic diagnostics;
- its structure is driven by local IDE/HIR goals rather than ANTLR parse-tree
  traversal;
- the code expresses semantic behavior in local abstractions like `Scope`,
  `SdblPackage`, `SdblQuery`, `FieldDef`, `JoinType`, and source-map categories;
- no direct evidence was found that `sdbl-hir` was copied from `bsl-parser`.

Why caution still remains:

- it sits directly on top of the parser and depends on still-unaudited grammar
  infrastructure;
- some diagnostic and completion behavior may have been validated against upstream
  Java behavior, which complicates provenance if code/tests/docs are too close.

Bottom line:

- `sdbl-hir` is a much better future candidate for permissive licensing than
  `parser`, but it should not be treated as clean by default until the parser
  layer is resolved.

## Test fixtures

### Favorable findings

- `crates/parser/tests/fixtures/Module.bsl` contains an explicit `CC BY 4.0`
  header from `ООО 1С-Софт`, so this fixture has its own provenance trail.
- `crates/parser/tests/fixtures/user_query_with_highlighting_issue.sdbl` was not
  found in the local `../bsl-parser` tree during this audit, which suggests it may
  be an original or independently sourced fixture.

### Remaining caution

- parser and SDBL tests still need a dedicated fixture audit, especially where
  grammar acceptance cases could have been copied or translated from upstream test
  corpora.

## Recommended licensing posture today

### Keep in copyleft-risk bucket

- `crates/parser`
- SDBL portions of `crates/lexer`

### Better future candidates after parser cleanup

- `crates/sdbl-hir`

## Path to permissive licensing

The realistic path is not “declare parser permissive now”, but:

1. Treat current parser/lexer grammar as derived-risk code.
2. Build a clean-room rewrite plan for SDBL grammar support using primary language
   documentation and independently authored tests.
3. Prefer official 1C language documentation and your own observed parser behavior
   as the specification source, rather than `bsl-parser` grammar text.
4. Replace or audit fixtures that may trace back to `bsl-parser`.
5. Reassess `sdbl-hir` after parser cleanup, since it is likely to become
   permissive first.

## Practical conclusion

Today, the best working conclusion is:

- the **SDBL language itself** does not force `LGPL`;
- the **current parser/lexer implementation** likely still carries `LGPL` risk
  because grammar derivation from `bsl-parser` is well documented;
- the **`sdbl-hir` semantic layer** appears substantially more original and is a
  plausible future `MIT OR Apache-2.0` candidate once the parser foundation is
  disentangled.
