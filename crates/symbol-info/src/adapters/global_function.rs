use bsl_platform::{
    global_function_query, GlobalFunction, MethodDocs, PlatformDataInner, TypeNameInput,
};
use ide_db::RootDatabase;

use crate::adapters::params::build_platform_params;
use crate::domain::{CodeExample, MethodKind, SignatureSource, SymbolSignature, TypeRef};

pub(super) fn build(db: &dyn RootDatabase, name: &str) -> Option<SymbolSignature> {
    let input = TypeNameInput::new(db, name.to_string());
    let function = global_function_query(db, input)?;
    let docs = PlatformDataInner::instance().get_global_function_docs(function.id);
    Some(from_global_function(function, docs.as_ref()))
}

pub fn from_global_function(
    function: &GlobalFunction,
    docs: Option<&MethodDocs>,
) -> SymbolSignature {
    let kind =
        if function.return_type.is_some() { MethodKind::Function } else { MethodKind::Procedure };

    let returns: Vec<TypeRef> = function
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

    SymbolSignature {
        kind,
        name_russian: function.name.clone(),
        name_english: Some(function.english_name.clone()),
        qualifier: None,
        prefix: None,
        params: build_platform_params(&function.parameters, docs),
        returns,
        purpose: docs.map(|d| d.description.clone()).filter(|s| !s.is_empty()),
        description: docs.map(|d| d.description.clone()).filter(|s| !s.is_empty()),
        examples: docs
            .map(|d| {
                d.examples
                    .iter()
                    .map(|e| CodeExample {
                        code: e.code.clone(),
                        description: e.description.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        notes: docs.and_then(|d| d.notes.clone()),
        deprecation: None,
        is_export: true,
        source: SignatureSource::GlobalFunction,
        method_id: None,
    }
}
