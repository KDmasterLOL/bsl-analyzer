use syntax::{NodeOrToken, SyntaxKind, SyntaxNode, TextRange, TextSize, WalkEvent};

use super::FormattingConfig;

#[derive(Debug, Clone)]
pub struct Atom {
    pub kind: SyntaxKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Gap {
    pub range: TextRange,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapDecision {
    Preserve,
    None,
    OneSpace,
    NewlineWithIndent { newlines: u32, body_level: u32, final_level: u32 },
}

#[derive(Debug, Clone, Default)]
pub struct Ir {
    pub atoms: Vec<Atom>,
    pub gaps: Vec<Gap>,
    pub atom_nodes: Vec<SyntaxNode>,
    pub atom_edits: Vec<GapEdit>,
}

impl Ir {
    pub fn build(root: &SyntaxNode) -> Self {
        let mut atoms: Vec<Atom> = Vec::new();
        let mut gaps: Vec<Gap> = Vec::new();
        let mut atom_nodes: Vec<SyntaxNode> = Vec::new();
        let mut atom_edits: Vec<GapEdit> = Vec::new();

        let mut pending_text = String::new();
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
                        let (text, stripped_suffix) = if kind == SyntaxKind::COMMENT {
                            let raw = token.text();
                            let trimmed = raw.trim_end_matches('\r');
                            let normalized = normalize_comment_spacing(trimmed);
                            let suffix = &raw[trimmed.len()..];
                            if normalized != trimmed {
                                let effective_end = tok_range.end() - TextSize::of(suffix);
                                atom_edits.push(GapEdit {
                                    range: TextRange::new(tok_range.start(), effective_end),
                                    new_text: normalized.clone(),
                                });
                            }
                            (normalized, suffix)
                        } else {
                            (token.text().to_string(), "")
                        };
                        atoms.push(Atom { kind, text });
                        if !stripped_suffix.is_empty() {
                            pending_text.push_str(stripped_suffix);
                        }
                        prev_atom_end = tok_range.end() - TextSize::of(stripped_suffix);
                        atom_nodes
                            .push(token.parent().expect("token without parent in syntax tree"));
                    }
                }
                WalkEvent::Leave(NodeOrToken::Token(_)) => {}
            }
        }

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

        Ir { atoms, gaps, atom_nodes, atom_edits }
    }
}

fn reindent_literal_continuation_lines(text: &str, target_indent: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut iter = text.split('\n');
    if let Some(first) = iter.next() {
        out.push_str(first);
    }
    for line in iter {
        out.push('\n');
        let content_start = line
            .char_indices()
            .find_map(|(i, c)| (!c.is_whitespace()).then_some(i))
            .unwrap_or(line.len());
        if line.as_bytes().get(content_start) == Some(&b'|') {
            out.push_str(target_indent);
            out.push_str(&line[content_start..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

fn normalize_comment_spacing(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("//") else { return raw.to_string() };
    if rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace()) {
        return raw.to_string();
    }
    let mut s = String::with_capacity(raw.len() + 1);
    s.push_str("// ");
    s.push_str(rest);
    s
}

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

#[cfg(test)]
fn apply_policy_preserve_all(
    ir: &Ir,
    _cfg: &FormattingConfig,
    _initial_indent: u32,
) -> Vec<GapDecision> {
    vec![GapDecision::Preserve; ir.gaps.len()]
}

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

fn decide_newline_gap(ir: &Ir, gap_index: usize, gap_text: &str) -> GapDecision {
    if gap_index == 0 || gap_index == ir.gaps.len() - 1 {
        return GapDecision::Preserve;
    }
    let prev_node = &ir.atom_nodes[gap_index - 1];
    let next_node = &ir.atom_nodes[gap_index];

    let lca = lowest_common_ancestor(prev_node, next_node);
    if !is_statement_boundary_container(lca.kind()) {
        let prev_kind = ir.atoms[gap_index - 1].kind;
        let next_atom = &ir.atoms[gap_index];
        let prev_anchors_multiline_literal = matches!(prev_kind, SyntaxKind::EQ | SyntaxKind::PLUS);
        if prev_anchors_multiline_literal
            && next_atom.kind == SyntaxKind::LITERAL
            && next_atom.text.contains('\n')
        {
            let body_level = block_depth(&lca);
            let newlines = count_newlines(gap_text);
            return GapDecision::NewlineWithIndent {
                newlines,
                body_level,
                final_level: body_level + 1,
            };
        }
        return GapDecision::Preserve;
    }

    let next_kind = ir.atoms[gap_index].kind;
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

    if prev == SyntaxKind::BOM {
        return GapDecision::None;
    }

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

    GapDecision::Preserve
}

fn count_newlines(s: &str) -> u32 {
    s.bytes().filter(|&b| b == b'\n').count() as u32
}

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
    )
}

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

#[cfg(test)]
fn render(ir: &Ir, decisions: &[GapDecision], cfg: &FormattingConfig) -> String {
    render_full(ir, decisions, cfg, "\n", false).0
}

#[derive(Debug, Clone)]
pub struct GapEdit {
    pub range: TextRange,
    pub new_text: String,
}

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

    #[allow(clippy::needless_range_loop)]
    for i in 0..n_gaps {
        let has_prev_atom_same_line = i > 0;
        let has_next_atom_same_line = i < n_atoms;
        let is_last_gap = i == n_gaps - 1;

        let mut rendered = emit_gap_text(&ir.gaps[i], &decisions[i], cfg, line_ending);
        if cfg.trim_trailing_whitespace && matches!(decisions[i], GapDecision::Preserve) {
            rendered =
                trim_preserve_gap(&rendered, has_prev_atom_same_line, has_next_atom_same_line);
        }
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
            let atom = &ir.atoms[i];
            let needs_reindent = atom.kind == SyntaxKind::LITERAL
                && atom.text.contains('\n')
                && matches!(decisions[i], GapDecision::NewlineWithIndent { .. });
            if needs_reindent {
                if let GapDecision::NewlineWithIndent { final_level, .. } = decisions[i] {
                    let target_indent = cfg.indent_for_level(final_level);
                    let normalized =
                        reindent_literal_continuation_lines(&atom.text, &target_indent);
                    if normalized != atom.text {
                        edits.push(GapEdit {
                            range: ir.atom_nodes[i].text_range(),
                            new_text: normalized.clone(),
                        });
                    }
                    out.push_str(&normalized);
                    continue;
                }
            }
            out.push_str(&atom.text);
        }
    }

    edits.extend(ir.atom_edits.iter().cloned());
    edits.sort_by_key(|e| e.range.start());

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

fn trim_preserve_gap(
    gap_text: &str,
    has_prev_atom_same_line: bool,
    has_next_atom_same_line: bool,
) -> String {
    if !gap_text.contains('\n') {
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
        let src = "\u{FEFF}// comment\nПерем А Экспорт;\n";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn round_trip_preserves_multiline_string() {
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
        assert_eq!(ir.gaps.len(), 2);
        assert_eq!(ir.gaps[0].text, "");
        assert_eq!(ir.gaps[1].text, "");
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn build_survives_parse_errors() {
        let src = "А = (\n";
        let ir = build(src);
        assert!(!ir.atoms.is_empty());
        assert_eq!(round_trip(src), src);
    }

    fn format_via_policy(src: &str) -> String {
        let ir = build(src);
        let cfg = FormattingConfig::default();
        let decisions = apply_policy(&ir, &cfg, 0);
        render(&ir, &decisions, &cfg)
    }

    #[test]
    fn policy_assignment_spaces() {
        assert_eq!(format_via_policy("А=1;"), "А = 1;");
    }

    #[test]
    fn policy_method_call_no_space_before_paren() {
        assert_eq!(format_via_policy("Х.Выполнить ()"), "Х.Выполнить()");
    }

    #[test]
    fn policy_index_no_space_before_bracket() {
        assert_eq!(format_via_policy("Х [0]"), "Х[0]");
    }

    #[test]
    fn policy_comma_after_comma_keeps_space() {
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
        let src = "Процедура Т()\nКонецПроцедуры\n";
        assert_eq!(format_via_policy(src), src);
    }

    #[test]
    fn policy_preserves_multiline_string_atom() {
        let src = "А = \"a\n|b\n|c\";";
        assert_eq!(format_via_policy(src), src);
    }

    #[test]
    fn policy_reindents_procedure_body() {
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
        let src = "а = \"foo\"\n\t\t+ \": \" + б;";
        assert_eq!(format_via_policy(src), src);
    }

    #[test]
    fn policy_unary_plus_after_return_across_newline() {
        let src = "Процедура Т()\nВозврат\n+ А;\nКонецПроцедуры";
        let expected = "Процедура Т()\n\tВозврат\n+А;\nКонецПроцедуры";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_unary_plus_after_semicolon_across_newline() {
        let src = "Процедура Т()\nБ = 1;\n+ А;\nКонецПроцедуры";
        let expected = "Процедура Т()\n\tБ = 1;\n\t+А;\nКонецПроцедуры";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_unary_plus_after_then_across_newline() {
        let src = "Если А Тогда\n+ Б;\nКонецЕсли;";
        let expected = "Если А Тогда\n\t+Б;\nКонецЕсли;";
        assert_eq!(format_via_policy(src), expected);
    }

    #[test]
    fn policy_binary_plus_continuation_across_newline() {
        let src = "а = 1\n\t\t+ б;";
        assert_eq!(format_via_policy(src), src);
    }

    #[test]
    fn policy_preserves_blank_lines() {
        let src = "Процедура Тест()\n\nА = 1;\n\nКонецПроцедуры";
        let expected = "Процедура Тест()\n\t\n\tА = 1;\n\t\nКонецПроцедуры";
        assert_eq!(format_via_policy(src), expected);
    }
}
