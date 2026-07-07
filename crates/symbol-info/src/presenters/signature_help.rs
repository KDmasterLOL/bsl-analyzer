use crate::domain::{MethodKind, SignatureParam, SignatureSource, SymbolSignature, TypeRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelpView {
    pub signature: String,
    pub doc: Option<String>,
    pub active_parameter: Option<usize>,
    pub parameters: Vec<ParameterInfoView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterInfoView {
    pub label: String,
    pub documentation: Option<String>,
}

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

/// The real BSL declaration line for a user method: `Функция Имя(Знач П = Умолч) Экспорт`.
///
/// Unlike [`render_signature_help`], this omits parameter type annotations and the synthetic
/// `: <ReturnType>` suffix (the return type is surfaced separately), keeps the `Знач`
/// by-value marker, and renders optional parameters as `= <default>` rather than the
/// signature-help `[…]` styling — so the string reads as source, not as an editor tooltip.
pub fn render_declaration(sig: &SymbolSignature) -> String {
    let header = declaration_header(sig);
    let params: Vec<String> = sig.params.iter().map(declaration_param).collect();
    let mut signature = format!("{}({})", header, params.join(", "));
    if sig.is_export {
        signature.push_str(" Экспорт");
    }
    signature
}

/// The declaration keyword + name, WITHOUT the call qualifier `make_header` prepends: a
/// definition-site declaration reads `Функция Имя(…)`, not `Функция Модуль.Имя(…)` — the owning
/// module is surfaced separately (the container), and the qualifier is not valid declaration
/// syntax. Platform members keep the bare name (no `Функция`/`Процедура` keyword).
fn declaration_header(sig: &SymbolSignature) -> String {
    let prefix = sig.prefix.as_deref().unwrap_or("");
    let core = format!("{}{}", prefix, sig.name_russian);
    match sig.source {
        SignatureSource::Platform
        | SignatureSource::PlatformManager
        | SignatureSource::PlatformConstructor => core,
        _ => match sig.kind {
            MethodKind::Procedure => format!("Процедура {}", core),
            MethodKind::Function => format!("Функция {}", core),
        },
    }
}

fn declaration_param(p: &SignatureParam) -> String {
    let mut out = String::new();
    if p.is_val {
        out.push_str("Знач ");
    }
    out.push_str(&p.name);
    if let Some(default) = &p.default_value {
        out.push_str(" = ");
        out.push_str(default);
    }
    out
}

fn make_header(sig: &SymbolSignature) -> String {
    let prefix = sig.prefix.as_deref().unwrap_or("");
    let qualifier = sig.qualifier.as_deref().unwrap_or("");
    let core = format!("{}{}{}", prefix, qualifier, sig.name_russian);
    match sig.source {
        SignatureSource::Platform
        | SignatureSource::PlatformManager
        | SignatureSource::PlatformConstructor => core,
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
