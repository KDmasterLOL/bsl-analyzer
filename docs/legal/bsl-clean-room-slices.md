# BSL Clean-Room Slices

## Purpose

This document is the plan for exit-criteria item 10 of
`sdbl-provenance-2026-07-audit.md`: the BSL grammar layer, the only item on that
checklist that has never had one.

Item 10 blocks Tier A for `parser` and `lexer` independently of how clean SDBL
becomes, because both crates host a BSL layer with the same origin. Finishing
the SDBL programme in full would not move either crate.

## Status as of 2026-08-20

Nothing is done. No slice has ever touched the BSL layer, and no provenance
marker of any kind survives in it — `843b00ab` and `3aa29b99` removed them all,
so the evidence lives only in git history.

| Slice | Area | Issue | State |
|---|---|---|---|
| B1 | BSL token inventory (`lexer`) | #233 | not started |
| B2 | Preprocessor symbols (`ide-diagnostics`) | #234 | not started |
| B3 | Grammar rules — attestation, not rewrite (`parser`) | #235 | not started |
| B4 | Test material | #236 | not started |

## Scope: half of what is already done

Measured 2026-08-20 on `develop`.

| What | Lines | Units |
|---|---:|---|
| BSL lexer, `crates/lexer/src/lib.rs` | 945 | 101 `TokenKind` variants |
| BSL grammar, `grammar.rs` + `grammar/{items,statements,expressions}.rs` | 1462 | 77 functions, of which 59 build tree nodes |
| Preprocessor symbols, `crates/ide-diagnostics/src/utils/preprocessor_symbols.rs` | 89 | 29 spellings |
| For comparison: SDBL, already attested | 4891 | |

`crates/parser/src/grammar/sdbl.rs` (381 lines) belongs to the SDBL programme
and is excluded from every count here.

The shared parser infrastructure — driver, events, sink, token sets, node kinds —
is not a risk: `parser-architecture-map.md` (lines 19–47) assesses it as local
work whose influence is rust-analyzer, which is permissively licensed.

The 77 grammar functions split on a mechanical criterion rather than on naming:
**18 take `&Parser` and return `bool`** — they only look ahead and build nothing.
Seventeen are the `at_*` family (sixteen distinct names; `at_then` is declared
twice, in `grammar.rs` and in `statements.rs`) and the eighteenth is
`continues_the_surrounding_expression`. The remaining **59 take `&mut Parser`**
and build tree nodes; they are B3's subject.

Two functions that return `bool` are *not* predicates and stay in the 59:
`postfix_expr_with_call_info` and `postfix_expression_for_assignment` take
`&mut Parser`, build nodes, and return a decision about what they built. Sorting
by name instead of by signature would have misfiled both.

## What the repository's own history proves, and what it does not

Established by a full review of every line ever added to the BSL part: **rule
citations in ANTLR notation were never present there**, unlike the SDBL parser,
where many grammar functions carried one, complete with ANTLR label syntax that
cannot be reconstructed from 1C documentation. Not every SDBL function was
annotated — `sdbl-provenance-2026-07-audit.md` (lines 64–70) is explicit that the
annotation shows how the file was written rather than mapping which rules were
transcribed. The asymmetry that matters is none-versus-many, and it splits the
work in two.

| Subject | Evidence | Consequence |
|---|---|---|
| Lexer token inventory | Header "Based on BSLLexer.g4 from bsl-parser project", added by `fe2f7ed2` (2025-12-29), removed by `843b00ab` (2026-03-06) | derived → **rewrite** |
| Preprocessor symbols | `eb504838` (2026-01-24) file header: "Ported from: BSLLexer.g4 (bsl-parser) - PREPROC_*_SYMBOL tokens", removed by `843b00ab` | derived, the strongest wording anywhere in the history → **rewrite** |
| Grammar intent | `ITERATIONS.md` of the initial commit `a6204f78` assigns named rules of a third-party grammar as the work items; our function names follow that plan | intent derived → **attest** |
| Grammar rule text | — | **not established** |

Not establishing derivation is not the same as establishing independence. B3
therefore produces a per-rule verdict rather than a blanket claim.

## Discrepancy retracted before the work starts

`parser-bsl-grammar-audit.md` named `../bsl-parser/src/main/antlr/BSLParser.g4`
as its comparison baseline, and its file-by-file verdicts read as the result of
that comparison. The comparison never happened: that note was written in April
2026 without access to the grammar files, as
`sdbl-provenance-2026-07-audit.md` (lines 12–18) states for the whole April set.
A plan resting on a verdict produced by a method that was never applied is not a
plan.

That note now carries a retraction banner and its baseline claim is withdrawn.
Its structural observations about **our own** files remain useful and are B3's
starting hypotheses; its risk ordering of the four files is not evidence and is
not carried forward.

There is no `bsl-parser` checkout on this machine, and none is to be obtained
for this work.

## Source: first-class, and present

Chapter 4 «Встроенный язык» of the 8.3.27 Developer's Guide is in the local ITS
crawl — 172k characters of text, the same presentation Chapter 8 has for SDBL:
per construct «Описание / Синтаксис / Англоязычный синтаксис / Параметры /
Пример». Public reference: `https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000116`.

| Part of the language | Section |
|---|---|
| Reserved words, bilingual table | 4.2.4.6 |
| Special characters | 4.2.5 |
| Types and literals (NULL, Булево, Дата, Неопределено, Строка, Тип, Число) | 4.3 |
| **Operator precedence and associativity** | 4.5.4 |
| Variable declaration, procedure, function | 4.6.1, 4.6.3, 4.6.4 |
| If / conditional-expression ternary | 4.6.5, 4.6.5.2 |
| Loops: Для / collection traversal / Пока | 4.6.6 |
| Goto, Выполнить, Ждать | 4.6.7, 4.6.8, 4.6.9 |
| Exceptions | 4.6.10 |
| ДобавитьОбработчик / УдалитьОбработчик | 4.6.11 |
| Parameter passing, Знач, defaults | 4.7.4 |
| **Preprocessor instructions: grammar plus the full symbol list** | 4.8.1.2 |
| Compilation directives | 4.8.1.3 |
| Annotations | 4.8.2 |

Section 4.5.4 gives the precedence table in **ascending** order with
left-associativity on ties. It contradicts `v8std/docs/lang/index.md` §3.5,
where the order is inverted: **v8std is not a normative source on syntax** —
`v8std-usage-policy.md` already forbids it, and this is a concrete instance of
why.

`shlang_ru.hbk` is absent from this machine (no installed platform). With
Chapter 4 available it drops from blocking condition to optional cross-check: it
would give per-article keyword forms, not a consolidated grammar.

`platform_data.json` plays no part at all: `keywords: 0`, zero syntactic
information.

## Legal framing

The working ownership note and the clean-room rule of
`sdbl-clean-room-slices.md` (sections «Working ownership note» and «Clean-room
rule») apply here unchanged, with BSL substituted for SDBL and Chapter 4
substituted for the query-language book. In short: the BSL **language** is 1C's;
a third party can hold rights only in its own concrete expression — grammar
text, token inventories, examples, tests, implementation code.

While writing replacement code for B1 or B2, do not consult the `bsl-parser`
grammar files, and do not read the «Comparison against the upstream grammars»
section of `sdbl-provenance-2026-07-audit.md`.

## B1 — BSL token inventory

**Subject.** 101 `TokenKind` variants: 40 keywords, 27 operators and
punctuation, 11 preprocessor directives, 10 annotations, 7 literals, 1
identifier, 5 service tokens.

**Method.** The direct analogue of lexer Slices 3b/4/5, audited in three passes:

1. forward — every regex in the lexer against the source;
2. reverse — every spelling the source defines, checked for a variant. In SDBL
   this pass produced **all** the findings;
3. execution — run the lexer, catching declarations that are byte-correct but
   unreachable, the way `FnDate` was.

**Work visible in advance.** Section 4.2.4.6 lists **30** bilingual pairs; we
have 40 `Kw*` variants. (The reconnaissance in issue #150 said 34 pairs; the
table has 30, and the ten-variant difference is listed below.) The ten are
attested from the sections that define them, not from 4.2.4.6:

- `Экспорт` — 4.6.3, 4.6.4;
- `Знач` — 4.7.4;
- `Асинх`, `Ждать` — 4.6.9;
- `Истина`, `Ложь`, `Неопределено`, `Null` — 4.3;
- `ДобавитьОбработчик`, `УдалитьОбработчик` — 4.6.11.

The last pair matters beyond bookkeeping: the source presents them as event
handler operators in 4.6.11 and deliberately keeps them out of the reserved-word
table. Whether they should be `Kw*` variants at all is a B1 question, and the
answer must come from 4.6.11 plus a corpus measurement, not from the current
code.

Section 4.2.5 enumerates the special characters and does **not** include `?`,
`&`, `#`, `{`, `}` or `!`. Being documented elsewhere is not the same as being
read: of these, only `Question` is consumed by a BSL rule
(`expressions.rs:247`, the ternary). `Hash` and `Ampersand` have their normative
description in 4.8.1.2 and 4.8.2, but the spellings that matter there are lexed
whole as `Pre*` and `Ann*` variants, so the bare tokens are what an *unknown*
directive or annotation leaves behind — and no rule reads them.

B1's open cases are therefore five: `Hash`, `Ampersand`, `Exclamation`,
`LBrace` and `RBrace`. None is consumed by a BSL rule today; the braces are read
only through the SDBL converter. Each needs a decision — read it, or stop
producing it — and J2 below is what makes the decision necessary rather than
optional.

**Expected outcome.** Additive and tree-invariant, as in Slices 4 and 5:
spellings that the grammar reads through `Ident` do not change shape when the
lexer learns their names.

## B2 — preprocessor symbols

**Subject.** `crates/ide-diagnostics/src/utils/preprocessor_symbols.rs`, 89
lines, 29 spellings: 13 bilingual pairs plus `LINUX`, `WINDOWS`, `MACOS`.

This is simultaneously the strongest evidence of borrowing in the whole BSL
layer and the cheapest repair: section 4.8.1.2 gives the complete bilingual
symbol list and the grammar of the instructions verbatim. The 13 pairs in the
file match 4.8.1.2 exactly.

**Two upstream channels, not one.** The reconnaissance named the file header.
The commit that introduced the file, `eb504838`, records a second: "Test fixture
copied verbatim from bsl-language-server" and "Diagnostics trigger at exact same
positions as Java implementation". That fixture,
`test_data/UnknownPreprocessorSymbolDiagnostic.bsl`, was not removed — `07d2b977`
migrated diagnostic tests from `.bsl` fixtures to inline code, and its 19 lines
live today, character for character, inside `test_comprehensive` in
`crates/ide-diagnostics/src/handlers/unknown_preprocessor_symbol.rs`. Deleting
the file moved the material rather than retiring it. Test material is a
provenance class of its own — the same class as exit-criteria item 9 — and B2
must replace this input, not merely re-derive the symbol list.

**`LINUX` / `WINDOWS` / `MACOS` are unattested.** Chapter 4 does not define
them; 4.8.1.2's list ends at `МобильныйАвтономныйСервер`. B2 must establish
where they come from and either attest them from a source or record them as a
deliberate compatibility allowance. Until that is settled, they are the one part
of this file with no known origin.

**Two crates is the wrong boundary.** The file lives outside `parser` and
`lexer`, so item 10 as worded on the checklist does not reach it, and neither
`NOTICE` nor `LICENSING.md` mentions it anywhere — the derivation notice in
`NOTICE` lists four paths and this is not one of them. `ide-diagnostics` is in
Tier B for an unrelated reason (17 diagnostics depend on the SDBL chain), so
nothing is mislicensed today; what is missing is the record. Fixing that
omission belongs to B2, not to the closing paperwork.

## B3 — grammar: attestation, not rewriting

**Subject.** All 77 functions of the four files, enumerated as a checklist:
the 59 tree-building rules against the language, and the 18 lookahead predicates
against the recovery architecture. The subject is whether each form is required
by the language, not whether the text is ours. No function is exempt from having
a verdict recorded; a function nobody assigned a verdict is the failure mode this
slice exists to prevent.

**Method.** Slice 12's method. Per rule, one verdict:

- **R** — required by the language, with the section cited;
- **A** — a deliberate allowance for IDE behaviour (recovery, editor buffers,
  partial input), with the reason stated;
- **D** — nobody chose this; it is here because it was here.

**A D verdict is a finding, not an outcome.** B3 does not close while any D
stands: each one must be resolved into R (a source was found), into A (the
allowance is chosen deliberately and its reason recorded), or into a change that
removes the form. Recording a rule as D and moving on would leave the BSL layer
with unattested forms while the closing paperwork declares it attested — the
exact state this programme exists to end.

Operator precedence is settled here from 4.5.4. A corpus measurement of
precedence is a positive control for that verdict, not a way of deriving it: a
measurement over committed code cannot show which of two parse orders the
language requires, only that both builds agree.

**Expected outcome**, based on how the `sdbl-hir` reconnaissance turned out: no
derived text will be found, and places where a form was chosen without a source
will be. The value of B3 is the D verdicts.

**Nothing in these four files is covered by `parser-architecture-map.md`.** That
document exempts Layer 1 — `event.rs`, `parser.rs`, `parser/input.rs`,
`sink.rs`, `parser/token_set.rs`, `syntax_kind.rs`, `lib.rs` — and puts all four
grammar files in Layer 2 at «high provenance risk», naming them as the rewrite
or proof target (the Layer 2 table at lines 50–59 and the «Likely needs dedicated
rewrite or proof» list at lines 140–148). The Scope section above cites that
document for the shared infrastructure only, and the citation does not extend
past Layer 1.

So the functions that look most obviously local — `assignment_or_call`,
`postfix_expr_with_call_info`, `stmt_list_inner`, the acceptance of keywords as
member names, the multiline-string handling — get an **A verdict with its reason
written down**, not an exemption. «It is clearly our own parser work» is the
expected conclusion for them; it still has to be reached one function at a time
and recorded, because that is the only thing distinguishing an attested rule
from one nobody looked at. The 18 lookahead predicates are attested the same way
and as a group: they encode recovery decisions, have no counterpart in a grammar
file, and each names the construct it looks for.

## B4 — test material

`crates/parser/tests/fixtures/Module.bsl`, 15 157 lines with no terminating
newline (`wc -l` reports 15 156 for that reason), is a verbatim copy of a
`bsl-parser` test resource (`16c5db20`, 2025-12-29). The content is clean: the
file carries an ООО «1С-Софт» header under CC BY 4.0, so it belongs to 1C and is
free to use with attribution; `bsl-parser` was only the delivery channel.

There is nothing to rewrite, and the attribution is **already in place**:
`NOTICE` (lines 159–163) names the copyright holder, CC BY 4.0, the intact
licence header, and the acquisition channel through `bsl-parser`; `LICENSING.md`
names the file and its licence in its third-party table. What `843b00ab` removed
was six in-code comments of the form
`Source: bsl-parser/src/test/resources/Module.bsl`, and the `NOTICE` record
postdates them.

B4 is therefore a verification, not a repair: confirm that the licence header in
the file is intact and unmodified, that the `NOTICE` statement matches it, and
that no other BSL test input carries an origin nobody recorded.

B4 also covers the fixture material B2 identifies, and any other BSL test input
whose origin the same review finds.

## Invariants, written before the code, in runnable form

- **J1** — every `TokenKind` variant is produced by at least one input. Catches
  unreachable declarations, as `FnDate` was in SDBL.
- **J2** — every significant token is **consumed by a rule**, not merely
  present in the tree. This is the class Slice 12 found in SDBL, and it has
  never been checked for BSL. Two ways of stating it produce a gate that cannot
  fail, and J2 must avoid both:

  - *Text coverage.* `Sink::finish` calls `take_the_tail()` before closing the
    root (`crates/parser/src/sink.rs:169,235`), sweeping every remaining raw
    token into the tree as pending trivia. An input whose `!` no rule ever reads
    still yields a tree whose text equals the input exactly.
  - *Mere parentage.* `Parser::emit_error` (`crates/parser/src/parser.rs:460`)
    opens a node, bumps the current token whatever it is, and closes it as
    `NodeKind::Error`. Every unreadable token therefore has a parent.

  So J2 is stated as: **a token consumed only under `NodeKind::Error` was not
  consumed by a rule.** `TokenKind::Error` is the single exemption, by
  construction rather than by exception — it exists to carry text no rule can
  name, and `NodeKind::Error` is its only lawful parent. Without that exemption
  J1 and J2 would contradict each other on exactly one variant. Every other kind
  that reaches the tree only through the recovery path across the whole corpus
  is a J2 finding, which is what decides B1's five open cases.
- **J3** — for every pair of operators there is an input that distinguishes the
  two parse orders, and the parse agrees with the table in 4.5.4. This is the
  positive control for precedence.
- **J4** — parsing is idempotent and independent of traversal order.
- **J5** — over the corpus, the classification of files (parsed / parsed with
  errors / not parsed) does not change between builds. A zero-difference result
  counts only when the same run reports a **named control input** together with
  the two classifications the two builds are expected to give it, and observes
  exactly that difference. Without the control, zero differences and a broken
  comparison — both builds reading the same artefact, the harness comparing a
  file with itself — are indistinguishable, and each slice's attestation must
  print the control it used.

J5 reuses the Slice 12 harness. Its gotcha carries over verbatim: multiline
literals lex as `StringStart` / `StringPart` / `StringTail`, so a harness that
collects only `String` sees a small fraction of the real data. The symptom is a
literal counter that does not agree with the expected order of magnitude.

J1 and J2 are the pair that makes B1's open cases decidable: a variant that no
input produces fails J1, and a token that no rule consumes shows up in J2.

## Compatibility: to be recorded as a decision

`a6f57021` (2026-06-15) is the BSL counterpart of `c7c942a7`: it clears "442
ParseErrors across 34 files that bsl-language-server 0.29.0 parses cleanly". The
correctness oracle for BSL parsing, after the markers were removed, was
somebody else's implementation.

As with the diagnostic catalogue, this is lawful and is the point — compatibility
with the predecessor is a goal. But it has to be **recorded as a decision**,
otherwise it reads as underivability. The line is the same one: our own code
behind the same observable interface is fine; obtaining that interface by
reading their source is not.

For this particular commit the record can be stronger than "compatibility".
Section 4.2.4.6 reserves keywords against use as «имена переменных, реквизитов
объектов конфигурации и объявляемых процедур и функций» — declaration sites. It
says nothing about member names after a dot, so `Выборка.Исключение` is valid by
the source, and the rule the commit implements is derivable from 4.2.4.6
directly. The other implementation served as a corpus-scale cross-check, not as
the origin of the rule. Establishing that for the remaining compatibility-driven
BSL commits is part of this step; where it cannot be established, the plain
compatibility record stands.

## Order

1. Retract the discrepancy in `parser-bsl-grammar-audit.md`.
2. B1.
3. B2.
4. J-invariants and harness.
5. B3.
6. B4.
7. Record the compatibility decision.
8. Update `NOTICE` and `LICENSING.md`.

Step 1 is done; the rest are separate slices with their own attestations, on the
model of `sdbl-clean-room-slice*.md`.

The invariants sit after B2 rather than first because J1 and J2 are cheap to
state and expensive to satisfy: B1 and B2 change the token inventory, and a
harness built before them would be rebuilt after them.

## What item 10 will not deliver

Item 10 does not by itself put the binary under MIT. Three checklist items
remain open: item 7 (SDBL parser rule naming), item 8 (Slice 13 — `sdbl-hir`
compared against `bsl-language-server`'s lowering, not only against the
grammar), and item 9 (the test corpus). Item 8 is the largest unknown of the
three and concerns a different crate from `hir-def`, which the checklist does
not mention at all and which was assessed separately in issue #151.

Item 10 does move `parser` and `lexer` to the point where the only thing between
them and Tier A is the remaining SDBL work, which is the state neither crate has
ever been in.
