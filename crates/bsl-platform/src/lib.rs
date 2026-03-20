//! 1C:Enterprise platform types and methods database.
//!
//! This crate provides access to platform types (Строка, Число, Массив, etc.)
//! and their methods with documentation.
//!
//! # Build-time Data Extraction
//!
//! The crate automatically detects installed 1C:Enterprise platform during build
//! and extracts documentation from `shcntx_ru.hbk`. If platform is not found,
//! builds with minimal data (types and signatures only, no documentation).
//!
//! # Usage
//!
//! ```
//! use bsl_platform::PlatformDataInner;
//!
//! let data = PlatformDataInner::instance();
//!
//! // Get all platform types
//! for ty in data.all_types() {
//!     println!("{} / {}", ty.name, ty.english_name);
//! }
//!
//! // Get methods for a type
//! let methods = data.get_type_methods("Строка");
//! for method in methods {
//!     println!("Method: {}", method.name);
//! }
//! ```

mod db;
pub mod search;
mod types;

// Include generated code from build.rs
#[allow(warnings)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));

    use super::types::*;
}

// Re-export public API
pub use db::{
    global_function_query, platform_method_query, platform_type_query, type_methods_query,
    MethodLookupInput, PlatformData, PlatformDataInner, TypeNameInput,
};
pub use search::{DocKind, SearchResult};
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_data_loads() {
        let data = PlatformData::instance();

        // Should not panic
        let types = data.all_types();
        let methods = data.all_methods();

        println!("Loaded {} types and {} methods", types.len(), methods.len());
    }

    #[test]
    fn test_bilingual_lookup() {
        let data = PlatformData::instance();

        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        // Should be able to look up by both Russian and English names
        for ty in data.all_types() {
            assert!(
                data.get_type(&ty.name).is_some(),
                "Failed to find type by Russian name: {}",
                ty.name
            );
            assert!(
                data.get_type(&ty.english_name).is_some(),
                "Failed to find type by English name: {}",
                ty.english_name
            );
        }
    }
}
