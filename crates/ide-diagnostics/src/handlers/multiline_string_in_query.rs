//! MultilineStringInQuery diagnostic.
//!
//! Detects multiline string literals in SDBL queries.
//!
//! ## Why?
//! Multiline string literals in SDBL queries are very rare and usually indicate
//! an error from incorrect number of double quotes. In SDBL, to represent an
//! empty string you should use """" (4 quotes), not "" (2 quotes).
//!
//! ## Bad practice
//! ```bsl
//! Query.Text = "SELECT
//! |   ЕСТЬNULL(Field, "") AS Code  // Wrong: "" becomes multiline string
//! |FROM Table";
//! ```
//!
//! ## Good practice
//! ```bsl
//! Query.Text = "SELECT
//! |   ЕСТЬNULL(Field, """") AS Code  // Correct: """" is empty string in SDBL
//! |FROM Table";
//! ```
//!
//! ## Implementation
//! Ported from:
//! - MultilineStringInQueryDiagnostic.java (bsl-language-server)

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::SyntaxKind;
use tracing::debug;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::MultilineStringInQuery) {
        return Vec::new();
    }

    let sdbl_queries = ctx.db.all_sdbl_in_file(ctx.file_id);
    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);

    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    for (_expr_id, query_info) in sdbl_queries.iter() {
        if !query_info.is_valid() {
            continue;
        }
        let Some(ref query_ast) = query_info.query_ast else {
            continue;
        };

        let mapper = SdblPositionMapper::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            &bsl_source,
            &line_starts,
        );

        check_query(query_ast, &query_info.query_text, &mapper, &mut diagnostics);
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "MultilineStringInQuery completed"
    );

    diagnostics
}

fn check_query(
    query_ast: &syntax::Parse<syntax::SyntaxNode>,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let root = query_ast.syntax_node();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::SDBL_MULTI_STRING => {
                let string_count = node
                    .children_with_tokens()
                    .filter(|child| child.kind() == SyntaxKind::STRING)
                    .count();

                if string_count > 2 {
                    // Map SDBL range to BSL coordinates
                    let sdbl_range = node.text_range();
                    let bsl_range = mapper.map_range(sdbl_range, query_text);

                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::MultilineStringInQuery,
                        message: "Check if multiline literal is correct".to_string(),
                        severity: Severity::Critical,
                        range: bsl_range,
                        tags: vec![],
                        fixes: vec![],
                    });
                }
            }
            SyntaxKind::SDBL_LITERAL => {
                for child in node.children_with_tokens() {
                    if let Some(token) = child.as_token() {
                        if token.kind() == SyntaxKind::STRING && token.text().contains('\n') {
                            let sdbl_range = token.text_range();
                            let bsl_range = mapper.map_range(sdbl_range, query_text);
                            diagnostics.push(Diagnostic {
                                code: DiagnosticCode::MultilineStringInQuery,
                                message: "Check if multiline literal is correct".to_string(),
                                severity: Severity::Critical,
                                range: bsl_range,
                                tags: vec![],
                                fixes: vec![],
                            });
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_sdbl_diagnostic;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let diagnostics = check_sdbl_diagnostic(code, check);
        // Extract content from the code (remove fixture header if present)
        let content = code.strip_prefix("//- /test.bsl\n").unwrap_or(code).to_string();
        (diagnostics, content)
    }

    #[test]
    fn test_multiline_string_in_query() {
        let code = include_str!("../../test_data/MultilineStringInQueryDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 3);

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::MultilineStringInQuery);
            assert_eq!(diag.severity, Severity::Critical);
            assert_eq!(diag.message, "Check if multiline literal is correct");
        }

        use crate::test_utils::assert_diagnostic_range_multiline;

        assert_diagnostic_range_multiline(&file_content, &diagnostics[0], 5, 8, 6, 5);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[1], 6, 30, 10, 10);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[2], 15, 60, 16, 68);
    }
}
