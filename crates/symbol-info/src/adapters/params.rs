//! Helpers that map source-specific parameter shapes into [`SignatureParam`].
//!
//! Two flavours: user-defined (HIR `Param` + parsed `MethodDocs`) and
//! platform (`MethodParam` + platform `MethodDocs`). Both funnel into the same
//! domain shape so presenters do not branch on source.

use bsl_platform::{MethodDocs as PlatformMethodDocs, MethodParam};
use hir::{MethodDocs as UserMethodDocs, Param, ParameterDoc};
use hir_def::docs::TypeDoc;
use smol_str::SmolStr;

use crate::domain::{SignatureParam, TypeRef};

pub(super) fn build_user_params(
    params: &[Param],
    docs: Option<&UserMethodDocs>,
) -> Vec<SignatureParam> {
    params
        .iter()
        .map(|p| {
            let name_str = p.name.as_str();
            let doc = find_param_doc(name_str, docs);
            let types =
                doc.map(|d| d.types.iter().map(user_type_to_ref).collect()).unwrap_or_default();
            SignatureParam {
                name: SmolStr::new(name_str),
                types,
                is_optional: p.has_default,
                default_value: None,
                description: doc.and_then(joined_descriptions),
                is_val: p.is_val,
            }
        })
        .collect()
}

pub(super) fn build_platform_params(
    items: &[MethodParam],
    docs: Option<&PlatformMethodDocs>,
) -> Vec<SignatureParam> {
    items
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let pdoc = docs.and_then(|d| d.params.get(i));
            let types = match &p.param_type {
                Some(t) => vec![TypeRef {
                    russian: t.clone(),
                    english: None,
                    description: None,
                    is_hyperlink: false,
                }],
                None => Vec::new(),
            };
            SignatureParam {
                name: p.name.clone(),
                types,
                is_optional: p.is_optional,
                default_value: pdoc.and_then(|d| d.default_value.as_deref().map(SmolStr::new)),
                description: pdoc.and_then(|d| {
                    if d.description.is_empty() {
                        None
                    } else {
                        Some(d.description.clone())
                    }
                }),
                is_val: false,
            }
        })
        .collect()
}

fn find_param_doc<'a>(name: &str, docs: Option<&'a UserMethodDocs>) -> Option<&'a ParameterDoc> {
    let target = name.to_lowercase();
    docs?.parameters.iter().find(|pd| pd.name.to_lowercase() == target)
}

fn joined_descriptions(doc: &ParameterDoc) -> Option<String> {
    let descs: Vec<&str> = doc.types.iter().filter_map(|t| t.description.as_deref()).collect();
    if descs.is_empty() {
        None
    } else {
        Some(descs.join("\n\n"))
    }
}

pub(super) fn user_type_to_ref(t: &TypeDoc) -> TypeRef {
    TypeRef {
        russian: SmolStr::new(&t.name),
        english: None,
        description: t.description.clone(),
        is_hyperlink: t.is_hyperlink,
    }
}
