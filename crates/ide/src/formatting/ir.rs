//! Token-level IR for the BSL formatter (Phase 2 architecture).
//!
//! Builds a flat stream of opaque [`Atom`]s separated by [`Gap`]s from a
//! Rowan CST. The pipeline is:
//!
//! ```text
//!   Ir::build(root)                                  — CST traversal → IR
//!   apply_policy(ir, cfg, initial_indent)            — per-gap GapDecision
//!   render_with_line_ending(ir, decisions, cfg, le)  — final String
//! ```
//!
//! Invariants enforced by the builder:
//!
//!   * `gaps.len() == atoms.len() + 1` — boundary gaps frame the stream so
//!     the leading BOM-only prefix and the trailing newline are first-class.
//!   * Multi-line / concatenated string literals (Rowan `LITERAL` nodes
//!     containing `STRING_*` tokens) coalesce into a **single** [`Atom`];
//!     their internal whitespace is preserved by construction.
//!   * Comments are emitted as standalone atoms — their same-line-ness is
//!     a property of the surrounding gap, not the atom itself.
//!   * Only the policy layer reads [`SyntaxKind`] to make spacing
//!     decisions; the builder is policy-free.

use syntax::{NodeOrToken, SyntaxKind, SyntaxNode, TextRange, TextSize, WalkEvent};

use super::FormattingConfig;

/// An opaque slice of source. The formatter emits `text` byte-for-byte.
#[derive(Debug, Clone)]
pub struct Atom {
    pub kind: SyntaxKind,
    pub text: String,
}

/// The whitespace between two atoms (or framing the stream at the edges).
/// Comments are NOT gaps — they are their own atoms. `range` is the source
/// span the gap occupies; per-gap edits replace `source[range]` with the
/// rendered text when policy reshapes the gap.
#[derive(Debug, Clone)]
pub struct Gap {
    pub range: TextRange,
    pub text: String,
}

/// Decision the policy layer makes for each gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapDecision {
    /// Emit `gap.text` byte-for-byte. The conservative-formatter default.
    Preserve,
    /// Emit no whitespace at all.
    None,
    /// Emit a single ASCII space.
    OneSpace,
    /// Emit `newlines` `\n`-separators. Intermediate `\n`-separators get
    /// `body_level` indent (the level where blank lines visually sit);
    /// the final `\n`-separator gets `final_level` indent (the depth of
    /// the next atom, adjusted for block-boundary keywords). When
    /// `body_level == final_level` the gap is plain re-indent; differing
    /// values arise around block-end / middle keywords (e.g. blank line
    /// before `КонецПроцедуры` sits at body depth, `КонецПроцедуры`
    /// itself sits at the outer depth).
    NewlineWithIndent { newlines: u32, body_level: u32, final_level: u32 },
}

#[derive(Debug, Clone, Default)]
pub struct Ir {
    pub atoms: Vec<Atom>,
    pub gaps: Vec<Gap>,
    /// One CST node per atom: the token's parent for token atoms, the
    /// LITERAL node itself for coalesced atoms. Used by `apply_policy` to
    /// query ancestry (statement boundaries, block depth) without
    /// embedding policy-relevant fields in `Atom` itself.
    pub atom_nodes: Vec<SyntaxNode>,
}

impl Ir {
    /// Builds the IR from a Rowan CST root. See module-level invariants.
    pub fn build(root: &SyntaxNode) -> Self {
        let mut atoms: Vec<Atom> = Vec::new();
        let mut gaps: Vec<Gap> = Vec::new();
        let mut atom_nodes: Vec<SyntaxNode> = Vec::new();

        // Pending whitespace accumulator between two atoms.
        let mut pending_text = String::new();
        // End of the previous atom (or start of file for the first gap).
        // The next gap's source range is `prev_atom_end..next_atom_start`.
        let mut prev_atom_end = TextSize::from(0);
        let mut coalesce_until: Option<SyntaxNode> = None;

        let flush_gap =
            |gaps: &mut Vec<Gap>, text: &mut String, gap_end: TextSize, gap_start: TextSize| {
                gaps.push(Gap {
                    range: TextRange::new(gap_start, gap_end),
                    text: std::mem::take(text),
                });
            };

        for event in root.preorder_with_tokens() {
            match event {
                WalkEvent::Enter(NodeOrToken::Node(node)) => {
                    if coalesce_until.is_some() {
                        continue;
                    }
                    if is_coalescing_literal(&node) {
                        let node_range = node.text_range();
                        flush_gap(&mut gaps, &mut pending_text, node_range.start(), prev_atom_end);
                        atoms.push(Atom { kind: node.kind(), text: node.text().to_string() });
                        atom_nodes.push(node.clone());
                        prev_atom_end = node_range.end();
                        coalesce_until = Some(node.clone());
                    }
                }
                WalkEvent::Leave(NodeOrToken::Node(node)) => {
                    if coalesce_until.as_ref() == Some(&node) {
                        coalesce_until = None;
                    }
                }
                WalkEvent::Enter(NodeOrToken::Token(token)) => {
                    if coalesce_until.is_some() {
                        continue;
                    }
                    let kind = token.kind();
                    let tok_range = token.text_range();

                    if matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE) {
                        pending_text.push_str(token.text());
                    } else {
                        flush_gap(&mut gaps, &mut pending_text, tok_range.start(), prev_atom_end);
                        // The lexer's `//[^\n]*` regex greedily eats `\r`
                        // before `\r\n`, so COMMENT tokens in CRLF files
                        // carry a trailing `\r`. Strip it from the atom
                        // (the `\r` is line-ending whitespace, not comment
                        // content) but re-inject it into the *next* gap so
                        // the line ending survives byte-for-byte.
                        let (text, stripped_suffix) = if kind == SyntaxKind::COMMENT {
                            let raw = token.text();
                            let trimmed = raw.trim_end_matches('\r');
                            (trimmed.to_string(), &raw[trimmed.len()..])
                        } else {
                            (token.text().to_string(), "")
                        };
                        atoms.push(Atom { kind, text });
                        if !stripped_suffix.is_empty() {
                            pending_text.push_str(stripped_suffix);
                        }
                        // Roll `prev_atom_end` back past the stripped `\r`
                        // bytes so the next gap's source range covers them.
                        // Otherwise a per-gap edit would re-insert `\r`
                        // while leaving the original byte in place, doubling
                        // the carriage return.
                        prev_atom_end = tok_range.end() - TextSize::of(stripped_suffix);
                        // Every non-root token has a parent in a valid CST.
                        atom_nodes
                            .push(token.parent().expect("token without parent in syntax tree"));
                    }
                }
                WalkEvent::Leave(NodeOrToken::Token(_)) => {}
            }
        }

        // Trailing boundary gap: from the last atom's end to the root end.
        let stream_end = root.text_range().end();
        flush_gap(&mut gaps, &mut pending_text, stream_end, prev_atom_end);

        assert_eq!(
            gaps.len(),
            atoms.len() + 1,
            "IR invariant violated: gaps.len()={}, atoms.len()={}",
            gaps.len(),
            atoms.len()
        );
        assert_eq!(atoms.len(), atom_nodes.len(), "atom_nodes must align with atoms");

        Ir { atoms, gaps, atom_nodes }
    }
}

/// A `LITERAL` node whose children include any string-flavored token. Such
/// literals carry whitespace-significant content (multi-line `|`-strings,
/// string concatenation chains) and must be emitted as a single atom.
fn is_coalescing_literal(node: &SyntaxNode) -> bool {
    if node.kind() != SyntaxKind::LITERAL {
        return false;
    }
    node.children_with_tokens().any(|c| {
        if let NodeOrToken::Token(t) = c {
            matches!(
                t.kind(),
                SyntaxKind::STRING
                    | SyntaxKind::STRING_START
                    | SyntaxKind::STRING_PART
                    | SyntaxKind::STRING_TAIL
            )
        } else {
            false
        }
    })
}

/// Minimal placeholder policy: preserve every gap. Renders the source text
/// unchanged. Kept as the round-trip identity baseline used by IR tests.
#[cfg(test)]
fn apply_policy_preserve_all(
    ir: &Ir,
    _cfg: &FormattingConfig,
    _initial_indent: u32,
) -> Vec<GapDecision> {
    vec![GapDecision::Preserve; ir.gaps.len()]
}

/// Per-gap policy. Within-line spacing mirrors `whitespace.rs`; newline
/// gaps split into statement boundaries (re-indented via CST depth) and
/// expression continuations (preserved byte-for-byte).
pub fn apply_policy(ir: &Ir, cfg: &FormattingConfig, _initial_indent: u32) -> Vec<GapDecision> {
    let mut decisions = Vec::with_capacity(ir.gaps.len());
    let mut prev_was_unary = false;

    for i in 0..ir.gaps.len() {
        let prev_kind = if i == 0 { None } else { Some(ir.atoms[i - 1].kind) };
        let next_kind = if i == ir.atoms.len() { None } else { Some(ir.atoms[i].kind) };
        let gap_text = ir.gaps[i].text.as_str();

        let decision = if gap_text.contains('\n') {
            decide_newline_gap(ir, i, gap_text)
        } else {
            decide_inline_gap(prev_kind, next_kind, prev_was_unary, cfg)
        };
        // A newline gap whose LCA is a statement-boundary container starts
        // a new statement. The next atom is then at "start of statement"
        // for the `+`/`-` unary heuristic, even though its raw predecessor
        // (`;`, `Тогда`, `Иначе`, …) isn't in the unary list.
        let crossed_stmt_boundary = matches!(decision, GapDecision::NewlineWithIndent { .. });
        decisions.push(decision);

        prev_was_unary = if i < ir.atoms.len() {
            let prev_prev_kind =
                if i == 0 || crossed_stmt_boundary { None } else { Some(ir.atoms[i - 1].kind) };
            super::whitespace::is_likely_unary(ir.atoms[i].kind, prev_prev_kind)
        } else {
            false
        };
    }
    decisions
}

/// Decides spacing for a gap that contains at least one newline. The two
/// outcomes are:
///   * `NewlineWithIndent { newlines, level }` — statement boundary, recompute
///     indent from CST depth. `newlines` mirrors the source so user blank
///     lines round-trip.
///   * `Preserve` — expression continuation (both atoms inside the same
///     statement), don't reformat user-authored indent.
fn decide_newline_gap(ir: &Ir, gap_index: usize, gap_text: &str) -> GapDecision {
    // Boundary gap (leading or trailing): no surrounding statement context.
    if gap_index == 0 || gap_index == ir.gaps.len() - 1 {
        return GapDecision::Preserve;
    }
    let prev_node = &ir.atom_nodes[gap_index - 1];
    let next_node = &ir.atom_nodes[gap_index];

    // A newline is a statement boundary iff the lowest common ancestor of
    // the two atoms is a node that *contains* multiple statements / clauses
    // / block sides (STMT_LIST, SOURCE_FILE, IF_STMT, TRY_STMT, …). If the
    // LCA is a sub-expression node, we are inside one statement and the
    // newline is a user-authored continuation.
    let lca = lowest_common_ancestor(prev_node, next_node);
    if !is_statement_boundary_container(lca.kind()) {
        return GapDecision::Preserve;
    }

    let next_kind = ir.atoms[gap_index].kind;
    // Body indent (blank lines between statements) sits at the LCA's
    // depth — the depth where the blank lines visually live. Final indent
    // is the next atom's own depth, dropped by one for block-boundary
    // keywords (`КонецПроцедуры`, `Иначе`, …).
    let body_level = block_depth(&lca);
    let next_depth = block_depth(next_node);
    let final_level = if is_block_boundary_keyword(next_kind) {
        next_depth.saturating_sub(1)
    } else {
        next_depth
    };
    let newlines = count_newlines(gap_text);
    GapDecision::NewlineWithIndent { newlines, body_level, final_level }
}

fn decide_inline_gap(
    prev: Option<SyntaxKind>,
    next: Option<SyntaxKind>,
    prev_was_unary: bool,
    cfg: &FormattingConfig,
) -> GapDecision {
    use super::whitespace::{
        forbids_space_after, forbids_space_before, forbids_space_before_paren, is_likely_unary,
        needs_space_after, needs_space_before,
    };

    let (Some(prev), Some(next)) = (prev, next) else {
        return GapDecision::Preserve;
    };

    if prev_was_unary {
        return GapDecision::None;
    }

    // UTF-8 BOM is file-leading metadata, never separated from the first
    // content token. Without this the `next == COMMENT` rule below would
    // synthesize a space between `\u{FEFF}` and `//...`.
    if prev == SyntaxKind::BOM {
        return GapDecision::None;
    }

    // Trailing inline comment (`КонецФункции\t\t// note` → `КонецФункции // note`):
    // collapse any horizontal whitespace before a same-line comment to one
    // space. Comments on their own line are reached via the newline path.
    if next == SyntaxKind::COMMENT {
        return GapDecision::OneSpace;
    }

    let comma_after_comma = prev == SyntaxKind::COMMA && next == SyntaxKind::COMMA;
    if !comma_after_comma && forbids_space_before(next) {
        return GapDecision::None;
    }
    if next == SyntaxKind::L_PAREN && forbids_space_before_paren(prev) {
        return GapDecision::None;
    }
    if forbids_space_after(prev) {
        return GapDecision::None;
    }

    if is_likely_unary(next, Some(prev)) {
        return if needs_space_after(prev, cfg) {
            GapDecision::OneSpace
        } else {
            GapDecision::None
        };
    }

    if needs_space_before(next, cfg) || needs_space_after(prev, cfg) {
        return GapDecision::OneSpace;
    }

    // Fallback: respect the source. Inserting a space here would be policy
    // overreach for an unhandled token pair.
    GapDecision::Preserve
}

fn count_newlines(s: &str) -> u32 {
    s.bytes().filter(|&b| b == b'\n').count() as u32
}

/// Mirrors `engine::calculate_indent_at_offset`. CLAUSE nodes
/// (ELSIF/ELSE/EXCEPT) are intentionally excluded — their boundary keywords
/// get a `-1` adjustment via `is_block_boundary_keyword` instead, which
/// matches the line-based engine's behavior of placing `Иначе`/`Исключение`
/// at the outer indent level.
fn is_block_defining(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PROCEDURE_DEF
            | SyntaxKind::FUNCTION_DEF
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::PRE_REGION_DIR
            | SyntaxKind::PRE_IF_DIR
    )
}

/// Block-boundary keywords: the start/middle/end markers of block-defining
/// constructs. They sit lexically inside their block-defining ancestor but
/// visually align with the outer level — hence the `-1` adjustment.
fn is_block_boundary_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::KW_PROCEDURE
            | SyntaxKind::KW_END_PROCEDURE
            | SyntaxKind::KW_FUNCTION
            | SyntaxKind::KW_END_FUNCTION
            | SyntaxKind::KW_IF
            | SyntaxKind::KW_ELSIF
            | SyntaxKind::KW_ELSE
            | SyntaxKind::KW_END_IF
            | SyntaxKind::KW_WHILE
            | SyntaxKind::KW_FOR
            | SyntaxKind::KW_END_DO
            | SyntaxKind::KW_TRY
            | SyntaxKind::KW_EXCEPT
            | SyntaxKind::KW_END_TRY
            | SyntaxKind::PRE_REGION
            | SyntaxKind::PRE_END_REGION
            | SyntaxKind::PRE_IF
            | SyntaxKind::PRE_ELSIF
            | SyntaxKind::PRE_ELSE
            | SyntaxKind::PRE_END_IF
    )
}

/// A CST node kind that *contains* multiple sibling statements, clauses, or
/// block sides. When the lowest common ancestor of two atoms has one of
/// these kinds, the gap between them crosses a statement boundary — so a
/// newline there is "the line break between statements" and the next atom
/// should be re-indented from CST depth. Anything below (BIN_EXPR, ARG_LIST,
/// CALL_EXPR, …) means the atoms are inside a single statement and the
/// newline is an expression continuation that must be preserved.
fn is_statement_boundary_container(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::STMT_LIST
            | SyntaxKind::SOURCE_FILE
            | SyntaxKind::IF_STMT
            | SyntaxKind::ELSIF_CLAUSE
            | SyntaxKind::ELSE_CLAUSE
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::EXCEPT_CLAUSE
            | SyntaxKind::PROCEDURE_DEF
            | SyntaxKind::FUNCTION_DEF
            | SyntaxKind::PRE_REGION_DIR
            | SyntaxKind::PRE_IF_DIR
            | SyntaxKind::PRE_ELSIF_CLAUSE
            | SyntaxKind::PRE_ELSE_CLAUSE
    )
}

fn lowest_common_ancestor(a: &SyntaxNode, b: &SyntaxNode) -> SyntaxNode {
    use std::collections::HashSet;
    let a_chain: HashSet<SyntaxNode> =
        std::iter::successors(Some(a.clone()), |n| n.parent()).collect();
    std::iter::successors(Some(b.clone()), |n| n.parent())
        .find(|n| a_chain.contains(n))
        .expect("both atoms must share the SOURCE_FILE root")
}

fn block_depth(node: &SyntaxNode) -> u32 {
    std::iter::successors(Some(node.clone()), |n| n.parent())
        .filter(|n| is_block_defining(n.kind()))
        .count() as u32
}

/// Render the IR to a string with LF (`"\n"`) line endings. Production
/// callers use [`render_full`] directly to honour the source file's line
/// ending and collect per-gap edits; this is a test-only convenience.
#[cfg(test)]
fn render(ir: &Ir, decisions: &[GapDecision], cfg: &FormattingConfig) -> String {
    render_full(ir, decisions, cfg, "\n", false).0
}

/// A per-gap text edit produced by [`render_full`]. `range` covers the
/// source bytes the gap occupies; replacing those bytes with `new_text`
/// is equivalent to running the formatter on the gap in isolation.
#[derive(Debug, Clone)]
pub struct GapEdit {
    pub range: TextRange,
    pub new_text: String,
}

/// Render the IR using `line_ending` (typically `"\n"` or `"\r\n"`).
/// Returns the full output text plus per-gap edits whose `range` ↦
/// `new_text` collectively reproduce the same transformation. Gaps whose
/// rendered text matches the source emit no edit. `insert_final_newline`
/// appends one line ending to the trailing gap if the output doesn't
/// already end with one.
pub fn render_full(
    ir: &Ir,
    decisions: &[GapDecision],
    cfg: &FormattingConfig,
    line_ending: &str,
    insert_final_newline: bool,
) -> (String, Vec<GapEdit>) {
    assert_eq!(
        decisions.len(),
        ir.gaps.len(),
        "decisions.len()={} must equal gaps.len()={}",
        decisions.len(),
        ir.gaps.len()
    );

    let n_gaps = ir.gaps.len();
    let n_atoms = ir.atoms.len();
    let mut out = String::new();
    let mut edits = Vec::new();

    // Index loop: the body reads three parallel collections (`gaps`,
    // `decisions`, `atoms`) plus structural predicates (`is_last_gap`,
    // `has_*_atom_same_line`) that key off the index. An iterator chain
    // would obscure the parallelism without removing it.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n_gaps {
        let has_prev_atom_same_line = i > 0;
        let has_next_atom_same_line = i < n_atoms;
        let is_last_gap = i == n_gaps - 1;

        let mut rendered = emit_gap_text(&ir.gaps[i], &decisions[i], cfg, line_ending);
        // Preserve-gap byte stream may carry trailing whitespace from the
        // source; trim it per line. Synthesized decisions never emit
        // trailing whitespace, so the trim is a no-op for them. The
        // global trim flag also gates this so round-trip identity passes
        // (which use `apply_policy_preserve_all`) can keep raw bytes.
        if cfg.trim_trailing_whitespace && matches!(decisions[i], GapDecision::Preserve) {
            rendered =
                trim_preserve_gap(&rendered, has_prev_atom_same_line, has_next_atom_same_line);
        }
        // `insert_final_newline` lives on the trailing gap: if the source
        // doesn't end with a line ending and the policy didn't add one,
        // append one here so the edit reflects the same byte stream.
        if is_last_gap
            && insert_final_newline
            && (!out.is_empty() || !rendered.is_empty() || n_atoms > 0)
            && !rendered.ends_with('\n')
            && !rendered.ends_with("\r\n")
        {
            rendered.push_str(line_ending);
        }

        if rendered != ir.gaps[i].text {
            edits.push(GapEdit { range: ir.gaps[i].range, new_text: rendered.clone() });
        }
        out.push_str(&rendered);

        if i < n_atoms {
            out.push_str(&ir.atoms[i].text);
        }
    }

    (out, edits)
}

fn emit_gap_text(
    gap: &Gap,
    decision: &GapDecision,
    cfg: &FormattingConfig,
    line_ending: &str,
) -> String {
    match decision {
        GapDecision::Preserve => gap.text.clone(),
        GapDecision::None => String::new(),
        GapDecision::OneSpace => " ".to_string(),
        GapDecision::NewlineWithIndent { newlines, body_level, final_level } => {
            let body_indent = cfg.indent_for_level(*body_level);
            let final_indent = cfg.indent_for_level(*final_level);
            let mut out = String::new();
            let n = *newlines;
            for k in 0..n {
                out.push_str(line_ending);
                if k + 1 < n {
                    out.push_str(&body_indent);
                } else {
                    out.push_str(&final_indent);
                }
            }
            out
        }
    }
}

/// Trim trailing horizontal whitespace per "line" in a Preserve gap. A
/// segment of the gap that immediately follows an atom on the same line
/// (the first segment when there is a previous atom) has its trailing
/// `' '`/`'\t'` stripped. Other segments are pure whitespace inside the
/// gap and are kept verbatim (the blank-but-indented rule).
fn trim_preserve_gap(
    gap_text: &str,
    has_prev_atom_same_line: bool,
    has_next_atom_same_line: bool,
) -> String {
    if !gap_text.contains('\n') {
        // Single-segment gap: trim only if it is trailing whitespace
        // after the last atom of the file (or this line, equivalently —
        // there's a previous atom and no following atom on the same line).
        if has_prev_atom_same_line && !has_next_atom_same_line {
            return gap_text.trim_end_matches([' ', '\t']).to_string();
        }
        return gap_text.to_string();
    }

    let mut segments = gap_text.split('\n');
    let first = segments.next().unwrap();
    let first_rendered = if has_prev_atom_same_line {
        first.trim_end_matches([' ', '\t']).to_string()
    } else {
        first.to_string()
    };

    let mut out = first_rendered;
    for seg in segments {
        out.push('\n');
        out.push_str(seg);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(src: &str) -> Ir {
        let parsed = parser::parse(src);
        Ir::build(&parsed.syntax_node())
    }

    fn round_trip(src: &str) -> String {
        let ir = build(src);
        // Disable trim so the identity-policy passes through trailing
        // whitespace byte-for-byte. The full formatter (which has a
        // real policy) enables trim via the same flag.
        let cfg = FormattingConfig { trim_trailing_whitespace: false, ..Default::default() };
        let decisions = apply_policy_preserve_all(&ir, &cfg, 0);
        render(&ir, &decisions, &cfg)
    }

    #[test]
    fn round_trip_empty() {
        assert_eq!(round_trip(""), "");
    }

    #[test]
    fn round_trip_simple_procedure() {
        let src = "Процедура Тест()\n\tА = 1;\nКонецПроцедуры\n";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn round_trip_preserves_bom() {
        let src = "\u{FEFF}//comment\nПерем А Экспорт;\n";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn round_trip_preserves_multiline_string() {
        // The `\n` inside the string literal must survive byte-for-byte.
        let src = "А = \"ВЫБРАТЬ\n|\tX.A\n|ИЗ\n|\tT КАК X\";\n";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn round_trip_preserves_plus_continuation() {
        let src = "а = \"foo\"\n\t\t+ \": \" + б;\n";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn round_trip_preserves_consecutive_commas() {
        let src = "Ф(а, б, , , , в);\n";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn ir_coalesces_multiline_string_into_single_atom() {
        let ir = build("А = \"a\n|b\n|c\";");
        let string_atoms: Vec<_> =
            ir.atoms.iter().filter(|a| a.kind == SyntaxKind::LITERAL).collect();
        // Exactly one LITERAL atom for the entire multi-line string.
        assert_eq!(string_atoms.len(), 1, "expected one coalesced LITERAL atom");
        assert!(string_atoms[0].text.contains('\n'));
    }

    #[test]
    fn ir_invariant_gaps_one_more_than_atoms() {
        let cases = [
            "",
            "А = 1;",
            "Процедура Т()\nКонецПроцедуры\n",
            "\u{FEFF}//x\n",
            "А = \"a\n|b\";",
            "   А = 1;",
            "А = 1; ",
            "   \n\t",
            "А",
        ];
        for src in cases {
            let ir = build(src);
            assert_eq!(
                ir.gaps.len(),
                ir.atoms.len() + 1,
                "invariant violated for {:?}: gaps={}, atoms={}",
                src,
                ir.gaps.len(),
                ir.atoms.len()
            );
        }
    }

    #[test]
    fn round_trip_leading_whitespace() {
        let src = "   А = 1;\n";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn round_trip_trailing_whitespace() {
        let src = "А = 1;   \n";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn round_trip_whitespace_only() {
        let src = "   \n\t";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn round_trip_single_atom_no_surrounding_ws() {
        let src = "А";
        let ir = build(src);
        assert_eq!(ir.atoms.len(), 1);
        // Leading boundary gap (0..0, ""), atom "А", trailing boundary
        // gap (1..1, "") — both zero-width on either side.
        assert_eq!(ir.gaps.len(), 2);
        assert_eq!(ir.gaps[0].text, "");
        assert_eq!(ir.gaps[1].text, "");
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn build_survives_parse_errors() {
        // Open paren never closed — parser emits errors but tokens are
        // still attached to the syntax tree; the IR must round-trip them.
        let src = "А = (\n";
        let ir = build(src);
        assert!(!ir.atoms.is_empty());
        assert_eq!(round_trip(src), src);
    }

    /// Renders `src` through `apply_policy` (not the preserve-all baseline).
    fn format_via_policy(src: &str) -> String {
        let ir = build(src);
        let cfg = FormattingConfig::default();
        let decisions = apply_policy(&ir, &cfg, 0);
        render(&ir, &decisions, &cfg)
    }

    #[test]
    fn policy_assignment_spaces() {
        // Tight `А=1;` expands to `А = 1;`.
        assert_eq!(format_via_policy("А=1;"), "А = 1;");
    }

    #[test]
    fn policy_method_call_no_space_before_paren() {
        // The KW_EXECUTE-before-paren rule fires through the new pipeline.
        assert_eq!(format_via_policy("Х.Выполнить ()"), "Х.Выполнить()");
    }

    #[test]
    fn policy_index_no_space_before_bracket() {
        assert_eq!(format_via_policy("Х [0]"), "Х[0]");
    }

    #[test]
    fn policy_comma_after_comma_keeps_space() {
        // Skipped default arguments stay visually separated.
        assert_eq!(format_via_policy("Ф(а,,,, в)"), "Ф(а, , , , в)");
    }

    #[test]
    fn policy_unary_minus_no_inner_space() {
        assert_eq!(format_via_policy("А = - 1;"), "А = -1;");
        assert_eq!(format_via_policy("Ф(-1)"), "Ф(-1)");
    }

    #[test]
    fn policy_keyword_space() {
        assert_eq!(format_via_policy("Возврат  А;"), "Возврат А;");
    }

    #[test]
    fn policy_paren_no_inner_spaces() {
        assert_eq!(format_via_policy("Ф( А )"), "Ф(А)");
    }

    #[test]
    fn policy_preserves_newlines_unchanged() {
        // Step B.1 leaves newline-containing gaps alone. The output is the
        // input verbatim because no within-line normalization is triggered.
        let src = "Процедура Т()\nКонецПроцедуры\n";
        assert_eq!(format_via_policy(src), src);
    }

    #[test]
    fn policy_preserves_multiline_string_atom() {
        let src = "А = \"a\n|b\n|c\";";
        assert_eq!(format_via_policy(src), src);
    }

    // ----- Step B.2: CST-driven indent for newline gaps -----

    #[test]
    fn policy_reindents_procedure_body() {
        // The line-based engine indents `А = 1;` to one tab inside the
        // procedure body; the IR pipeline should produce the same shape.
        let src = "Процедура Тест()\nА = 1;\nКонецПроцедуры";
        let expected = "Процедура Тест()\n\tА = 1;\nКонецПроцедуры";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_reindents_nested_if() {
        let src = "Процедура Т()\nЕсли А Тогда\nЕсли Б Тогда\nВ = 1;\nКонецЕсли;\nКонецЕсли;\nКонецПроцедуры";
        let expected = "Процедура Т()\n\tЕсли А Тогда\n\t\tЕсли Б Тогда\n\t\t\tВ = 1;\n\t\tКонецЕсли;\n\tКонецЕсли;\nКонецПроцедуры";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_else_at_outer_level() {
        // `Иначе` sits inside ELSE_CLAUSE inside IF_STMT but is displayed
        // at the outer level via the boundary-keyword adjustment.
        let src = "Если А Тогда\nБ = 1;\nИначе\nВ = 2;\nКонецЕсли;";
        let expected = "Если А Тогда\n\tБ = 1;\nИначе\n\tВ = 2;\nКонецЕсли;";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_try_except_indent() {
        let src = "Попытка\nА = 1;\nИсключение\nБ = 2;\nКонецПопытки;";
        let expected = "Попытка\n\tА = 1;\nИсключение\n\tБ = 2;\nКонецПопытки;";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_preserves_plus_continuation() {
        // Both atoms surrounding the newline are in the same assignment
        // statement → expression continuation → user indent kept.
        let src = "а = \"foo\"\n\t\t+ \": \" + б;";
        assert_eq!(format_via_policy(src), src);
    }

    // ----- Cross-newline unary/binary `+`/`-` classification -----

    #[test]
    fn policy_unary_plus_after_return_across_newline() {
        // `Возврат\n+ А;` parses as one return statement (`+ А` is the
        // return expression). The newline gap is expression-continuation,
        // so the user's indent on the `+` line is preserved (zero here),
        // and the unary classification (KW_RETURN is in the unary list)
        // collapses `+ А` to `+А`.
        let src = "Процедура Т()\nВозврат\n+ А;\nКонецПроцедуры";
        let expected = "Процедура Т()\n\tВозврат\n+А;\nКонецПроцедуры";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_unary_plus_after_semicolon_across_newline() {
        // `+ А;` is its own statement; `+` after `;` should classify as
        // unary (start of new expression).
        let src = "Процедура Т()\nБ = 1;\n+ А;\nКонецПроцедуры";
        let expected = "Процедура Т()\n\tБ = 1;\n\t+А;\nКонецПроцедуры";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_unary_plus_after_then_across_newline() {
        // `Тогда\n+ Б;` — `+` opens body, unary on Б.
        let src = "Если А Тогда\n+ Б;\nКонецЕсли;";
        let expected = "Если А Тогда\n\t+Б;\nКонецЕсли;";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_binary_plus_continuation_across_newline() {
        // Inside a single assignment statement, `+` is binary continuation.
        // The newline is expression-continuation (preserved), and the in-
        // line spacing around `+` is binary (one space each side).
        let src = "а = 1\n\t\t+ б;";
        assert_eq!(format_via_policy(src), src);
    }

    #[test]
    fn policy_preserves_blank_lines() {
        let src = "Процедура Тест()\n\nА = 1;\n\nКонецПроцедуры";
        // Each blank line in the source produces a blank indented line in
        // the output.
        let expected = "Процедура Тест()\n\t\n\tА = 1;\n\t\nКонецПроцедуры";
        assert_eq!(format_via_policy(src), expected);
    }
}
