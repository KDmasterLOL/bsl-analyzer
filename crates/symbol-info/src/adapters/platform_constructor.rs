use bsl_platform::{platform_constructors_query, PlatformDataInner, TypeNameInput};
use ide_db::RootDatabase;
use smol_str::SmolStr;

use crate::adapters::params::build_constructor_params;
use crate::domain::{MethodKind, SignatureSource, SymbolSignature, TypeRef};

pub(super) fn build(db: &dyn RootDatabase, type_name: &str) -> Option<Vec<SymbolSignature>> {
    let input = TypeNameInput::new(db, type_name.to_string());
    let ctors = platform_constructors_query(db, input);
    if ctors.is_empty() {
        return None;
    }

    let mut signatures = Vec::with_capacity(ctors.len());
    for ctor in ctors.iter() {
        let docs = PlatformDataInner::instance().get_constructor_docs(ctor.id);
        let name_russian = if let Some(variant) = &ctor.variant_name {
            SmolStr::from(format!("{}.{}", type_name, variant))
        } else {
            SmolStr::new(type_name)
        };

        signatures.push(SymbolSignature {
            candidate_ordinal: None,
            platform_id: Some(ctor.id),
            kind: MethodKind::Function,
            name_russian,
            name_english: Some(ctor.type_name.clone()),
            qualifier: None,
            prefix: Some("Новый ".into()),
            params: build_constructor_params(&ctor.parameters, docs.as_ref()),
            returns: vec![TypeRef {
                russian: SmolStr::new(type_name),
                english: Some(ctor.type_name.clone()),
                description: None,
                is_hyperlink: false,
            }],
            purpose: docs.as_ref().map(|d| d.description.clone()).filter(|s| !s.is_empty()),
            description: docs.as_ref().map(|d| d.description.clone()).filter(|s| !s.is_empty()),
            examples: docs
                .as_ref()
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
            notes: docs.as_ref().and_then(|d| d.notes.clone()),
            deprecation: None,
            is_export: true,
            source: SignatureSource::PlatformConstructor,
            method_id: None,
        });
    }

    Some(signatures)
}
