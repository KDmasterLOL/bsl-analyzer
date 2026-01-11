//! Platform data database with efficient indexing.
//!
//! This module provides both:
//! - Salsa tracked queries for IDE integration with caching
//! - A singleton for standalone usage

use crate::types::{GlobalFunction, PlatformMethod, PlatformType};
use once_cell::sync::OnceCell;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::sync::Arc;

static PLATFORM_DATA_SINGLETON: OnceCell<PlatformDataInner> = OnceCell::new();

/// Internal platform data implementation.
///
/// This struct is not public API. Access it through:
/// - Salsa tracked queries (`platform_type_query`, `platform_method_query`, `global_function_query`)
/// - `PlatformDataInner::instance()` for standalone usage
pub struct PlatformDataInner {
    /// Converted platform types (from raw const data)
    types: Vec<PlatformType>,
    /// Types indexed by lowercase name (both Russian and English)
    types_by_name: FxHashMap<SmolStr, usize>,
    /// Converted platform methods (from raw const data)
    methods: Vec<PlatformMethod>,
    /// Methods indexed by (type_name, method_name)
    methods_by_name: FxHashMap<(SmolStr, SmolStr), usize>,
    /// Converted global functions (from raw const data)
    global_functions: Vec<GlobalFunction>,
    /// Global functions indexed by lowercase name (both Russian and English)
    global_functions_by_name: FxHashMap<SmolStr, usize>,
    /// Method documentation indexed by method ID
    method_docs_by_id: FxHashMap<u32, usize>,
    /// Global function documentation indexed by function ID
    global_function_docs_by_id: FxHashMap<u32, usize>,
}

impl PlatformDataInner {
    /// Get the global platform data instance.
    pub fn instance() -> &'static Self {
        PLATFORM_DATA_SINGLETON.get_or_init(Self::new)
    }

    /// Initialize platform data and build indices.
    fn new() -> Self {
        // Convert raw types to SmolStr-based types
        let types: Vec<PlatformType> =
            crate::generated::PLATFORM_TYPES.iter().map(PlatformType::from).collect();

        let mut types_by_name = FxHashMap::default();

        // Index platform types
        for (idx, ty) in types.iter().enumerate() {
            let ru_key: SmolStr = ty.name.to_lowercase().into();
            let en_key: SmolStr = ty.english_name.to_lowercase().into();
            types_by_name.insert(ru_key, idx);
            types_by_name.insert(en_key, idx);
        }

        // Convert raw methods to SmolStr-based methods
        let methods: Vec<PlatformMethod> =
            crate::generated::PLATFORM_METHODS.iter().map(PlatformMethod::from).collect();

        let mut methods_by_name = FxHashMap::default();

        // Index platform methods by (type_name, method_name)
        for (idx, method) in methods.iter().enumerate() {
            let type_key: SmolStr = method.type_name.to_lowercase().into();
            let ru_key: SmolStr = method.name.to_lowercase().into();
            let en_key: SmolStr = method.english_name.to_lowercase().into();

            // Index by Russian name: (type, method_ru)
            methods_by_name.insert((type_key.clone(), ru_key), idx);
            // Index by English name: (type, method_en)
            methods_by_name.insert((type_key, en_key), idx);
        }

        // Convert raw global functions to SmolStr-based functions
        let global_functions: Vec<GlobalFunction> =
            crate::generated::PLATFORM_GLOBAL_FUNCTIONS.iter().map(GlobalFunction::from).collect();

        let mut global_functions_by_name = FxHashMap::default();

        // Index global functions by name (both Russian and English)
        for (idx, function) in global_functions.iter().enumerate() {
            let ru_key: SmolStr = function.name.to_lowercase().into();
            let en_key: SmolStr = function.english_name.to_lowercase().into();

            // Index by Russian name
            global_functions_by_name.insert(ru_key, idx);
            // Index by English name
            global_functions_by_name.insert(en_key, idx);
        }

        // Index method documentation by ID
        let mut method_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::METHOD_DOCS.iter().enumerate() {
            method_docs_by_id.insert(docs.method_id, idx);
        }

        // Index global function documentation by ID
        let mut global_function_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::GLOBAL_FUNCTION_DOCS.iter().enumerate() {
            global_function_docs_by_id.insert(docs.method_id, idx);
        }

        Self {
            types,
            types_by_name,
            methods,
            methods_by_name,
            global_functions,
            global_functions_by_name,
            method_docs_by_id,
            global_function_docs_by_id,
        }
    }

    /// Get platform type by name (case-insensitive, bilingual).
    pub(crate) fn get_type(&self, name: &str) -> Option<&PlatformType> {
        let key: SmolStr = name.to_lowercase().into();
        let idx = *self.types_by_name.get(&key)?;
        self.types.get(idx)
    }

    /// Get all platform types.
    pub fn all_types(&self) -> &[PlatformType] {
        &self.types
    }

    /// Get platform method by type and method name (case-insensitive, bilingual).
    pub fn get_method(&self, type_name: &str, method_name: &str) -> Option<&PlatformMethod> {
        let type_key: SmolStr = type_name.to_lowercase().into();
        let method_key: SmolStr = method_name.to_lowercase().into();
        let idx = *self.methods_by_name.get(&(type_key, method_key))?;
        self.methods.get(idx)
    }

    /// Get all platform methods.
    pub fn all_methods(&self) -> &[PlatformMethod] {
        &self.methods
    }

    /// Get all methods for a specific type (case-insensitive, bilingual).
    pub fn get_type_methods(&self, type_name: &str) -> Vec<&PlatformMethod> {
        let type_key: SmolStr = type_name.to_lowercase().into();
        self.methods.iter().filter(|m| m.type_name.to_lowercase() == type_key.as_str()).collect()
    }

    /// Get global function by name (case-insensitive, bilingual).
    pub fn get_global_function(&self, name: &str) -> Option<&GlobalFunction> {
        let key: SmolStr = name.to_lowercase().into();
        let idx = *self.global_functions_by_name.get(&key)?;
        self.global_functions.get(idx)
    }

    /// Get all global functions.
    pub fn all_global_functions(&self) -> &[GlobalFunction] {
        &self.global_functions
    }

    /// Get method documentation by method ID.
    pub fn get_method_docs(&self, method_id: u32) -> Option<crate::types::MethodDocs> {
        let idx = *self.method_docs_by_id.get(&method_id)?;
        let raw_docs = crate::generated::METHOD_DOCS.get(idx)?;
        Some(crate::types::MethodDocs::from(raw_docs))
    }

    /// Get global function documentation by function ID.
    pub fn get_global_function_docs(&self, function_id: u32) -> Option<crate::types::MethodDocs> {
        let idx = *self.global_function_docs_by_id.get(&function_id)?;
        let raw_docs = crate::generated::GLOBAL_FUNCTION_DOCS.get(idx)?;
        Some(crate::types::MethodDocs::from(raw_docs))
    }
}

// Backward compatibility: type alias for non-Salsa usage
pub type PlatformData = PlatformDataInner;

// ============================================================================
// Salsa Interned Inputs
// ============================================================================

/// Interned type name for platform lookups.
///
/// This allows Salsa to intern and deduplicate type name strings.
#[salsa::interned(debug)]
pub struct TypeNameInput {
    pub name: String,
}

/// Interned method lookup key (type_name + method_name).
#[salsa::interned(debug)]
pub struct MethodLookupInput {
    pub type_name: String,
    pub method_name: String,
}

// ============================================================================
// Salsa Queries
// ============================================================================

/// Lookup platform type by name (case-insensitive, bilingual).
///
/// This Salsa query provides cached access to platform types.
/// The underlying data never changes (loaded once at startup), but Salsa caching
/// provides efficient repeated lookups.
///
/// # Example
/// ```ignore
/// let input = TypeNameInput::new(db, "Строка".to_string());
/// let ty = platform_type_query(db, input);
/// assert_eq!(ty.unwrap().english_name.as_str(), "String");
/// ```
#[salsa::tracked(lru = 256)]
pub fn platform_type_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Option<PlatformType> {
    let name = input.name(db);
    let data = PlatformDataInner::instance();
    data.get_type(&name).cloned()
}

/// Lookup platform method by type and method name (case-insensitive, bilingual).
///
/// This Salsa query provides cached access to platform methods.
///
/// # Example
/// ```ignore
/// let input = MethodLookupInput::new(db, "Строка".to_string(), "ВРег".to_string());
/// let method = platform_method_query(db, input);
/// assert_eq!(method.unwrap().english_name.as_str(), "Upper");
/// ```
#[salsa::tracked(lru = 256)]
pub fn platform_method_query<'db>(
    db: &'db dyn salsa::Database,
    input: MethodLookupInput<'db>,
) -> Option<PlatformMethod> {
    let type_name = input.type_name(db);
    let method_name = input.method_name(db);
    let data = PlatformDataInner::instance();
    data.get_method(&type_name, &method_name).cloned()
}

/// Get all methods for a specific type (case-insensitive, bilingual).
///
/// Returns Arc<Vec<>> for efficient sharing across queries.
///
/// # Example
/// ```ignore
/// let input = TypeNameInput::new(db, "Строка".to_string());
/// let methods = type_methods_query(db, input);
/// for method in methods.iter() {
///     println!("{} / {}", method.name, method.english_name);
/// }
/// ```
#[salsa::tracked(lru = 128)]
pub fn type_methods_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Arc<Vec<PlatformMethod>> {
    let type_name = input.name(db);
    let data = PlatformDataInner::instance();
    Arc::new(data.get_type_methods(&type_name).into_iter().cloned().collect())
}

/// Lookup global function by name (case-insensitive, bilingual).
///
/// This Salsa query provides cached access to global platform functions.
///
/// # Example
/// ```ignore
/// let input = TypeNameInput::new(db, "НачатьТранзакцию".to_string());
/// let function = global_function_query(db, input);
/// assert_eq!(function.unwrap().english_name.as_str(), "BeginTransaction");
/// ```
#[salsa::tracked(lru = 256)]
pub fn global_function_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Option<GlobalFunction> {
    let name = input.name(db);
    let data = PlatformDataInner::instance();
    data.get_global_function(&name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_data_singleton() {
        let data1 = PlatformDataInner::instance();
        let data2 = PlatformDataInner::instance();

        // Same instance
        assert!(std::ptr::eq(data1, data2));
    }

    #[test]
    fn test_get_type_case_insensitive() {
        let data = PlatformDataInner::instance();

        // When platform is not installed, no types available
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let ty = data.all_types().first().unwrap();
        let name = ty.name.as_str();

        // Should find by different cases
        assert!(data.get_type(name).is_some());
        assert!(data.get_type(&name.to_lowercase()).is_some());
        assert!(data.get_type(&name.to_uppercase()).is_some());
    }

    #[test]
    fn test_get_method_bilingual() {
        let data = PlatformDataInner::instance();

        if data.all_methods().is_empty() {
            println!("Skipping test: no platform methods available");
            return;
        }

        // Test bilingual method lookup
        let method = data.all_methods().first().unwrap();
        let type_name = method.type_name.as_str();
        let ru_name = method.name.as_str();
        let en_name = method.english_name.as_str();

        // Should find by Russian name
        let found = data.get_method(type_name, ru_name);
        assert!(found.is_some(), "Should find method by Russian name");
        assert_eq!(found.unwrap().id, method.id);

        // Should find by English name
        let found = data.get_method(type_name, en_name);
        assert!(found.is_some(), "Should find method by English name");
        assert_eq!(found.unwrap().id, method.id);

        // Should be case-insensitive
        let found = data.get_method(&type_name.to_uppercase(), &ru_name.to_uppercase());
        assert!(found.is_some(), "Should be case-insensitive");
    }

    #[test]
    fn test_get_type_methods() {
        let data = PlatformDataInner::instance();

        if data.all_types().is_empty() || data.all_methods().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        // Get first type that has methods
        let ty = data.all_types().first().unwrap();
        let methods = data.get_type_methods(&ty.english_name);

        if !methods.is_empty() {
            println!("Type {} has {} methods", ty.english_name, methods.len());
            // All methods should belong to this type
            for method in &methods {
                assert_eq!(method.type_name.to_lowercase(), ty.english_name.to_lowercase());
            }
        }
    }

    // Salsa query tests - these test the Salsa integration
    #[salsa::db]
    #[derive(Clone, Default)]
    struct TestDatabase {
        storage: salsa::Storage<Self>,
    }

    impl salsa::Database for TestDatabase {}

    #[test]
    fn test_platform_type_query() {
        let db = TestDatabase::default();

        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        // Case-insensitive lookup using interned inputs
        let input1 = TypeNameInput::new(&db, "Строка".to_string());
        let input2 = TypeNameInput::new(&db, "СТРОКА".to_string());
        let input3 = TypeNameInput::new(&db, "String".to_string());

        let ty1 = platform_type_query(&db, input1);
        let ty2 = platform_type_query(&db, input2);
        let ty3 = platform_type_query(&db, input3);

        // All lookups should return the same type (or all None)
        assert_eq!(ty1.is_some(), ty2.is_some());
        assert_eq!(ty1.is_some(), ty3.is_some());

        if let (Some(t1), Some(t2), Some(t3)) = (ty1, ty2, ty3) {
            assert_eq!(t1.name, t2.name);
            assert_eq!(t1.name, t3.name);
        }
    }

    #[test]
    fn test_platform_method_query() {
        let db = TestDatabase::default();

        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_methods().is_empty() {
            println!("Skipping test: no platform methods available");
            return;
        }

        let input = MethodLookupInput::new(&db, "Строка".to_string(), "ВРег".to_string());
        let method = platform_method_query(&db, input);

        if let Some(method) = method {
            assert_eq!(method.name.as_str(), "ВРег");
            assert_eq!(method.english_name.as_str(), "Upper");
        }
    }

    #[test]
    fn test_type_methods_query() {
        let db = TestDatabase::default();

        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let input = TypeNameInput::new(&db, "Строка".to_string());
        let methods = type_methods_query(&db, input);

        if !methods.is_empty() {
            // All methods should belong to String type
            for method in methods.iter() {
                assert_eq!(method.type_name.to_lowercase(), "строка");
            }
        }
    }

    #[test]
    fn test_global_functions() {
        let data = PlatformDataInner::instance();

        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        println!("Total global functions: {}", data.all_global_functions().len());

        // Test НачатьТранзакцию
        let func = data.get_global_function("НачатьТранзакцию");
        assert!(func.is_some(), "Should find НачатьТранзакцию");

        let func = func.unwrap();
        assert_eq!(func.name.as_str(), "НачатьТранзакцию");
        assert_eq!(func.english_name.as_str(), "BeginTransaction");
        println!("Found function: {} / {}", func.name, func.english_name);

        // Test with English name
        let func_en = data.get_global_function("BeginTransaction");
        assert!(func_en.is_some(), "Should find by English name");
        assert_eq!(func_en.unwrap().id, func.id);

        // Test case insensitivity
        let func_upper = data.get_global_function("НАЧАТЬТРАНЗАКЦИЮ");
        assert!(func_upper.is_some(), "Should be case-insensitive");
        assert_eq!(func_upper.unwrap().id, func.id);
    }
}
