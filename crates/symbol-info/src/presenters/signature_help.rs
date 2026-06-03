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
