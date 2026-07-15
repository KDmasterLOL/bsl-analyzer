use bsl_platform::{GlobalFunction, MethodDocs, PlatformMethod};
use symbol_info::{from_global_function, from_platform_method, SignatureSource};

#[test]
fn platform_method_adapter_keeps_deprecation_outside_source_docs() {
    // Given: platform docs can contain lifecycle-looking prose, but they are not source comments.
    let method = PlatformMethod {
        id: 900_001,
        type_name: "ТестовыйТип".into(),
        name: "УстаревшийМетод".into(),
        english_name: "TestType.DeprecatedMethod".into(),
        return_type: None,
        parameters: Vec::new(),
        variants: Vec::new(),
        min_version: None,
        context: None,
    };
    let docs = MethodDocs {
        method_id: method.id,
        syntax: "УстаревшийМетод()".to_string(),
        description: "Deprecated. Use ReplacementMethod().".to_string(),
        params: Vec::new(),
        examples: Vec::new(),
        notes: Some("Устарела. Используйте НовыйМетод().".to_string()),
        see_also: Vec::new(),
    };

    // When: symbol-info builds a platform method signature.first().expect("at least one signature").
    let signature = from_platform_method(&method, Some(&docs));

    // Then: platform prose stays descriptive; source-doc deprecation remains absent.
    assert_eq!(
        signature.first().expect("at least one signature").source,
        SignatureSource::Platform
    );
    assert_eq!(
        signature.first().expect("at least one signature").description.as_deref(),
        Some("Deprecated. Use ReplacementMethod().")
    );
    assert_eq!(
        signature.first().expect("at least one signature").notes.as_deref(),
        Some("Устарела. Используйте НовыйМетод().")
    );
    assert!(signature.first().expect("at least one signature").deprecation.is_none());
}

#[test]
fn global_function_adapter_keeps_deprecation_outside_source_docs() {
    // Given: global-function platform docs can mention deprecation-like text.
    let function = GlobalFunction {
        id: 900_002,
        name: "УстаревшаяФункция".into(),
        english_name: "DeprecatedFunction".into(),
        return_type: Some("Строка".into()),
        parameters: Vec::new(),
        variants: Vec::new(),
        min_version: None,
        context: None,
    };
    let docs = MethodDocs {
        method_id: function.id,
        syntax: "УстаревшаяФункция()".to_string(),
        description: "Устарела. Используйте НоваяФункция().".to_string(),
        params: Vec::new(),
        examples: Vec::new(),
        notes: Some("Deprecated. Use NewFunction().".to_string()),
        see_also: Vec::new(),
    };

    // When: symbol-info builds a global function signature.first().expect("at least one signature").
    let signature = from_global_function(&function, Some(&docs));

    // Then: platform docs do not enter the source-comment deprecation slot.
    assert_eq!(
        signature.first().expect("at least one signature").source,
        SignatureSource::GlobalFunction
    );
    assert_eq!(
        signature.first().expect("at least one signature").description.as_deref(),
        Some("Устарела. Используйте НоваяФункция().")
    );
    assert_eq!(
        signature.first().expect("at least one signature").notes.as_deref(),
        Some("Deprecated. Use NewFunction().")
    );
    assert!(signature.first().expect("at least one signature").deprecation.is_none());
}
