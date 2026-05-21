// Phase 2 of the formatter is landing as a sequence of self-contained
// commits. This module is the foundation (build + render + boundary-gap
// invariant); the policy and engine wiring follow. Until those land, the
// items below are reachable only from unit tests, so suppress dead-code
// warnings for the whole module rather than scatter narrow allows.
#![allow(dead_code)]

//! Token-level IR for the BSL formatter (Phase 2 architecture).
//!
//! Builds a flat stream of opaque [`Atom`]s separated by [`Gap`]s from a
//! Rowan CST. The pipeline is:
//!
//! ```text
//!   Ir::build(root)                      — CST traversal → IR
//!   apply_policy_preserve_all(ir, cfg)   — per-gap GapDecision (placeholder)
//!   render(ir, decisions, cfg)           — final String
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
    pub range: TextRange,
    pub text: String,
}

/// The whitespace between two atoms (or framing the stream at the edges).
/// Comments are NOT gaps — they are their own atoms.
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
    /// Emit `\n` followed by indent at the given abstract level. The
    /// renderer expands the level into tabs/spaces using [`FormattingConfig`].
    NewlineWithIndent(u32),
}

#[derive(Debug, Clone, Default)]
pub struct Ir {
    pub atoms: Vec<Atom>,
    pub gaps: Vec<Gap>,
}

impl Ir {
    /// Builds the IR from a Rowan CST root. See module-level invariants.
    pub fn build(root: &SyntaxNode) -> Self {
        let mut atoms: Vec<Atom> = Vec::new();
        let mut gaps: Vec<Gap> = Vec::new();

        // Pending whitespace accumulator between two atoms.
        let mut pending_start: Option<TextSize> = None;
        let mut pending_text = String::new();

        // When traversing inside a coalesced LITERAL node, skip its children.
        let mut coalesce_until: Option<SyntaxNode> = None;

        let stream_end = root.text_range().end();

        // Helper: flush accumulated whitespace into one Gap and clear state.
        let flush_gap = |gaps: &mut Vec<Gap>,
                         start: &mut Option<TextSize>,
                         text: &mut String,
                         end: TextSize| {
            let start_pos = start.take().unwrap_or(end);
            let actual_end = if text.is_empty() { start_pos } else { end };
            gaps.push(Gap {
                range: TextRange::new(start_pos, actual_end),
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
                        let range = node.text_range();
                        // Close the gap that precedes this atom.
                        flush_gap(&mut gaps, &mut pending_start, &mut pending_text, range.start());
                        atoms.push(Atom {
                            kind: node.kind(),
                            range,
                            text: node.text().to_string(),
                        });
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
                    let range = token.text_range();

                    if matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE) {
                        if pending_start.is_none() {
                            pending_start = Some(range.start());
                        }
                        pending_text.push_str(token.text());
                    } else {
                        flush_gap(&mut gaps, &mut pending_start, &mut pending_text, range.start());
                        atoms.push(Atom { kind, range, text: token.text().to_string() });
                    }
                }
                WalkEvent::Leave(NodeOrToken::Token(_)) => {}
            }
        }

        // We emit one gap before each atom; the call below emits the final
        // trailing boundary gap. The closure always pushes, so the invariant
        // `gaps.len() == atoms.len() + 1` holds even for empty input
        // (no atoms, one zero-width gap).
        flush_gap(&mut gaps, &mut pending_start, &mut pending_text, stream_end);

        assert_eq!(
            gaps.len(),
            atoms.len() + 1,
            "IR invariant violated: gaps.len()={}, atoms.len()={}",
            gaps.len(),
            atoms.len()
        );

        Ir { atoms, gaps }
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
/// unchanged. Used as the baseline for Phase 2 incremental rule porting.
pub fn apply_policy_preserve_all(
    ir: &Ir,
    _cfg: &FormattingConfig,
    _initial_indent: u32,
) -> Vec<GapDecision> {
    vec![GapDecision::Preserve; ir.gaps.len()]
}

/// Render the IR to a string. Atoms are emitted verbatim; each gap is
/// emitted per its [`GapDecision`].
pub fn render(ir: &Ir, decisions: &[GapDecision], cfg: &FormattingConfig) -> String {
    assert_eq!(
        decisions.len(),
        ir.gaps.len(),
        "decisions.len()={} must equal gaps.len()={}",
        decisions.len(),
        ir.gaps.len()
    );

    let mut out = String::new();
    // Interleave: gap[0] atom[0] gap[1] atom[1] ... atom[n-1] gap[n].
    emit_gap(&mut out, &ir.gaps[0], &decisions[0], cfg);
    for (i, atom) in ir.atoms.iter().enumerate() {
        out.push_str(&atom.text);
        emit_gap(&mut out, &ir.gaps[i + 1], &decisions[i + 1], cfg);
    }
    out
}

fn emit_gap(out: &mut String, gap: &Gap, decision: &GapDecision, cfg: &FormattingConfig) {
    match decision {
        GapDecision::Preserve => out.push_str(&gap.text),
        GapDecision::None => {}
        GapDecision::OneSpace => out.push(' '),
        GapDecision::NewlineWithIndent(level) => {
            out.push('\n');
            out.push_str(&cfg.indent_for_level(*level));
        }
    }
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
        let cfg = FormattingConfig::default();
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
}
