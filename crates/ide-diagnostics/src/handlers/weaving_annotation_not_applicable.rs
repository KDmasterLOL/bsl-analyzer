use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Interception, InterceptionKind, SymbolTree};
use syntax::{ast, ast::AstNode, SyntaxKind, SyntaxNode, TextRange};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Report `&Перед` / `&После` interceptors that target an extended *function*: the platform
/// allows a function to be extended only with `&Вместо`. The base method's kind comes from
/// `base_symbols` (the paired base module). When the base method does not resolve, no
/// diagnostic is produced — that is an unresolved-reference case, not an applicability one.
pub fn check(ctx: &DiagnosticsContext, base_symbols: &SymbolTree) -> Vec<Diagnostic> {
    let code = DiagnosticCode::WeavingAnnotationNotApplicable;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let ext_symbols = ctx.symbol_tree();
    let mut diagnostics = Vec::new();

    for method in ext_symbols.methods() {
        let Some(node) = method.syntax_node(&parse) else {
            continue;
        };
        let Some(interception) = hir::interceptor_target(&node) else {
            continue;
        };
        let Some(base_method) = base_symbols.find_method(&hir::Name::new(&interception.target))
        else {
            continue;
        };
        if hir::interception_applicable(interception.kind, base_method) {
            continue;
        }

        let range = method_name_range(&node).unwrap_or(method.source_range);
        diagnostics.push(Diagnostic {
            code,
            message: format_message(&interception),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    diagnostics
}

/// The name-identifier range of a procedure/function node, for placing the diagnostic on
/// the interceptor's name rather than its whole body.
fn method_name_range(node: &SyntaxNode) -> Option<TextRange> {
    match node.kind() {
        SyntaxKind::FUNCTION_DEF => {
            ast::FunctionDef::cast(node.clone())?.name().map(|n| n.text_range())
        }
        SyntaxKind::PROCEDURE_DEF => {
            ast::ProcedureDef::cast(node.clone())?.name().map(|n| n.text_range())
        }
        _ => None,
    }
}

fn format_message(interception: &Interception) -> String {
    let annotation = match interception.kind {
        InterceptionKind::Before => "&Перед",
        InterceptionKind::After => "&После",
        // `interception_applicable` only rejects Before/After, so Around never reaches here.
        InterceptionKind::Around => "&Вместо",
    };
    let target = &interception.target;
    format!(
        "Аннотация «{annotation}» неприменима к функции «{target}»: \
         функцию можно расширить только аннотацией «&Вместо»"
    )
}
