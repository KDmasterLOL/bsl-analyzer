//! Platform data database with efficient indexing.

use crate::types::{PlatformMethod, PlatformType};
use once_cell::sync::OnceCell;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;

static PLATFORM_DATA: OnceCell<PlatformData> = OnceCell::new();

/// Platform data singleton with indexed access.
pub struct PlatformData {
    /// Types indexed by lowercase name (both Russian and English)
    types_by_name: FxHashMap<SmolStr, &'static PlatformType>,
    /// Methods indexed by (type_name, method_name)
    methods_by_name: FxHashMap<(SmolStr, SmolStr), &'static PlatformMethod>,
}

impl PlatformData {
    /// Get the global platform data instance.
    pub fn instance() -> &'static Self {
        PLATFORM_DATA.get_or_init(Self::new)
    }

    /// Initialize platform data and build indices.
    fn new() -> Self {
        let mut types_by_name = FxHashMap::default();

        // Index platform types
        for ty in crate::generated::PLATFORM_TYPES {
            let ru_key = ty.name.to_lowercase();
            let en_key = ty.english_name.to_lowercase();
            types_by_name.insert(ru_key.into(), ty);
            types_by_name.insert(en_key.into(), ty);
        }

        // Index platform methods (TODO: populate when methods are generated)
        let methods_by_name = FxHashMap::default();

        Self { types_by_name, methods_by_name }
    }

    /// Get platform type by name (case-insensitive, supports both Russian and English).
    pub fn get_type(&self, name: &str) -> Option<&'static PlatformType> {
        let key: SmolStr = name.to_lowercase().into();
        self.types_by_name.get(&key).copied()
    }

    /// Get all platform types.
    pub fn all_types(&self) -> &'static [PlatformType] {
        crate::generated::PLATFORM_TYPES
    }

    /// Get platform method by type and method name.
    pub fn get_method(
        &self,
        type_name: &str,
        method_name: &str,
    ) -> Option<&'static PlatformMethod> {
        let type_key: SmolStr = type_name.to_lowercase().into();
        let method_key: SmolStr = method_name.to_lowercase().into();
        self.methods_by_name.get(&(type_key, method_key)).copied()
    }

    /// Get all platform methods.
    pub fn all_methods(&self) -> &'static [PlatformMethod] {
        crate::generated::PLATFORM_METHODS
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
}
