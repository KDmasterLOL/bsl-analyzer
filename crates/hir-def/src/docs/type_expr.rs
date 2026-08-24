use super::{ParameterDoc, TypeDoc};
use crate::{Name, QualifiedName, TypeRef};
use stdx::case::CaseExt;

/// A type expression retained from structured documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocTypeExpr {
    /// A regular type that can use the existing type-reference representation.
    TypeRef(TypeRef),
    /// A qualified documentation cross-reference.
    See(QualifiedName),
    /// A structure with its directly documented fields.
    Structure {
        /// The fields declared by one-level documentation bullets.
        fields: Vec<DocField>,
    },
    /// An array with a documented element expression.
    Array(Box<DocTypeExpr>),
}

/// A direct field declared in a documented structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocField {
    /// The documented field name.
    pub name: String,
    /// The alternatives documented for the field.
    pub types: Vec<DocTypeExpr>,
}

impl DocTypeExpr {
    /// Whether the expression names another method's documentation somewhere inside it.
    ///
    /// Two segments name a method and mean its returned value; three name one of that method's
    /// parameters. One segment is prose — `см. в`, `см. также`, `см. описание` all parse into a
    /// single-segment reference, and treating those as cross-references would claim a target for
    /// tens of thousands of slots that never had one.
    ///
    /// The search is recursive because a reference is not only ever the whole slot:
    /// `Массив из см. Модуль.Метод` puts it under an array, and `* Поле - см. Модуль.Метод` puts
    /// it in a field of a documented structure.
    pub fn names_documentation_target(&self) -> bool {
        match self {
            Self::See(name) => matches!(name.segments().len(), 2 | 3),
            Self::Array(element) => element.names_documentation_target(),
            Self::Structure { fields } => {
                fields.iter().any(|field| field.types.iter().any(Self::names_documentation_target))
            }
            Self::TypeRef(_) => false,
        }
    }
}

/// Parses a documentation type slot into the rich expression preserved by `hir-def`.
pub fn parse_type_expr(type_doc: &TypeDoc) -> Option<DocTypeExpr> {
    if type_doc.is_hyperlink || is_see_candidate(&type_doc.name) {
        return parse_see_reference(&type_doc.name).map(DocTypeExpr::See);
    }

    parse_non_hyperlink_type(&type_doc.name, type_doc.description.as_deref(), &type_doc.parameters)
}

fn parse_non_hyperlink_type(
    name: &str,
    description: Option<&str>,
    fields: &[ParameterDoc],
) -> Option<DocTypeExpr> {
    if is_see_candidate(name) {
        return parse_see_reference(name).map(DocTypeExpr::See);
    }

    if let Some(element) = collection_element(name, description) {
        return parse_non_hyperlink_type(element, None, fields)
            .map(Box::new)
            .map(DocTypeExpr::Array);
    }

    let name = name.trim().trim_end_matches(':').trim();
    // `Произвольный` is a declaration of no constraint, and it has to survive as the top type: the
    // kernel drops `Unknown` from a union (`T | Unknown == T`), so lowering this word as an
    // unrecognised name would silently narrow the slot to whatever was documented beside it. The
    // other doc-type parser has always spelled this out (`ty/doc_types.rs`); a slot rebuilt through
    // this one has to agree, or the two disagree exactly where permissiveness is at stake.
    if matches!(name.fold_lower().as_str(), "произвольный" | "any" | "arbitrary") {
        return Some(DocTypeExpr::TypeRef(TypeRef::Any));
    }
    // `Структура из КлючИЗначение` names a structure whose values share a type. The kernel has no
    // place for that value type, so the tail is dropped and the slot stays the structure its
    // bullets describe — the return path already arrives here with the tail moved into the
    // description, and a parameter keeps it in the name.
    if is_structure_name(name) || collection_head(name).is_some_and(is_structure_name) {
        return Some(DocTypeExpr::Structure { fields: parse_fields(fields) });
    }

    parse_qualified_name(name)
        .map(|qualified_name| {
            TypeRef::from_bare_name(name).unwrap_or(TypeRef::Name(qualified_name))
        })
        .map(DocTypeExpr::TypeRef)
}

fn parse_fields(fields: &[ParameterDoc]) -> Vec<DocField> {
    fields
        .iter()
        .map(|field| DocField {
            name: field.name.clone(),
            types: field.types.iter().filter_map(parse_type_expr).collect(),
        })
        .collect()
}

fn collection_element<'a>(name: &'a str, description: Option<&'a str>) -> Option<&'a str> {
    collection_element_from_name(name).or_else(|| {
        is_array_name(name)
            .then(|| description.and_then(collection_element_from_description))
            .flatten()
    })
}

fn collection_element_from_name(name: &str) -> Option<&str> {
    for marker in [" из ", " of "] {
        let Some(at) = stdx::case::find_ignore_case(name, marker) else {
            continue;
        };
        if is_array_name(&name[..at.start]) {
            return declaration_fragment(&name[at.end..]);
        }
    }
    None
}

fn collection_element_from_description(description: &str) -> Option<&str> {
    for prefix in ["из ", "of "] {
        match stdx::case::find_ignore_case(description, prefix) {
            Some(at) if at.start == 0 => return declaration_fragment(&description[at.end..]),
            _ => continue,
        }
    }
    None
}

fn declaration_fragment(text: &str) -> Option<&str> {
    let fragment = text.split_once(" - ").map_or(text, |(fragment, _)| fragment);
    let fragment = fragment.trim().trim_end_matches(':').trim();
    (!fragment.is_empty()).then_some(fragment)
}

/// The collection name in front of an `из` / `of` tail, if the name carries one. The marker is
/// matched in either language regardless of the head's language: documentation mixes them.
fn collection_head(name: &str) -> Option<&str> {
    [" из ", " of "].iter().find_map(|marker| {
        stdx::case::find_ignore_case(name, marker).map(|at| name[..at.start].trim())
    })
}

fn is_array_name(name: &str) -> bool {
    matches!(name.trim().fold_lower().as_str(), "массив" | "array")
}

fn is_structure_name(name: &str) -> bool {
    matches!(name.fold_lower().as_str(), "структура" | "structure")
}

fn is_see_candidate(name: &str) -> bool {
    let name = name.trim();
    name.fold_lower().starts_with("см.")
        || name
            .fold_lower()
            .strip_prefix("see")
            .is_some_and(|tail| tail.chars().next().is_some_and(char::is_whitespace))
}

fn parse_see_reference(name: &str) -> Option<QualifiedName> {
    let name = name.trim();
    let tail = strip_prefix_ci(name, "см.").or_else(|| strip_prefix_ci(name, "see"))?;
    let target = tail.trim_start();
    if target.len() == tail.len() {
        return None;
    }
    parse_qualified_name(target.strip_suffix('.').unwrap_or(target))
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    if !text.fold_lower().starts_with(&prefix.fold_lower()) {
        return None;
    }
    let prefix_end =
        text.char_indices().nth(prefix.chars().count()).map_or(text.len(), |(index, _)| index);
    Some(&text[prefix_end..])
}

fn parse_qualified_name(name: &str) -> Option<QualifiedName> {
    let segments = name.split('.').map(str::trim).collect::<Vec<_>>();
    (!segments.is_empty() && segments.iter().all(|segment| is_identifier_like(segment)))
        .then(|| QualifiedName::from_segments(segments.into_iter().map(Name::new)))
}

fn is_identifier_like(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(first) if first.is_alphabetic() || first == '_')
        && chars.all(|character| character.is_alphanumeric() || character == '_')
}

#[cfg(test)]
#[path = "type_expr_tests.rs"]
mod tests;
