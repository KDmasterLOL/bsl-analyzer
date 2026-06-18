use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Interception, SignatureMismatch, SymbolTree};
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

/// Validate every `&Вместо` / `&Перед` / `&После` interceptor in `ctx`'s extension module
/// against the signature of the base method it weaves onto. The base method's symbols come
/// from `base_symbols` (the paired base module's [`SymbolTree`]). When a base method does
/// not resolve at all, no signature diagnostic is produced — that case belongs to
/// unresolved-reference diagnostics, not signature equivalence.
pub fn check(ctx: &DiagnosticsContext, base_symbols: &SymbolTree) -> Vec<Diagnostic> {
    let code = DiagnosticCode::WeavingSignatureMismatch;
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
        let Some(mismatch) = hir::signature_mismatch(interception.kind, method, base_method) else {
            continue;
        };

        let range = method_name_range(&node).unwrap_or(method.source_range);
        diagnostics.push(Diagnostic {
            code,
            message: format_message(&interception, &mismatch),
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

fn format_message(interception: &Interception, mismatch: &SignatureMismatch) -> String {
    let target = &interception.target;
    match mismatch {
        SignatureMismatch::ParamCount { base, interceptor } => format!(
            "Сигнатура перехватчика не совпадает с расширяемым методом «{target}»: \
             в расширяемом методе параметров — {base}, в перехватчике — {interceptor}"
        ),
        SignatureMismatch::ByVal { param, base_is_val, .. } => {
            let expected = if *base_is_val {
                "со словом Знач"
            } else {
                "без слова Знач"
            };
            format!(
                "Параметр «{param}» перехватчика не совпадает с расширяемым методом «{target}» \
                 по способу передачи: в расширяемом методе он объявлен {expected}"
            )
        }
        SignatureMismatch::MethodKind { base_is_function } => {
            let expected =
                if *base_is_function { "функцией" } else { "процедурой" };
            format!(
                "Перехватчик «&Вместо» для «{target}» должен быть {expected}, \
                 как и расширяемый метод"
            )
        }
    }
}
