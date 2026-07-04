use bsl_platform::{platform_method_query, MethodLookupInput, PlatformDataInner, PlatformMethod};
use ide_db::RootDatabase;
use smol_str::SmolStr;

use crate::adapters::params::build_platform_params;
use crate::domain::{MethodKind, SignatureSource, SymbolSignature, TypeRef};

pub(super) fn build(
    db: &dyn RootDatabase,
    type_name: &str,
    method_name: &str,
) -> Option<SymbolSignature> {
    let input = MethodLookupInput::new(db, type_name.to_string(), method_name.to_string());
    let method = platform_method_query(db, input)?;
    let docs = PlatformDataInner::instance().get_method_docs(method.id);
    Some(from_platform_method(method, docs.as_ref()))
}

pub fn from_platform_method(
    method: &PlatformMethod,
    docs: Option<&bsl_platform::MethodDocs>,
) -> SymbolSignature {
    let kind =
        if method.return_type.is_some() { MethodKind::Function } else { MethodKind::Procedure };

    let display_name = docs
        .and_then(|d| d.syntax.split('(').next())
        .filter(|n| !n.is_empty() && !n.starts_with('<'))
        .map(SmolStr::new)
        .unwrap_or_else(|| method.name.clone());

    let english_name = method
        .english_name
        .rsplit_once('.')
        .map(|(_, n)| SmolStr::new(n))
        .unwrap_or_else(|| method.english_name.clone());

    let returns: Vec<TypeRef> = method
        .return_type
        .as_ref()
        .map(|r| {
            vec![TypeRef {
                russian: r.clone(),
                english: None,
                description: None,
                is_hyperlink: false,
            }]
        })
        .unwrap_or_default();

    let russian_type_name = PlatformDataInner::instance()
        .get_type(&method.type_name)
        .map(|ty| ty.name.clone())
        .unwrap_or_else(|| method.type_name.clone());

    SymbolSignature {
        kind,
        name_russian: display_name,
        name_english: Some(english_name),
        qualifier: Some(SmolStr::from(format!("{}.", russian_type_name))),
        prefix: None,
        params: build_platform_params(&method.parameters, docs),
        returns,
        purpose: docs.map(|d| d.description.clone()).filter(|s| !s.is_empty()),
        description: docs.map(|d| d.description.clone()).filter(|s| !s.is_empty()),
        examples: docs
            .map(|d| {
                d.examples
                    .iter()
                    .map(|e| crate::domain::CodeExample {
                        code: e.code.clone(),
                        description: e.description.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        notes: docs.and_then(|d| d.notes.clone()),
        deprecation: None,
        is_export: true,
        source: SignatureSource::Platform,
        method_id: None,
    }
}
