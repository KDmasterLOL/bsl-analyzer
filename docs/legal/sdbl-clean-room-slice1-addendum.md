# SDBL Slice 1-addendum — Clean-Room Attestation (query-extension brace pair)

**Status:** complete (2026-07-27).

This document attests the clean-room authorship of the Slice 1-addendum
material of the SDBL lexer, per the staged migration plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

Slice 1-addendum claims the two brace tokens `LBrace` and `RBrace`. It
exists because Slice 3b, while restoring the provenance map, found them
covered by no attestation and owned by no slice, and recommended a
separate small slice rather than folding punctuation into a vocabulary
slice — see [`sdbl-clean-room-slice3b.md`](sdbl-clean-room-slice3b.md)
§ Unowned brace tokens.

With this slice the lexer's token inventory is completely attested:
Slice 3b's partition of every `SdblTokenKind` variant against the closed
attestations left exactly these two unaccounted for, and they are now
accounted for.

## Status

The Slice 1-addendum attestation flipped from "in progress" to
"complete" at phase C3, with the absolute-date stamp at the top of this document.
The C0a / C0b / C1 / C2 / C3 landings are atomic; the absolute-last
trailing commit on the branch (the Anti-Hilbert disclosure) is
necessarily not named in the enumeration, mirroring every closed slice
in this programme.

## Scope

The paths claimed as clean-room Slice 1-addendum authorship are:

- `crates/lexer/src/sdbl/mod.rs` — specifically:
  - the file-level docstring bullet added for Slice 1-addendum;
  - the `LBrace` and `RBrace` variants declared under the `CLEAN-ROOM
    Slice 1-addendum — query-extension brace pair` banner, with their
    per-variant provenance docstrings;
  - the retirement of the `LEGACY (unowned)` note Slice 3b left over
    the pair.
- `crates/parser/src/grammar/sdbl/select.rs` — the `eat_query_extensions`
  doc comment, corrected to distinguish the extension elements the
  documentation defines from those that only occur in practice. See
  § The undocumented ordering element. This is the one non-lexer file
  the slice touches, it is a comment, and it carries no grammar
  decision.
- `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt` — the first
  corpus entries either brace has ever had.
- `crates/lexer/tests/sdbl_golden_corpus.rs` — the snapshot regenerated
  at C0b.
- `crates/lexer/tests/sdbl_slice1_addendum_braces.rs` — the spec-driven
  acceptance test file born at C3, 14 tests: 3 on the tokens existing at
  all and carrying their own spans, 4 on the documented elements,
  2 pinning the refusal of the extension keywords, 3 on the shapes the
  documentation does not describe, and 2 structural.

### The 2 claimed variants

`LBrace` and `RBrace`. Both pre-exist; neither changes.

### Out of scope

- The nine `ХАРАКТЕРИСТИКИ` keywords the extension defines. They are a
  documented part of the same source and they have no lexer variant,
  which in Slices 3b, 4 and 5 would have made them an Option A ADD.
  Here the measurement points the other way; see § The nine extension
  keywords are deliberately not added.
- The parser-side treatment of `{…}` regions — which positions accept
  an extension, how brace depth interacts with recovery, and the
  decision to tolerate an unbalanced `{`. That behaviour landed with
  `537527eb` and belongs to master-doc Slice 12, which owns recovery
  and IDE allowances. Slice 1-addendum touches one doc comment in that
  file and nothing else.
- Every vocabulary family — Slices 2, 2-addendum, 3a, 3b, 4 and 5.

## Per-variant tier source map

Both variants are classified **Tier A1** with the 1C:Enterprise 8.3.27
syntax assistant article «Расширение языка запросов для системы
компоновки данных» as the primary canonical source. The article opens by
defining exactly what the braces are:

> Расширение языка запросов для системы компоновки данных
> осуществляется при помощи специальных синтаксических инструкций,
> **заключаемых в фигурные скобки** и помещаемых непосредственно в текст
> запроса.

and its English build says the same:

> The query language extension for the data composition system adds
> syntax instructions that must be **enclosed in braces** and added
> directly to the query text.

| Variant | Spelling | Canonical attestation |
|---|---|---|
| `LBrace` | `{` | opens a data-composition query-extension instruction |
| `RBrace` | `}` | closes it |

There is no bilingual pair to reconcile and no spelling to get wrong:
each token is a single ASCII character, declared with `#[token("{")]`
and `#[token("}")]`. What this slice attests is therefore not a
spelling but a **contract** — that the two characters carry a documented
meaning in a query text, that the meaning belongs to an extension of the
query language rather than to the language itself, and that the lexer
must emit them as tokens rather than as `Error`.

The second half of that contract is what makes the pair necessary. The
base query language has no braces: Developer's Reference Глава 8 «Работа
с запросами» (<https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453>)
describes the whole language without one. A tool that reads only Глава 8
would be right to reject `{`, and the SDBL lexer did exactly that until
`537527eb` — which is why the pair exists at all, and why its source is
the data-composition documentation rather than the query-language
documentation.

### What the extension defines

The article gives three named elements plus a parameter form:

| Element | RU | EN | Meaning |
|---|---|---|---|
| selection | `{ВЫБРАТЬ …}` | `{SELECT …}` | field aliases the user may choose to display |
| filter | `{ГДЕ …}` | `{WHERE …}` | table fields the user may filter on |
| characteristics | `{ХАРАКТЕРИСТИКИ …}` | `{CHARACTERISTICS …}` | where to find characteristic types and values for a type |
| virtual-table parameter | `{&Параметр}`, `{Поле.*}` | same | parameters and fields exposed through a virtual table's argument list |

Inside these, `.*` after a field alias marks the field's child fields as
available. Both characters are already tokens — `Dot` and `Star` from
Slice 1 — and the combination needs nothing new.

## C0a discrepancy audit

### Audit method

Unlike the vocabulary slices there is no alternation text to compare, so
the audit is:

1. Read the two `#[token(...)]` attribute bodies verbatim.
2. Confirm against the canonical article that the braces are the
   extension's delimiters and that no other delimiter is used.
3. In the opposite direction: enumerate everything the extension's
   syntax needs and check that the lexer can produce it.
4. Tokenise every documented form and record what the lexer emits.

### Audit results — declarations (2/2 MATCH)

`#[token("{")] LBrace` and `#[token("}")] RBrace`. The article's
delimiters are `{` and `}`; the declarations are those two characters
and nothing else. **MATCH.**

### Audit results — what the extension's syntax needs

Every lexical element the documented forms require already has a token:

| Needed by | Element | Token | Owner |
|---|---|---|---|
| all forms | `{`, `}` | `LBrace`, `RBrace` | this slice |
| `{ВЫБРАТЬ …}` | `ВЫБРАТЬ (SELECT)` | `KwSelect` | Slice 2 |
| `{ГДЕ …}` | `ГДЕ (WHERE)` | `KwWhere` | Slice 2 |
| `{ХАРАКТЕРИСТИКИ …}` | `ТИП (TYPE)` | `KwType` | Slice 2-addendum |
| child-field marker | `.` and `*` | `Dot`, `Star` | Slice 1 |
| parameter form | `&Параметр` | `Parameter` | Slice 1 |
| separators | `,` `(` `)` | `Comma`, `LParen`, `RParen` | Slice 1 |

No punctuation is missing. The `|` and `[…]` that appear in the
article's syntax description are notation for alternation and
optionality, not query text.

### The nine extension keywords are deliberately not added

Nine words of the `ХАРАКТЕРИСТИКИ` construct are defined by the article
and have no lexer variant:

| RU | EN | Lexes as |
|---|---|---|
| `ХАРАКТЕРИСТИКИ` | `CHARACTERISTICS` | `Ident` |
| `ВИДЫХАРАКТЕРИСТИК` | `CHARACTERISTICTYPES` | `Ident` |
| `ПОЛЕКЛЮЧА` | `KEYFIELD` | `Ident` |
| `ПОЛЕИМЕНИ` | `NAMEFIELD` | `Ident` |
| `ПОЛЕТИПАЗНАЧЕНИЯ` | `VALUETYPEFIELD` | `Ident` |
| `ЗНАЧЕНИЯХАРАКТЕРИСТИК` | `CHARACTERISTICVALUES` | `Ident` |
| `ПОЛЕОБЪЕКТА` | `OBJECTFIELD` | `Ident` |
| `ПОЛЕВИДА` | `TYPEFIELD` | `Ident` |
| `ПОЛЕЗНАЧЕНИЯ` | `VALUEFIELD` | `Ident` |

In Slices 3b, 4 and 5 a gap of this shape was an Option A ADD. Here the
answer is the opposite (repository owner's decision, 2026-07-27), and
the reasons are specific rather than a change of policy.

**The distinction is erased before the tree, and not by the brace
region.** `query_extension` in
`crates/parser/src/grammar/sdbl/select.rs` bumps every token of a
brace-balanced region into an `SDBL_QUERY_EXTENSION` node, tracking only
depth. That does not hide the tokens: `p.bump()` emits an
`Event::Token`, so they become children of the node and keep their
kinds. Parsing `ВЫБРАТЬ 1 {ГДЕ A И B}` puts `L_BRACE`, `KW_AND` and
`R_BRACE` inside the extension node, verbatim.

What erases keyword identity is one step earlier. In
`crates/parser/src/sdbl_token_converter.rs` every `Kw*`, `Fn*`, `Mdo*`
and `Vt*` variant maps to `TokenKind::Ident`, so `ГДЕ` reaches the tree
as `IDENT` whatever the lexer called it — while `И`, which the converter
maps to `TokenKind::KwAnd`, survives as `KW_AND`. A `CHARACTERISTICS`
variant would therefore either follow the convention every keyword
family already follows and be invisible to the tree, or break it and
require parser-side grammar — which belongs to a slice that interprets
the region rather than skips it, not to this one.

**The lexer cannot see the context.** These words are keywords only
inside braces, and the lexer has no state — it does not know whether a
brace is open. A variant would therefore fire on every occurrence
anywhere in a query.

**And they occur overwhelmingly outside braces.** Measured over a real
1C configuration used as this repository's extension testbed:

| Spelling | As an ordinary identifier | Inside a `{…}` extension |
|---|---|---|
| `Характеристики` | 2696, of which 590 immediately after a dot | 195 |
| `ПолеКлюча` | 160 | 195 |
| `ПолеЗначения` | 68 | 195 |

`Характеристики` is a tabular-section and attribute name across the
configuration; relabelling 2696 occurrences to serve 195 would assert a
classification the lexer has no evidence for. That is the same defect
class this programme has just spent two slices removing — `FnDate`,
which was emitted for no input, and `База<Имя>`, which is never a token
— except that this one would be reachable and wrong rather than
unreachable and useless.

The correct home for the `ХАРАКТЕРИСТИКИ` grammar is a parser-side slice
that interprets the region instead of skipping it. Until such a slice
exists, the honest lexer position is that the words are identifiers.

### The undocumented ordering element

`eat_query_extensions` documents the braces as marking
`{ГДЕ …}`, `{ВЫБРАТЬ …}` and `{УПОРЯДОЧИТЬ ПО …}`. The canonical article
defines the first two and `{ХАРАКТЕРИСТИКИ …}`; it does not mention a
braced ordering element, and neither does the query-language book, Глава
8, or any of the 162 ITS query-language textbook chapters.

The construct is nevertheless real. Measured over the same
configuration:

| Braced element | Occurrences |
|---|---|
| `{ГДЕ` | 917 |
| `{ВЫБРАТЬ` | 522 |
| `{ХАРАКТЕРИСТИКИ` | 195 |
| `{УПОРЯДОЧИТЬ` | 4 |

So the comment is not wrong about the world; it is imprecise about the
source, presenting as an element of the extension something the
documentation does not attest, and omitting the one element that
outranks it in both documentation and frequency. C2 corrects it to name
the three documented elements and to mark the ordering form as observed
rather than defined.

Note that `УПОРЯДОЧИТЬ ПО` itself is an ordinary clause of the query
language, fully attested by Slice 2 as `KwOrder` and `KwOnOrBy`. What is
undocumented is only its braced form as an extension element.

None of this changes behaviour: an opaque brace-balanced region tolerates
any element, documented or not, which is precisely the property that
makes the tolerance correct.

## Behaviour change

**None.**

This is the only slice in the programme with no behaviour change at all.
No regex is added, removed or edited; no variant is added or removed;
the converter is untouched. `LBrace` and `RBrace` emit exactly what they
emitted before, and `SdblTokenKind` keeps its 181 variants.

The corpus grows, and the snapshot with it, but every added line records
behaviour that already existed and was simply never pinned. The C0b
snapshot diff is therefore additive-only: no existing line changes.

That absence is itself worth stating. The slice's value is not a fix but
a record: two tokens that no attestation named now have one, and two
tokens with no coverage of their own now have a suite that names them.

Not "no coverage at all" — the distinction matters for the record.
`537527eb` shipped four parser tests that drive both braces end to end
(`test_query_extension_where_block_no_errors`,
`…_produces_extension_node`, `…_nested_braces_no_errors` and
`…_unbalanced_brace_is_tolerated`), so the tokens were exercised. What
they assert is parser outcomes — no errors, an extension node, tolerance
— never a token kind, and no attestation named the tokens at all.

## Marker restoration

Slice 3b placed a `LEGACY (unowned)` note over the brace pair so that a
reader of the file would see the gap instead of inferring from silence
that the lexer was done. That note has served its purpose and C1
replaces it with a `CLEAN-ROOM Slice 1-addendum` banner.

After this slice `crates/lexer/src/sdbl/mod.rs` contains no `LEGACY`
marker of any kind. The module docstring is updated to say so, and to
keep saying that the banners of the closed Slices 1, 2, 2-addendum and
3a are deliberately absent, so that a missing banner is still not read
as a missing attestation.

## Sources consulted

The Slice 1-addendum material was re-derived from:

1. **1C:Enterprise 8.3.27 syntax assistant**, data-composition help
   book, article «Расширение языка запросов для системы компоновки
   данных» (ships with the platform as `dcsui_ru.hbk`, with the English
   build as `dcsui_root.hbk`; the same class of artefact the repository
   already uses as the provenance base for
   `crates/bsl-platform/data/platform_data.json`, see
   `crates/bsl-platform/data/PROVENANCE.md`). This is the primary
   source and the only one that defines the braces at all.
2. **v8.3.27 Developer's Reference, Глава 8 «Работа с запросами»** —
   <https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453> — consulted
   for the negative attestation that the base query language contains no
   braces, which is what makes the pair an extension rather than core
   syntax.
3. **1C ITS pubqlang textbook** —
   <https://its.1c.ru/db/pubqlang/content/7/hdoc> and the other 161
   chapters, likewise consulted for the negative: the textbook describes
   the query language without the brace extension.
4. A production 1C configuration, kept locally as this project's
   extension testbed and not part of this repository, used only to count
   occurrences for the frequency tables in § The nine extension keywords
   are deliberately not added and § The undocumented ordering element.
   Nothing was read out of it and no text was taken from it; the
   measurements are counts of spellings this attestation had already
   derived from the documentation.
5. The repository's own history — commit `537527eb`, which introduced
   the tokens — read once to establish when and why they appeared, per
   the provenance method of `sdbl-provenance-2026-07-audit.md`
   § Finding 1.

Per the citation policy adopted in Slice 8-addendum and reaffirmed in
every slice since, committed artefacts cite only public ITS URLs and
named document sections; local mirror paths are working convenience only
and appear nowhere in this document, in source provenance comments, or
in commit messages. The syntax assistant has no public per-article URL,
so it is cited by product, version, book and article title.

## Non-consultation statement

During the authorship of the Slice 1-addendum material the following
sources were not used as working text:

- the sibling `bsl-parser` project — neither its grammar files nor its
  token inventory were consulted;
- `bsl-language-server` — neither its source tree nor its diagnostics;
- the "Comparison against the upstream grammars" section of
  `sdbl-provenance-2026-07-audit.md`, which is under read-quarantine for
  clean-room authors by that document's own read-order warning;
- any other third-party SDBL grammar, token inventory, or parser.

One point deserves an explicit note rather than silence. The commit
message of `537527eb`, which introduced the two tokens, reports
`bsl-language-server` finding counts on an ERP configuration as evidence
that the new tolerance produced no false positives. That is a comparison
of two tools' **output on a shared corpus**, not a reading of the other
tool's source, and it is the same kind of black-box comparison the
repository uses throughout its false-positive campaign. It is recorded
here so that a later reader who finds the reference in the history does
not have to guess what kind of comparison it was.

## Verification recipe

All of the following must be green before this attestation is considered
live. Pre-slice baseline test counts (post-Slice-5, `develop` as of
2026-07-27) are pinned to detect silent test regressions:

1. `cargo test -p lexer --lib` — **70 passed** pre-slice.
2. `cargo test -p lexer --test sdbl_slice1_core` — **34 passed**
   pre-slice; expected unchanged. Slice 1 owns the surrounding
   punctuation run and must not move.
3. `cargo test -p lexer --test sdbl_slice2_keywords` — **43 passed**
   pre-slice; expected unchanged.
4. `cargo test -p lexer --test sdbl_slice2_addendum_clause_keywords` —
   **30 passed** pre-slice; expected unchanged.
5. `cargo test -p lexer --test sdbl_slice3a_types` — **25 passed**
   pre-slice; expected unchanged.
6. `cargo test -p lexer --test sdbl_slice3b_metadata_objects` —
   **49 passed** pre-slice; expected unchanged.
7. `cargo test -p lexer --test sdbl_slice4_functions` — **25 passed**
   pre-slice; expected unchanged.
8. `cargo test -p lexer --test sdbl_slice5_virtual_tables` —
   **23 passed** pre-slice; expected unchanged.
9. `cargo test -p lexer --test sdbl_golden_corpus` — **1 passed**
   (single snapshot test) throughout; the snapshot grows at C0b and does
   not change again.
10. `cargo test -p lexer --test sdbl_slice1_addendum_braces` — file does
    **not exist** pre-slice; **14 passed** post-slice.
11. `cargo test -p lexer --tests` — **300 passed** pre-slice,
    **314 passed** post-slice (300 + 14, matching item 10).
12. `cargo test -p parser` — **596 passed** before and after. The slice
    changes one doc comment in the parser crate and nothing else.
13. `cargo build --workspace --all-targets`.
14. `cargo clippy -p lexer -p parser --all-targets --all-features
    -- -D warnings`.

## Pre-C0b corpus coverage audit

Neither brace has ever appeared in
`crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`. Slice 3b recorded
this while enumerating the pair's state, and it is still true: the
characters `{` and `}` occur in no entry, so the corpus pins nothing
about them.

| Construct | In corpus? |
|---|---|
| `{ВЫБРАТЬ …}` selection element | ❌ MISSING |
| `{ГДЕ …}` filter element | ❌ MISSING |
| `{ХАРАКТЕРИСТИКИ …}` characteristics element | ❌ MISSING |
| `{&Параметр}` in virtual-table arguments | ❌ MISSING |
| child-field marker `Поле.*` inside a block | ❌ MISSING |
| nested braces | ❌ MISSING |
| empty `{}` | ❌ MISSING |
| unbalanced `{` | ❌ MISSING |
| braces inside a string literal | ❌ MISSING |
| English `{SELECT …}` / `{WHERE …}` | ❌ MISSING |

Ten blind spots in the corpus, which is every observable property of the
pair that a token-level test can pin. They are closed at C0b. The
parser-level behaviour — balanced, nested and unbalanced regions — was
already covered by the four tests `537527eb` shipped; what was missing
was coverage that names the tokens. Because the slice changes no behaviour, the entries
are written once and their snapshot lines never move again — unlike the
vocabulary slices, where the C0b snapshot was a before-picture.

### C0b outcome

Nine thematic entries landed, numbered 111–119, closing all ten blind
spots. The snapshot diff is **208 insertions and 0 deletions**: not one
existing line changed, which is the mechanical form of § Behaviour
change.

- **111** extension selection element russian — `{ВЫБРАТЬ Номенклатура,
  Склад}` after a complete query.
- **112** extension filter element with child field marker — both
  documented `{ГДЕ …}` forms, the alias list with `.*` and the
  parameterised comparison.
- **113** extension characteristics element — `{ХАРАКТЕРИСТИКИ
  ТИП(…) ПОЛЕКЛЮЧА … ПОЛЕИМЕНИ …}`, which renders `ХАРАКТЕРИСТИКИ`,
  `ПОЛЕКЛЮЧА` and `ПОЛЕИМЕНИ` as `Ident` and so pins the refusal
  recorded in § The nine extension keywords are deliberately not added.
- **114** extension in virtual table arguments — the article's own
  example shape, `Обороты({&ДатаНачала}, {&ДатаКонца}, ,
  {Номенклатура.*, Склад.*})`, including the empty third argument.
- **115** empty and nested braces — `{}`, `{{}}` and a block nested
  inside a `{ГДЕ …}`.
- **116** unbalanced opening brace at eof, and **117** unbalanced
  closing brace. Both tokenise cleanly; neither produces `Error`. The
  tolerance those entries pin is the parser's, but it rests on the lexer
  emitting brace tokens at all.
- **118** braces inside a string literal — `"{ВЫБРАТЬ Поле}"` stays a
  single `String` payload, so the extension delimiters do not leak into
  quoted text.
- **119** extension elements english — `{SELECT Products, Warehouse}`
  and `{WHERE Products.*}`, the article's English examples.

Two incidental readings in these entries are pre-existing shared
spellings owned elsewhere and are pinned as they are: `Ссылка` renders
`KwRefs`, the `ССЫЛКА (REFS)` operator of Slice 2, and `Дата` renders
`TypeDate` from Slice 3a.

## Commit trail

- `5475fe6d` (2026-07-27) — C0a: this attestation document authored.
  Sole change: addition of
  `docs/legal/sdbl-clean-room-slice1-addendum.md`.
- `6715b5b3` (2026-07-27) — C0b: corpus entries 111–119 added to
  `crates/lexer/tests/fixtures/sdbl_golden_corpus.txt`; snapshot
  regenerated via `UPDATE_EXPECT=1`. See § C0b outcome.
- `ca597ec7` (2026-07-27) — C1: module docstring and the `CLEAN-ROOM
  Slice 1-addendum` banner replacing the `LEGACY (unowned)` note.
  Comments only.
- `6a485082` (2026-07-27) — C2: per-variant provenance docstrings on the
  two tokens, and the correction of the `eat_query_extensions` doc
  comment. Comments only in both crates: 20 inserted lines in the lexer,
  one rewritten doc comment in the parser, no declaration or token
  touched, so the golden corpus snapshot needed no regeneration and
  `cargo test -p parser` stayed at 596.
- `1d9c75e6` (2026-07-27) — C3: the `sdbl_slice1_addendum_braces.rs`
  acceptance suite (14 tests), this attestation's flip to complete, and
  the master-document Slice 1-addendum section.
- `e8e6b23d` (2026-07-27) — review correction: the account of where
  token kinds are erased, and the overstated claim that the braces had
  no test coverage. See § The nine extension keywords are deliberately
  not added and § Behaviour change; both were reproduced before being
  rewritten.
- Close-out: a single trailing commit strikes item 5 from the exit
  criteria in `sdbl-provenance-2026-07-audit.md`, closing the lexer side
  of that checklist, and pins the C3 SHA explicitly in the trail above.
  The exit-criteria edit is deliberately last: that document's
  § Finding 2 is under read-quarantine for clean-room authors, and
  touching the file surfaces its contents. The Anti-Hilbert disclosure:
  this trailing commit's own SHA is NOT named in this enumeration — it
  cannot be, by construction.

## Licensing note

This slice completes the lexer-side exit criteria in
[`sdbl-provenance-2026-07-audit.md`](sdbl-provenance-2026-07-audit.md)
§ Exit criteria: with item 5 struck, all five lexer items are done and
every `SdblTokenKind` variant is covered by an attestation.

That does **not** promote `crates/lexer` to Tier A
(`MIT OR Apache-2.0`). The crate is shared with the BSL grammar layer,
which has never had a slice plan, and the parser-side and test-corpus
criteria — Slice 12, Slice 13, rule naming, and bucket C of the SDBL
test corpus — remain open. The lexer's SDBL token inventory being clean
is a necessary step, not a sufficient one, and the exit-criteria list
says so.

## Author attestation

The Slice 1-addendum material listed above under **Scope** was authored
as a clean-room re-derivation from the sources listed under **Sources
consulted**, without using the `bsl-parser` project, the
`bsl-language-server` project, or any other third-party SDBL grammar as
working text. This attestation applies at the date recorded at the top
of the document.
