# SDBL Provenance Audit — July 2026

> **Read-order warning.** Section “Comparison against the upstream grammars”
> characterises the concrete structure of a third-party LGPL grammar. It is
> written without reproducing upstream text, but it is still knowledge *about*
> upstream expression. Anyone performing a clean-room slice must not read that
> section while writing replacement code. The rest of this document is safe to
> read at any time.

## Why this document exists

The earlier notes in this directory — `sdbl-lexer-audit.md`,
`parser-sdbl-select-audit.md`, `parser-sdbl-inventory.md`,
`parser-sdbl-hir-audit.md`, `parser-bsl-grammar-audit.md` — were written in
April 2026 without access to the upstream grammar files. They reasoned from
structural resemblance and from the project's own planning documents, and they
said so: they consistently used “likely”, “appears”, “assumed”, and one of them
records that a line-by-line audit was still an open action item.

This audit closes that gap. It answers two questions the earlier notes could
only estimate:

1. Did this project actually derive its SDBL layer from `bsl-parser`?
2. How much of that derivation is still present in the code today?

The answers are different from each other, and conflating them is what made the
earlier picture confusing.

## Method

- Upstream `SDBLLexer.g4` and `SDBLParser.g4` were fetched from the public
  repository at `https://github.com/1c-syntax/bsl-parser`, tag `v0.37.2`
  (LGPL-3.0-or-later).
- They were read outside the repository tree and **were not committed here**.
  No upstream grammar text is reproduced in this document.
- Local evidence: the full git history of this repository, and the current
  contents of `crates/lexer/src/sdbl/`, `crates/parser/src/grammar/sdbl/`,
  `crates/parser/src/sdbl_token_converter.rs`, `crates/parser/src/event.rs`.
- Performed 2026-07-27 with Claude Opus 5, on the repository owner's explicit
  instruction to consult the upstream grammars for audit purposes.

The clean-room prohibition in `LICENSING.md` covers consulting the grammars
*while writing replacement code*. Consulting them to establish what was and was
not copied is a different act, and is the only way to answer the question
factually.

## Finding 1 — derivation happened, and our own history proves it

This is no longer an inference. The evidence is internal.

`2ccf30bc` (2025-12-29), the commit that introduced the SDBL lexer roughly eight
hours after the repository was initialised, lists in its own message:

```
References:
- bsl-parser/src/main/antlr/SDBLLexer.g4 (token definitions)
- bsl-parser/src/main/antlr/SDBLParser.g4 (grammar reference)
```

The file header added by the same commit read `Based on SDBLLexer.g4 from
bsl-parser project`.

`73c478d5` (2025-12-30) added the parser. Its commit message does not name the
upstream files, but the file header does — `Grammar reference: SDBLParser.g4
from bsl-parser` — and many of its grammar functions carry a doc-comment quoting
the corresponding upstream rule in ANTLR notation, including ANTLR label syntax
(`=` and `+=` element labels). Not every function was annotated this way, so the
annotation is evidence of how the file was written, not an exhaustive map of
which rules were transcribed. The notation itself cannot be reconstructed from
1C documentation; it only appears in a grammar file.

Supporting records from the same period:

- the initial commit `a6204f78` shipped `docs/planning/SOURCES.md`, naming a
  working copy of `bsl-parser` as the grammar source for both BSL and SDBL;
- integration tests contained hard-coded absolute paths into that working copy,
  removed later by `16c5db20` and `64877fe2`.

For the BSL layer specifically the record is weaker in form but not in
substance. The initial commit that introduced the BSL lexer and grammar names
neither `BSLLexer.g4` nor `BSLParser.g4` in its message or in those files; the
evidence there is the planning document shipped alongside them. A file header
referencing `BSLLexer.g4` appeared shortly afterwards in `fe2f7ed2`. No
equivalent header ever named `BSLParser.g4`.

These markers were removed from the code by `843b00ab` (2026-03-06, “remove
references to source projects”, 125 files) and `a7bce6f4` (2026-04-24, “remove
ANTLR provenance markers”). Removing an attribution does not undo a derivation.
The removals are recorded here so that the history is not mistaken for absence
of provenance.

**Conclusion.** The SDBL lexer and parser, and the BSL grammar layer, were
originally written with the upstream grammar files open. The April 2026 audits
were right about the origin, even though they did not prove it.

## Finding 2 — comparison against the upstream grammars

Upstream at v0.37.2: about 217 token definitions and 68 parser rules. Locally:
154 `SdblTokenKind` variants, 56 SDBL `NodeKind` variants, roughly 50 grammar
functions. Differences below are stated as characterisations; upstream text is
not reproduced.

Where the two implementations now diverge:

- **Ordering.** Upstream orders its token sections alphabetically by the English
  literal. Ours are ordered by usage frequency and by clause order within a
  query. No section reproduces upstream ordering. Ordering is the single
  cheapest thing to carry across in a port, and it did not survive.
- **Multi-word tokens.** Upstream folds multi-word constructs into single
  tokens at the lexer level, twenty-one of them. We have none: every such
  construct is assembled in the parser from separate word tokens.
- **Lexer modes.** Upstream uses seven lexical modes. We have one hand-written
  string scanner and an otherwise flat token enum. One consequence is a genuine
  behavioural difference: without upstream's dot-sensitive mode, a virtual-table
  name appearing as an ordinary column name is classified differently here.
- **Permutation expansion.** Upstream's most distinctive implementation habit is
  writing out permutations of order-independent modifiers by hand. We solve both
  such constructs with loops (`crates/parser/src/grammar/sdbl/select.rs:903` and
  `:505`). These are precisely the sites that would betray a port.
- **Decomposition.** Upstream keeps the query body as one rule with inline
  clauses; we give each clause its own function and syntax node. Upstream treats
  conjunction and disjunction as one flat level; we use a precedence ladder, so
  the two implementations bind `AND` and `OR` differently. Upstream hides
  brace-delimited extension blocks from the parser; we parse them into a node.
  Upstream resolves soft keywords with one very large alternation rule; we
  resolve them with a token converter plus targeted predicates.
- **Token inventory.** More than twenty upstream tokens have no counterpart
  here, including several metadata-object types, several virtual tables, and a
  whole family of mathematical functions of which we implement an arbitrary
  subset. Conversely we carry tokens that do not exist in SDBL at all. A
  transcribed inventory does not look like this.
- **Copying fingerprints.** Upstream's grammar contains several misspellings and
  artificial rule names that would survive any mechanical transcription. All of
  them were searched for across `crates/`. None is present.

Where similarity remains:

- **Top-level rule names.** About eleven of our grammar functions transliterate
  upstream rule names. Some of those names are ordinary parser vocabulary; the
  least forced is the name we use for the group of `ПЕРВЫЕ` / `РАЗЛИЧНЫЕ` /
  `РАЗРЕШЕННЫЕ` modifiers, which is not a term used in 1C documentation. Note
  that in every one of these cases the rule *body* differs, and in two cases it
  is structurally opposite.
- **Token grouping axis.** Both projects separate metadata-object types and
  virtual tables into their own token classes.

**Conclusion.** At the level of expression, today's SDBL layer is an independent
implementation. What survives from the derivation is naming, plus the decision
of how to group two token families. Naming is the thinnest form of protectable
expression, and the slices already completed are why so little else remains.

## Finding 3 — what still carries unmitigated risk

Not everything has been through the clean-room process.

- **Lexer vocabularies.** Slice 3b closed the metadata-object type names on
  2026-07-27. Slices 4 and 5 have not been started: the query-function names
  and the virtual-table names. These lists were transcribed from upstream in
  the first commit and have not been re-derived from 1C documentation since.
  Their present contents already diverge from upstream, but no attestation
  covers them. Separately, `LBrace` and `RBrace` are covered by no attestation
  and owned by no slice; see item 5 of the exit criteria.
- **`sdbl-hir`.** Slice 13 has not been started. Roughly 13.8k lines. It is not
  unexamined: `parser-sdbl-hir-audit.md` assesses it as medium risk and records
  that no direct evidence of copying from `bsl-parser` was found. What has never
  happened is a line-by-line comparison — and the relevant comparison target is
  not the grammar but `bsl-language-server`'s Java implementation, since comments
  removed by `843b00ab` show parts of the lowering semantics were written against
  it. That comparison is the largest remaining unknown in the chain.

- **The SDBL test corpus (Slice 0).** Triaged in April 2026 into buckets A, B and
  C, where bucket C is regression material explicitly marked as unusable as
  clean-room specification input. The recommended first action — rewrite bucket C
  — was never carried out, and `3aa29b99` removed the bucket labels, so the
  classification no longer exists in the tree. Test material of upstream-derived
  shape is a provenance class in its own right and is easy to overlook because it
  is not implementation code.
- **Slice 12.** Recovery and IDE allowances: individual fixes landed, no
  attestation exists.
- **The BSL grammar layer.** Same origin, same first-commit evidence, and no
  slice plan exists at all. See `parser-bsl-grammar-audit.md`.

## Finding 4 — the evidence chain has a gap

`3aa29b99` (2026-05-29, “refactor: prune project comments”, −58k lines) removed
every `CLEAN-ROOM` and `LEGACY` banner from the source, and the Bucket A/B/C
classification from the SDBL test corpus. In `crates/lexer/src/sdbl/mod.rs`
alone this went from 27 provenance annotations to zero.

The attestations in this directory cite those banners by file and line. On
current `develop` those citations no longer resolve, and the remaining
unrewritten surface is no longer visible in the code. The acceptance test files
survived, so the substantive work is intact; what was lost is the map.

Restoring markers on the still-unrewritten vocabulary blocks would close this
gap cheaply. It is a code change and needs its own decision.

## What this means for licensing

Separating the two findings matters, because they point in opposite directions.

The **origin** is documented derivation. That makes an “independent creation”
defence unavailable for the SDBL and BSL grammar layers as originally written,
regardless of how the code looks today.

The **present expression** has largely diverged. Copyright protects expression,
not the language, not the ideas, and not the facts of the 1C platform — keyword
spellings, function names and metadata-type names are platform facts and are not
upstream's to license. On the current code, the surviving overlap is
concentrated in rule naming.

The honest position is therefore neither “we are already clean” nor “we copied
the parser”. It is: *the layer was derived, has been substantially rewritten,
and the rewrite is incomplete in identifiable places.*

## Exit criteria for a Tier B → Tier A flip

No single checklist existed before; this is it.

For `lexer`:

1. ~~Slice 3b — metadata-object type vocabulary re-derived and attested.~~
   Done 2026-07-27, `sdbl-clean-room-slice3b.md`. It landed 18 variants
   rather than 14: the audit found four canonical table roots that the lexer
   never had.
2. Slice 4 — query-function vocabulary re-derived and attested.
3. Slice 5 — virtual-table vocabulary and external-data-source handling.
4. ~~The `Error` fallback variant re-derived.~~ Done 2026-07-27 with Slice 3b,
   which classifies it Tier D: it carries no pattern, and no 1C source defines
   an error token because the documented language describes what is accepted,
   not how a tool represents what is not.
5. `LBrace` / `RBrace` re-derived and attested. **This item did not exist when
   the list was written and is the reason the list was not exhaustive.** The
   brace pair was added by `537527eb` (2026-06-16), after every closed lexer
   slice, so no attestation could have covered it and none does. Slice 3b found
   it while restoring the markers, closed the class at exactly two variants by
   partitioning all 154 `SdblTokenKind` variants, and recommends a separate
   Slice 1-addendum rather than absorbing punctuation into a vocabulary slice.

   Note that “no `LEGACY` marker remains” is no longer a usable check:
   `3aa29b99` removed every marker, so that test now passes vacuously. Slice 3b
   restored markers for the vocabularies it owns and for those still pending,
   but the closed slices' banners are still absent by choice — check the
   vocabularies themselves, and treat this numbered list, not the source, as
   the inventory.

For `parser` (SDBL side):

6. Slice 12 — recovery and IDE allowances attested.
7. Rule naming reviewed. See the non-action below before acting on this.

For `sdbl-hir`:

8. Slice 13 — compare against `bsl-language-server`'s lowering, not only against
   the grammar, then rewrite whatever the comparison finds.

For test material:

9. Slice 0 — bucket C of the SDBL test corpus rewritten, and the bucket
   classification restored in some durable form. Upstream-shaped test data is a
   provenance class of its own; a crate is not clean because its implementation
   is clean.

For `parser` as a crate:

10. The BSL grammar layer needs its own plan. Until it exists, `parser` and
   `lexer` cannot become Tier A even with SDBL fully clean, because the two
   languages share the crates.

Cross-cutting:

11. Provenance notes in this directory updated to match the code.
12. `NOTICE` and `LICENSING.md` updated.

## Options, including one not previously considered

**A. Finish the slices.** Items 1–9 above. Bounded and well understood; item 7
is the expensive one.

**B. Split SDBL into its own crates.** Only helps if the goal is to publish the
SDBL parser separately. The lexer separates almost mechanically; the parser does
not, because the SDBL grammar runs on BSL token kinds through
`sdbl_token_converter.rs` and shares the parser driver, the node-kind enum, the
token sets, the sink and the syntax-kind mapping.

**C. Ask upstream for permission.** Not considered anywhere in this directory
before. The rights holders in `bsl-parser` are identifiable and the projects are
not adversarial. An explicit grant, or a permissive dual-licence for the grammar
material, would resolve the question wholesale instead of piecemeal — including
the BSL layer, which option A does not currently cover. This is the cheapest
path to the stated end goal of a fully permissive binary, and it should be
attempted before item 7 is funded.

**D. Stay on LGPL-3.0-or-later.** The status quo is defensible and honest. It
costs nothing and misleads nobody.

## Non-action: do not rename to obscure

Since the surviving overlap is largely naming, renaming the grammar functions
would make the resemblance disappear. Do not do this as a licensing measure.
Renaming does not undo derivation, it destroys the remaining evidence of what
was derived, and it would sit in the history immediately after two commits that
already removed provenance markers. If rule names are changed, change them for
readability, say so in the commit message, and do not present the change as
provenance work.

## Supersession

On the question of *fact* — whether derivation occurred — this document
supersedes the estimates in the April 2026 audits; their conclusion was correct
and is now proven.

On the question of *current state* — how much upstream expression remains — this
document supersedes them outright. Their risk labels (`high`,
“strongly grammar-derived”) describe the code as it stood before Slices 1–11 and
overstate what is in the tree today. Those documents are retained as the
historical record of the assessment that motivated the clean-room programme.
