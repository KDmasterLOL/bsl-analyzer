//! Renders a [`SymbolSignature`] into a SignatureHelp-shaped view-model.
//!
//! Output mirrors LSP `SignatureHelp` semantics but without `lsp_types`
//! dependency. The IDE crate maps it to `lsp_types::SignatureHelp`.

use crate::domain::{MethodKind, SignatureParam, SignatureSource, SymbolSignature, TypeRef};

/// Domain-only mirror of `lsp_types::SignatureHelp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelpView {
    /// Full signature label, e.g. `"Функция МояФункция(Параметр1, Параметр2): Строка"`.
    pub signature: String,
    /// Top-level documentation (the method `purpose` line).
    pub doc: Option<String>,
    /// Index of the active parameter (0-based), if the cursor sits on one.
    pub active_parameter: Option<usize>,
    pub parameters: Vec<ParameterInfoView>,
}

/// Domain-only mirror of `lsp_types::ParameterInformation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterInfoView {
    pub label: String,
    pub documentation: Option<String>,
}

/// Render a signature into the view-model.
pub fn render_signature_help(sig: &SymbolSignature, active_param: usize) -> SignatureHelpView {
    let header = make_header(sig);
    let param_strs: Vec<String> = sig.params.iter().map(format_param).collect();

    let mut signature = format!("{}({})", header, param_strs.join(", "));
    if matches!(sig.kind, MethodKind::Function) && !sig.returns.is_empty() {
        signature.push_str(": ");
        signature.push_str(&join_types(&sig.returns));
    }

    let parameters: Vec<ParameterInfoView> = sig
        .params
        .iter()
        .zip(param_strs)
        .map(|(p, label)| ParameterInfoView { label, documentation: p.description.clone() })
        .collect();

    let active_parameter = if active_param < parameters.len() { Some(active_param) } else { None };

    SignatureHelpView { signature, doc: sig.purpose.clone(), active_parameter, parameters }
}

fn make_header(sig: &SymbolSignature) -> String {
    let qualifier = sig.qualifier.as_deref().unwrap_or("");
    let core = format!("{}{}", qualifier, sig.name_russian);
    match sig.source {
        // Platform methods read more naturally without `Процедура`/`Функция`,
        // since the qualifier already names the receiver type or collection.
        SignatureSource::Platform | SignatureSource::PlatformManager => core,
        _ => match sig.kind {
            MethodKind::Procedure => format!("Процедура {}", core),
            MethodKind::Function => format!("Функция {}", core),
        },
    }
}

fn format_param(p: &SignatureParam) -> String {
    let inner = if p.types.is_empty() {
        p.name.to_string()
    } else {
        format!("{}: {}", p.name, join_types(&p.types))
    };
    let with_default = match (p.is_optional, &p.default_value) {
        (true, Some(v)) => format!("{} = {}", inner, v),
        _ => inner,
    };
    if p.is_optional {
        format!("[{}]", with_default)
    } else {
        with_default
    }
}

fn join_types(types: &[TypeRef]) -> String {
    types.iter().map(|t| t.russian.as_str()).collect::<Vec<_>>().join(" | ")
}
