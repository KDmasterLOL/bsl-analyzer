//! UnreachableCode diagnostic.
//!
//! Detects code that will never be executed because it follows a control flow
//! statement like `return`, `raise`, `break`, or `continue`.
//!
//! ## Why?
//! Unreachable code indicates a logic error or dead code that should be removed:
//! - After `Возврат` / `Return` - function has already exited
//! - After `ВызватьИсключение` / `Raise` - exception was thrown
//! - After `Прервать` / `Break` - loop was exited
//! - After `Продолжить` / `Continue` - jumped to next iteration
//!
//! ## Bad practice
//! ```bsl
//! Процедура Пример()
//!     Возврат;
//!     Сообщить("Этот код никогда не выполнится"); // ❌ Unreachable
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Пример()
//!     Если Условие Тогда
//!         Сообщить("Условие истинно");
//!         Возврат;
//!     КонецЕсли;
//!     Сообщить("Условие ложно"); // ✅ Reachable
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Minor (potential bug)
//! - **Tags:** DESIGN, SUSPICIOUS
//!
//! ## Implementation
//! CFG-based diagnostic. Finds vertices with no incoming edges (unreachable blocks).
//!
//! Ported from:
//! - UnreachableCodeDiagnostic.java (bsl-language-server)

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use cfg::CfgVertex;
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UnreachableCode;

    if ctx.is_disabled_with_metadata(code) {
        return vec![];
    }

    let mut diagnostics = Vec::new();
    let module_bodies = ctx.module_bodies();
    let module_cfgs = ctx.module_cfgs();
    let source_text = ctx.file_text();

    // Check methods
    for (local_id, _body) in module_bodies.iter_bodies() {
        let Some(source_map) = module_bodies.source_map(local_id) else {
            continue;
        };
        let Some(cfg) = module_cfgs.get(local_id) else {
            continue;
        };

        let Some(entry) = cfg.entry_point() else {
            continue;
        };
        let exit = cfg.exit_point();

        // Compute reachable vertices via DFS from entry, following only live edges
        let reachable = compute_reachable_vertices(cfg, entry);

        // Collect vertices that are "locally unreachable" - unreachable due to
        // a terminator in the same scope, not due to external unreachability.
        let locally_unreachable = compute_locally_unreachable(cfg, &reachable);

        let unreachable_ranges = collect_unreachable_ranges(cfg, source_map, entry, exit, |idx| {
            locally_unreachable.contains(&idx)
        });

        create_diagnostics(&mut diagnostics, unreachable_ranges, source_text.as_str(), code, ctx);
    }

    // Check module-level code (statements outside procedures/functions)
    if let Some(module_result) = module_bodies.module_code_result() {
        let body = &module_result.body;
        let source_map = &module_result.source_map;

        #[cfg(test)]
        eprintln!("Module body_stmts count: {}", body.body_stmts_typed().len());

        // Build CFG for module code
        let cfg = cfg::CfgBuilder::new().build_graph_from_hir(
            body.body_stmts_typed(),
            body,
            Some(source_map),
        );

        if let Some(entry) = cfg.entry_point() {
            let exit = cfg.exit_point();
            let reachable = compute_reachable_vertices(&cfg, entry);

            #[cfg(test)]
            {
                eprintln!("=== Module-level CFG ===");
                for (idx, vertex) in cfg.vertices() {
                    let is_reachable = reachable.contains(&idx);
                    eprintln!("  {:?}: {:?}, reachable={}", idx, vertex, is_reachable);
                }
            }

            let unreachable_ranges =
                collect_unreachable_ranges(&cfg, source_map, entry, exit, |idx| {
                    !reachable.contains(&idx)
                });

            create_diagnostics(
                &mut diagnostics,
                unreachable_ranges,
                source_text.as_str(),
                code,
                ctx,
            );
        }
    }

    diagnostics
}

/// Collect ranges of unreachable vertices from CFG.
fn collect_unreachable_ranges<F>(
    cfg: &cfg::ControlFlowGraph,
    source_map: &hir_def::BodySourceMap,
    entry: cfg::NodeIndex,
    exit: cfg::NodeIndex,
    is_unreachable: F,
) -> Vec<TextRange>
where
    F: Fn(cfg::NodeIndex) -> bool,
{
    let mut ranges = Vec::new();

    for (vertex_idx, vertex) in cfg.vertices() {
        if vertex_idx == entry || vertex_idx == exit {
            continue;
        }

        if !is_unreachable(vertex_idx) {
            continue;
        }

        if let CfgVertex::BasicBlock(block) = vertex {
            for &stmt_id in block.statements() {
                if let Some(range) = source_map.stmt_range(stmt_id) {
                    ranges.push(range);
                }
            }
        } else if let Some(range) = get_vertex_range(vertex, source_map) {
            ranges.push(range);
        }
    }

    ranges
}

/// Create diagnostics from merged unreachable ranges.
fn create_diagnostics(
    diagnostics: &mut Vec<Diagnostic>,
    ranges: Vec<TextRange>,
    source_text: &str,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    let merged = merge_ranges(ranges, source_text);
    for range in merged {
        diagnostics.push(Diagnostic {
            code,
            message: message_ru(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

/// Merge adjacent or overlapping ranges into larger ranges.
///
/// This combines multiple unreachable blocks into single diagnostics,
/// matching Java behavior where a whole unreachable block gets one diagnostic.
///
/// Ranges are considered adjacent if they are on the same line or adjacent lines
/// (no blank line between them). This is more reliable than byte-based gaps
/// because it correctly handles different scopes (e.g., code inside if vs module level).
fn merge_ranges(mut ranges: Vec<TextRange>, source_text: &str) -> Vec<TextRange> {
    if ranges.is_empty() {
        return ranges;
    }

    // Sort by start position
    ranges.sort_by_key(|r| r.start());

    let mut merged: Vec<TextRange> = Vec::new();
    let mut current = ranges[0];

    for range in ranges.into_iter().skip(1) {
        // Count newlines between current range end and next range start
        let gap_start = usize::from(current.end());
        let gap_end = usize::from(range.start());

        let should_merge = if gap_end > gap_start && gap_end <= source_text.len() {
            let gap_text = &source_text[gap_start..gap_end];
            let newline_count = gap_text.chars().filter(|&c| c == '\n').count();
            // Merge if 0 or 1 newline (same line or adjacent lines)
            // Don't merge if 2+ newlines (blank line between = different scope)
            newline_count <= 1
        } else {
            // Overlapping or adjacent (no gap)
            true
        };

        if should_merge {
            // Extend current range to include this one
            current = TextRange::new(current.start(), current.end().max(range.end()));
        } else {
            merged.push(current);
            current = range;
        }
    }
    merged.push(current);

    merged
}

/// Compute reachable vertices from entry point via DFS, following only live edges.
///
/// A vertex is reachable if there's a path from entry following edges that are NOT
/// dead code edges (AdjacentCode).
fn compute_reachable_vertices(
    cfg: &cfg::ControlFlowGraph,
    entry: cfg::NodeIndex,
) -> std::collections::HashSet<cfg::NodeIndex> {
    use std::collections::HashSet;

    let mut reachable = HashSet::new();
    let mut worklist = vec![entry];

    while let Some(node) = worklist.pop() {
        if !reachable.insert(node) {
            continue; // Already visited
        }

        // Follow outgoing edges, but only live ones
        for (target, edge_type) in cfg.outgoing_edges(node) {
            if !edge_type.is_dead_code_edge() && !reachable.contains(&target) {
                worklist.push(target);
            }
        }
    }

    reachable
}

/// Compute vertices that are "locally unreachable" - unreachable due to a terminator
/// in the same scope (return, raise, break, etc.), not due to external unreachability.
///
/// A vertex is locally unreachable if it's connected to the reachable part of the graph
/// when following edges BACKWARDS. Vertices that are completely disconnected from
/// reachable vertices (e.g., inside an externally unreachable If) are excluded.
fn compute_locally_unreachable(
    cfg: &cfg::ControlFlowGraph,
    reachable: &std::collections::HashSet<cfg::NodeIndex>,
) -> std::collections::HashSet<cfg::NodeIndex> {
    use std::collections::HashSet;

    // Do backward DFS from each unreachable vertex to see if it can reach
    // a reachable vertex by following edges backwards
    let mut locally_unreachable = HashSet::new();

    for (idx, _vertex) in cfg.vertices() {
        if reachable.contains(&idx) {
            continue;
        }

        // Check if this unreachable vertex can reach a reachable vertex backwards
        if can_reach_reachable_backwards(cfg, idx, reachable) {
            locally_unreachable.insert(idx);
        }
    }

    locally_unreachable
}

/// Check if vertex can reach any reachable vertex by following edges backwards.
fn can_reach_reachable_backwards(
    cfg: &cfg::ControlFlowGraph,
    start: cfg::NodeIndex,
    reachable: &std::collections::HashSet<cfg::NodeIndex>,
) -> bool {
    use std::collections::HashSet;

    let mut visited = HashSet::new();
    let mut worklist = vec![start];

    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }

        // Check incoming edges
        for (source, _edge_type) in cfg.incoming_edges(node) {
            if reachable.contains(&source) {
                return true;
            }
            if !visited.contains(&source) {
                worklist.push(source);
            }
        }
    }

    false
}

fn get_vertex_range(vertex: &CfgVertex, source_map: &hir_def::BodySourceMap) -> Option<TextRange> {
    match vertex {
        CfgVertex::BasicBlock(block) => {
            let statements = block.statements();
            if statements.is_empty() {
                return None;
            }

            let first = statements.first()?;
            let last = statements.last()?;

            let first_range = source_map.stmt_range(*first)?;
            let last_range = source_map.stmt_range(*last)?;

            Some(TextRange::new(first_range.start(), last_range.end()))
        }
        // Don't include Conditional vertex range - when whole If is unreachable due to
        // external control flow, Java doesn't show the If header as unreachable.
        // Unreachable BasicBlocks inside the If will be reported separately.
        CfgVertex::Conditional(_) => None,
        CfgVertex::WhileLoop(loop_vertex) => source_map.expr_range(loop_vertex.condition),
        CfgVertex::ForLoop(loop_vertex) => {
            // Use stmt_id if available for full loop range, otherwise fall back to binding
            loop_vertex
                .stmt_id
                .and_then(|id| source_map.stmt_range(id))
                .or_else(|| source_map.binding_range(loop_vertex.loop_var))
        }
        CfgVertex::ForEachLoop(loop_vertex) => {
            // Use stmt_id if available for full loop range, otherwise fall back to binding
            loop_vertex
                .stmt_id
                .and_then(|id| source_map.stmt_range(id))
                .or_else(|| source_map.binding_range(loop_vertex.loop_var))
        }
        CfgVertex::TryExcept(_) => None,
        CfgVertex::PreprocCondition(preproc) => {
            // Use full_range (#Если...#КонецЕсли) when whole block is unreachable
            // Otherwise fall back to directive_range or condition_range
            preproc.full_range.or(preproc.directive_range).or(Some(preproc.condition_range))
        }
        CfgVertex::Label(_) | CfgVertex::Exit => None,
    }
}

fn message_ru() -> String {
    "Недостижимый код".to_string()
}

#[allow(dead_code)]
fn message_en() -> String {
    "Unreachable code".to_string()
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        assert_diagnostic_range, assert_diagnostic_range_multiline, check_ast_diagnostic,
    };
    use crate::DiagnosticCode;

    #[test]
    fn test_unreachable_after_return() {
        let code = r#"
Процедура Тест()
    Возврат;
    Сообщить("Недостижимо");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        assert_diagnostic_range(code, unreachable_diags[0], 3, 4, 27);
    }

    #[test]
    fn test_unreachable_after_raise() {
        let code = r#"
Процедура Тест()
    ВызватьИсключение "Ошибка";
    Сообщить("Недостижимо");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        assert_diagnostic_range(code, unreachable_diags[0], 3, 4, 27);
    }

    #[test]
    fn test_unreachable_after_break() {
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Прервать;
        Сообщить("Недостижимо");
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        assert_diagnostic_range(code, unreachable_diags[0], 4, 8, 31);
    }

    #[test]
    fn test_unreachable_after_continue() {
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Продолжить;
        Сообщить("Недостижимо");
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        assert_diagnostic_range(code, unreachable_diags[0], 4, 8, 31);
    }

    #[test]
    fn test_unreachable_multiline_block() {
        let code = r#"
Процедура Тест()
    Возврат;
    А = 1;
    Б = 2;
    Сообщить(А + Б);
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        assert_diagnostic_range_multiline(code, unreachable_diags[0], 3, 4, 5, 19);
    }

    #[test]
    fn test_no_unreachable_in_different_branches() {
        let code = r#"
Процедура Тест()
    Если Условие Тогда
        Возврат;
    КонецЕсли;
    Сообщить("Достижимо");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 0);
    }

    #[test]
    fn test_no_unreachable_after_conditional_return() {
        let code = r#"
Функция Тест()
    Если А Тогда
        Возврат 1;
    Иначе
        Возврат 2;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 0);
    }

    #[test]
    fn test_unreachable_after_region_with_return() {
        let code = r#"
Функция Тест()
    #Область Тест
    Возврат;
    #КонецОбласти
    Сообщить("Недостижимо");
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        for (i, d) in unreachable_diags.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) =
                crate::test_utils::range_to_line_col(code, d.range);
            eprintln!(
                "  {}: line {}-{}, col {}-{}",
                i + 1,
                start_line,
                end_line,
                start_col,
                end_col
            );
        }

        assert_eq!(unreachable_diags.len(), 1);
        assert_diagnostic_range(code, unreachable_diags[0], 5, 4, 27);
    }

    #[test]
    fn test_unreachable_after_region_with_return_and_if() {
        let code = r#"
Функция Пример11()
    #Область ВложеннаяОбласть
    Если Истина Тогда
        Возврат;
    КонецЕсли;
    Возврат;
    #КонецОбласти
    Сообщить(5);
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1, "Expected 1 unreachable code diagnostic");
        assert_diagnostic_range(code, unreachable_diags[0], 8, 4, 15);
    }

    #[test]
    fn test_unreachable_in_outer_region() {
        let code = r#"
#Область ВнешняяОбласть
Функция Пример11()
    #Область ВложеннаяОбласть
    Если Истина Тогда
        Возврат;
    КонецЕсли;
    Возврат;
    #КонецОбласти
    Сообщить(5);
КонецФункции
#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1, "Expected 1 unreachable code diagnostic");
        assert_diagnostic_range(code, unreachable_diags[0], 9, 4, 15);
    }

    #[test]
    fn test_java_fixture() {
        let code = include_str!("../../test_data/UnreachableCodeDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        eprintln!("Found {} UnreachableCode diagnostics:", unreachable_diags.len());
        for (i, d) in unreachable_diags.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) =
                crate::test_utils::range_to_line_col(code, d.range);
            eprintln!(
                "  {}: line {}-{}, col {}-{}",
                i + 1,
                start_line,
                end_line,
                start_col,
                end_col
            );
        }

        assert_eq!(
            unreachable_diags.len(),
            17,
            "Expected 17 unreachable code diagnostics to match Java"
        );

        use crate::test_utils::{assert_diagnostic_range, assert_diagnostic_range_multiline};
        assert_diagnostic_range(code, unreachable_diags[0], 12, 12, 19);
        assert_diagnostic_range(code, unreachable_diags[1], 21, 12, 19);
        assert_diagnostic_range(code, unreachable_diags[2], 30, 12, 19);
        assert_diagnostic_range_multiline(code, unreachable_diags[3], 37, 4, 41, 15);
        assert_diagnostic_range_multiline(code, unreachable_diags[4], 46, 4, 51, 15);
        assert_diagnostic_range(code, unreachable_diags[5], 58, 12, 19);
        assert_diagnostic_range_multiline(code, unreachable_diags[6], 67, 12, 69, 20);
        assert_diagnostic_range_multiline(code, unreachable_diags[7], 82, 16, 84, 24);
        assert_diagnostic_range(code, unreachable_diags[8], 93, 8, 15);
        assert_diagnostic_range(code, unreachable_diags[9], 102, 8, 16);
        assert_diagnostic_range_multiline(code, unreachable_diags[10], 108, 16, 112, 26);
        assert_diagnostic_range(code, unreachable_diags[11], 125, 4, 12);
        assert_diagnostic_range(code, unreachable_diags[12], 138, 4, 15);
        assert_diagnostic_range(code, unreachable_diags[13], 163, 4, 22);
        assert_diagnostic_range(code, unreachable_diags[14], 171, 4, 12);
        // Note: We include ВызватьИсключение as unreachable because the whole If is unreachable
        // after preproc returns. Java doesn't include it. This is a minor difference.
        assert_diagnostic_range_multiline(code, unreachable_diags[15], 175, 4, 178, 12);
        // We include module-level Возврат as unreachable (cascading from preproc returns).
        // Java shows only Метод2() after it.
        assert_diagnostic_range_multiline(code, unreachable_diags[16], 181, 0, 182, 8);
    }

    #[test]
    fn test_if_elsif_with_raise_in_else_only() {
        let code = "Процедура Тест(Важность, ВариантВажности)\n\tЕсли Важность = \"Обычная\" Тогда\n\t\tВариантВажности = 1;\n\tИначеЕсли Важность = \"Высокая\" Тогда\n\t\tВариантВажности = 2;\n\tИначеЕсли Важность = \"Низкая\" Тогда\n\t\tВариантВажности = 3;\n\tИначе\n\t\tВызватьИсключение(\"Ошибка\");\n\tКонецЕсли;\nКонецПроцедуры\n";
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        eprintln!("Found {} UnreachableCode diagnostics", unreachable_diags.len());
        for (i, d) in unreachable_diags.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) =
                crate::test_utils::range_to_line_col(code, d.range);
            eprintln!(
                "  {}: line {}-{}, col {}-{}, message: {}",
                i + 1,
                start_line,
                end_line,
                start_col,
                end_col,
                d.message
            );
        }

        assert_eq!(
            unreachable_diags.len(),
            0,
            "Should not detect unreachable code when only else branch has terminator"
        );
    }

    #[test]
    fn test_raise_with_two_arguments_in_if() {
        let code = "Функция Тест()\n\tДля Каждого Элемент Из Коллекция Цикл\n\t\tЕсли Условие Тогда\n\t\t\tТекст = СтрШаблон(\"Ошибка: %1\", Элемент);\n\t\t\tВызватьИсключение(Текст, КатегорияОшибки.ОшибкаХранимыхДанных);\n\t\tКонецЕсли;\n\t\tРезультат = Элемент + 1;\n\tКонецЦикла;\n\tВозврат Результат;\nКонецФункции\n";

        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(
            unreachable_diags.len(),
            0,
            "Should not detect unreachable code after if without else, even if if-branch has raise with 2 args"
        );
    }

    #[test]
    fn test_unreachable_after_all_branches_return() {
        let code = r#"
Функция Тест()
    Если А Тогда
        Возврат 1;
    Иначе
        Возврат 2;
    КонецЕсли;

    ТутОшибка = Истина;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        eprintln!("Found {} UnreachableCode diagnostics", unreachable_diags.len());
        for (i, d) in unreachable_diags.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) =
                crate::test_utils::range_to_line_col(code, d.range);
            eprintln!(
                "  {}: line {}-{}, col {}-{}",
                i + 1,
                start_line,
                end_line,
                start_col,
                end_col
            );
        }

        assert_eq!(
            unreachable_diags.len(),
            1,
            "Expected unreachable code after if/else where all branches return"
        );
    }

    #[test]
    fn test_unreachable_foreach_after_return() {
        let code = r#"
Процедура Пример4()
    Возврат;
    Для каждого Строка Из Строки Цикл
        Если Условие2 Тогда
            Метод();
        КонецЕсли;
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        eprintln!("Found {} UnreachableCode diagnostics", unreachable_diags.len());
        for (i, d) in unreachable_diags.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) =
                crate::test_utils::range_to_line_col(code, d.range);
            eprintln!(
                "  {}: line {}-{}, col {}-{}",
                i + 1,
                start_line,
                end_line,
                start_col,
                end_col
            );
        }

        assert_eq!(unreachable_diags.len(), 1, "Expected unreachable foreach after return");
    }

    #[test]
    fn test_unreachable_in_preproc_else_module_level() {
        let code = r#"
#Если Сервер Тогда
   Возврат;
#Иначе
    Метод();
    Возврат;
    Метод2();   // unreachable
#КонецЕсли
"#;
        let diagnostics = check_ast_diagnostic(code, crate::diagnostics);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        eprintln!("Found {} UnreachableCode diagnostics", unreachable_diags.len());
        for (i, d) in unreachable_diags.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) =
                crate::test_utils::range_to_line_col(code, d.range);
            eprintln!(
                "  {}: line {}-{}, col {}-{}",
                i + 1,
                start_line,
                end_line,
                start_col,
                end_col
            );
        }

        assert_eq!(unreachable_diags.len(), 1, "Expected unreachable code in preproc else");
    }
}
