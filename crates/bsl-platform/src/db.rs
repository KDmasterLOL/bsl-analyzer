//! Platform data database with efficient indexing.

use crate::types::{PlatformMethod, PlatformType};
use once_cell::sync::OnceCell;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;

static PLATFORM_DATA: OnceCell<PlatformData> = OnceCell::new();

/// Platform data singleton with indexed access.
pub struct PlatformData {
    /// Converted platform types (from raw const data)
    types: Vec<PlatformType>,
    /// Types indexed by lowercase name (both Russian and English)
    types_by_name: FxHashMap<SmolStr, usize>,
    /// Converted platform methods (from raw const data)
    methods: Vec<PlatformMethod>,
    /// Methods indexed by (type_name, method_name)
    methods_by_name: FxHashMap<(SmolStr, SmolStr), usize>,
}

impl PlatformData {
    /// Get the global platform data instance.
    pub fn instance() -> &'static Self {
        PLATFORM_DATA.get_or_init(Self::new)
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

        Self { types, types_by_name, methods, methods_by_name }
    }

    /// Get platform type by name (case-insensitive, supports both Russian and English).
    pub fn get_type(&self, name: &str) -> Option<&PlatformType> {
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

    /// Get method documentation (only available with platform_docs feature).
    #[cfg(feature = "platform_docs")]
    pub fn get_method_docs(&self, _method_id: u32) -> Option<crate::types::MethodDocs> {
        // TODO: Load from generated docs
        None
    }

    /// Get method documentation (stub when platform_docs is disabled).
    #[cfg(not(feature = "platform_docs"))]
    pub fn get_method_docs(&self, _method_id: u32) -> Option<()> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_data_singleton() {
        let data1 = PlatformData::instance();
        let data2 = PlatformData::instance();

        // Same instance
        assert!(std::ptr::eq(data1, data2));
    }

    #[test]
    fn test_get_type_case_insensitive() {
        let data = PlatformData::instance();

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
        let data = PlatformData::instance();

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
        let data = PlatformData::instance();

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
}
