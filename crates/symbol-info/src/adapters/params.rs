use bsl_platform::{
    ConstructorDocs as PlatformConstructorDocs, MethodDocs as PlatformMethodDocs, MethodParam,
};
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
                default_value: printable_default(p.default_value.clone()),
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
    build_from_param_docs(items, docs.map(|d| d.params.as_slice()))
}

pub(super) fn build_constructor_params(
    items: &[MethodParam],
    docs: Option<&PlatformConstructorDocs>,
) -> Vec<SignatureParam> {
    build_from_param_docs(items, docs.map(|d| d.params.as_slice()))
}

fn build_from_param_docs(
    items: &[MethodParam],
    param_docs: Option<&[bsl_platform::ParamDocs]>,
) -> Vec<SignatureParam> {
    items
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let pdoc = param_docs.and_then(|d| d.get(i));
            let (types_from_desc, clean_desc) =
                pdoc.map(|d| split_platform_param_description(&d.description)).unwrap_or_default();
            let types: Vec<TypeRef> = if !types_from_desc.is_empty() {
                types_from_desc
                    .into_iter()
                    .map(|t| TypeRef {
                        russian: t.into(),
                        english: None,
                        description: None,
                        is_hyperlink: false,
                    })
                    .collect()
            } else {
                match &p.param_type {
                    Some(t) => vec![TypeRef {
                        russian: t.clone(),
                        english: None,
                        description: None,
                        is_hyperlink: false,
                    }],
                    None => Vec::new(),
                }
            };
            SignatureParam {
                name: p.name.clone(),
                types,
                is_optional: p.is_optional,
                default_value: printable_default(
                    pdoc.and_then(|d| d.default_value.as_deref().map(SmolStr::new)),
                ),
                description: if clean_desc.is_empty() { None } else { Some(clean_desc) },
                is_val: false,
            }
        })
        .collect()
}

fn split_platform_param_description(s: &str) -> (Vec<String>, String) {
    let trimmed = s.trim_start();
    let Some(after_marker) = trimmed.strip_prefix("Тип:") else {
        return (Vec::new(), s.to_string());
    };
    let after_marker = after_marker.trim_start();
    let Some(dot_pos) = after_marker.find('.') else {
        return (Vec::new(), s.to_string());
    };
    let types_part = after_marker[..dot_pos].trim();
    let rest = after_marker[dot_pos + 1..].trim_start();
    let types: Vec<String> = types_part
        .split([',', ';'])
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    (types, rest.to_string())
}

fn find_param_doc<'a>(name: &str, docs: Option<&'a UserMethodDocs>) -> Option<&'a ParameterDoc> {
    let target = name.to_lowercase();
    docs?.parameters.iter().find(|pd| pd.name.to_lowercase() == target)
}

/// The default-value text a declaration can actually be rendered with.
///
/// An optional parameter whose default expression could not be read carries an EMPTY text,
/// not an absent one: the parser builds an expression node after every `=`, so the node is
/// there and holds nothing. Printed as-is that becomes `Имя = ` with a dangling equals sign,
/// so the empty text is dropped here, once, rather than guarded in every presenter that
/// renders a parameter. Optionality is untouched — it is `has_default`'s answer, not this
/// field's.
fn printable_default(text: Option<SmolStr>) -> Option<SmolStr> {
    text.filter(|value| !value.trim().is_empty())
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

#[cfg(test)]
mod tests {
    use super::build_user_params;
    use hir::Param;

    /// The parser builds an expression node after every `=`, so a default it could not read
    /// arrives as EMPTY text rather than as no text. Carried through as-is, that empty text
    /// renders as `Имя = ` — a declaration with a dangling equals sign, which is not what the
    /// file says. The parameter stays optional; only the unreadable text is dropped.
    #[test]
    fn an_unreadable_default_leaves_the_parameter_optional_without_a_text() {
        let param = Param {
            name: hir::Name::new("А"),
            is_val: true,
            has_default: true,
            default_value: Some(smol_str::SmolStr::new("")),
            name_range: syntax::TextRange::new(0.into(), 1.into()),
        };

        let built = build_user_params(std::slice::from_ref(&param), None);

        assert!(built[0].is_optional, "`=` in the declaration makes it optional");
        assert_eq!(built[0].default_value, None, "no text is better than an empty one");
    }
}
