//! Configuration metadata root object
//!
//! Ported from <https://github.com/1c-syntax/mdclasses>

use crate::common_module::CommonModule;
use crate::error::Result;
use crate::metadata_object::{MdoType, MetadataObject};
use crate::traits::{MdObject, Module};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

/// Configuration - root metadata object
///
/// Java equivalent: `com.github._1c_syntax.bsl.mdclasses.Configuration`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Configuration {
    /// Configuration UUID
    #[serde(rename = "uuid", default = "Uuid::new_v4")]
    uuid: Uuid,

    /// Configuration name
    #[serde(rename = "name")]
    name: String,

    /// Common modules list
    #[serde(rename = "commonModules", default)]
    common_modules: Vec<CommonModule>,

    /// Metadata objects (Catalogs, Documents, Registers, etc.)
    #[serde(rename = "metadataObjects", default)]
    metadata_objects: Vec<MetadataObject>,

    /// Cache: URI -> Module index mapping (not serialized)
    #[serde(skip)]
    uri_to_module: HashMap<String, usize>,

    /// Cache: Name -> Common Module index mapping (not serialized)
    #[serde(skip)]
    name_to_common_module: HashMap<String, usize>,

    /// Use managed forms in ordinary application
    #[serde(rename = "useManagedFormInOrdinaryApplication", default)]
    use_managed_form_in_ordinary_application: bool,

    /// Use ordinary forms in managed application
    #[serde(rename = "useOrdinaryFormInManagedApplication", default)]
    use_ordinary_form_in_managed_application: bool,
}

impl PartialEq for Configuration {
    fn eq(&self, other: &Self) -> bool {
        // Compare all fields EXCEPT the HashMap caches (which are derived from data)
        self.uuid == other.uuid
            && self.name == other.name
            && self.common_modules == other.common_modules
            && self.metadata_objects == other.metadata_objects
            && self.use_managed_form_in_ordinary_application
                == other.use_managed_form_in_ordinary_application
            && self.use_ordinary_form_in_managed_application
                == other.use_ordinary_form_in_managed_application
    }
}

impl Configuration {
    /// Create empty configuration
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            name: name.into(),
            common_modules: Vec::new(),
            metadata_objects: Vec::new(),
            uri_to_module: HashMap::new(),
            name_to_common_module: HashMap::new(),
            use_managed_form_in_ordinary_application: false,
            use_ordinary_form_in_managed_application: false,
        }
    }

    /// Load configuration from XML file
    ///
    /// Supports both EDT and Designer formats
    pub fn from_xml_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_xml_str(&content)
    }

    /// Parse configuration from XML string
    pub fn from_xml_str(xml: &str) -> Result<Self> {
        let mut config: Configuration = quick_xml::de::from_str(xml)?;
        config.build_caches();
        Ok(config)
    }

    /// Build internal caches for fast lookups
    fn build_caches(&mut self) {
        // Build URI -> Module mapping
        self.uri_to_module.clear();
        self.name_to_common_module.clear();

        for (idx, module) in self.common_modules.iter().enumerate() {
            if let Some(uri) = module.uri() {
                self.uri_to_module.insert(uri.to_string(), idx);
            }
            self.name_to_common_module.insert(module.name().to_lowercase(), idx);
        }
    }

    // === Getters (following Java naming conventions) ===

    /// Get configuration name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get configuration UUID
    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Get all common modules
    ///
    /// Java equivalent: `getCommonModules()`
    pub fn common_modules(&self) -> &[CommonModule] {
        &self.common_modules
    }

    /// Find common module by name (case-insensitive)
    ///
    /// Java equivalent: `findCommonModule(String)`
    pub fn find_common_module(&self, name: &str) -> Option<&CommonModule> {
        let name_lower = name.to_lowercase();
        self.name_to_common_module.get(&name_lower).and_then(|&idx| self.common_modules.get(idx))
    }

    /// Find module by URI
    ///
    /// Java equivalent: `getModuleByUri(URI)`
    pub fn find_module_by_uri(&self, uri: &str) -> Option<&dyn Module> {
        self.uri_to_module
            .get(uri)
            .and_then(|&idx| self.common_modules.get(idx))
            .map(|cm| cm as &dyn Module)
    }

    /// Find any metadata object by URI
    ///
    /// Java equivalent: `findChild(URI)`
    pub fn find_child_by_uri(&self, uri: &str) -> Option<&dyn MdObject> {
        // For now, only common modules are supported
        // TODO: Add other MDO types (Documents, Catalogs, etc.)
        self.uri_to_module
            .get(uri)
            .and_then(|&idx| self.common_modules.get(idx))
            .map(|cm| cm as &dyn MdObject)
    }

    /// Add common module
    pub fn add_common_module(&mut self, module: CommonModule) {
        let idx = self.common_modules.len();

        if let Some(uri) = module.uri() {
            self.uri_to_module.insert(uri.to_string(), idx);
        }
        self.name_to_common_module.insert(module.name().to_lowercase(), idx);

        self.common_modules.push(module);
    }

    /// Check if managed forms are used in ordinary application
    ///
    /// Java equivalent: `isUseManagedFormInOrdinaryApplication()`
    pub fn use_managed_form_in_ordinary_application(&self) -> bool {
        self.use_managed_form_in_ordinary_application
    }

    /// Check if ordinary forms are used in managed application
    ///
    /// Java equivalent: `isUseOrdinaryFormInManagedApplication()`
    pub fn use_ordinary_form_in_managed_application(&self) -> bool {
        self.use_ordinary_form_in_managed_application
    }

    /// Set use managed forms in ordinary application flag
    pub fn set_use_managed_form_in_ordinary_application(&mut self, value: bool) {
        self.use_managed_form_in_ordinary_application = value;
    }

    /// Set use ordinary forms in managed application flag
    pub fn set_use_ordinary_form_in_managed_application(&mut self, value: bool) {
        self.use_ordinary_form_in_managed_application = value;
    }

    /// Get all metadata objects
    pub fn metadata_objects(&self) -> &[MetadataObject] {
        &self.metadata_objects
    }

    /// Add metadata object
    pub fn add_metadata_object(&mut self, object: MetadataObject) {
        self.metadata_objects.push(object);
    }

    /// Find metadata object by type and name (case-insensitive)
    ///
    /// Returns true if object exists
    pub fn has_metadata_object(&self, mdo_type: MdoType, name: &str) -> bool {
        let name_lower = name.to_lowercase();
        self.metadata_objects
            .iter()
            .any(|obj| obj.mdo_type == mdo_type && obj.name.to_lowercase() == name_lower)
    }

    /// Find metadata object by type and name
    pub fn find_metadata_object(&self, mdo_type: MdoType, name: &str) -> Option<&MetadataObject> {
        let name_lower = name.to_lowercase();
        self.metadata_objects
            .iter()
            .find(|obj| obj.mdo_type == mdo_type && obj.name.to_lowercase() == name_lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::ReturnValueReuse;

    #[test]
    fn test_configuration_creation() {
        let config = Configuration::new("TestConfiguration");
        assert_eq!(config.name(), "TestConfiguration");
        assert_eq!(config.common_modules().len(), 0);
    }

    #[test]
    fn test_add_and_find_common_module() {
        let mut config = Configuration::new("Test");

        let module = CommonModule::builder()
            .name("TestModule")
            .uri(Some("CommonModules/TestModule/Ext/Module.bsl"))
            .return_values_reuse(ReturnValueReuse::DuringRequest)
            .build();

        config.add_common_module(module);

        assert_eq!(config.common_modules().len(), 1);

        // Find by name
        let found = config.find_common_module("TestModule");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), "TestModule");

        // Find by name case-insensitive
        let found_ci = config.find_common_module("testmodule");
        assert!(found_ci.is_some());

        // Find by URI
        let found_uri = config.find_module_by_uri("CommonModules/TestModule/Ext/Module.bsl");
        assert!(found_uri.is_some());
        assert_eq!(found_uri.unwrap().name(), "TestModule");
    }

    #[test]
    fn test_find_child_by_uri() {
        let mut config = Configuration::new("Test");

        let module = CommonModule::builder()
            .name("Global")
            .uri(Some("CommonModules/Global/Ext/Module.bsl"))
            .global(true)
            .build();

        config.add_common_module(module);

        let child = config.find_child_by_uri("CommonModules/Global/Ext/Module.bsl");
        assert!(child.is_some());
        assert_eq!(child.unwrap().name(), "Global");
    }

    #[test]
    fn test_metadata_objects() {
        let mut config = Configuration::new("Test");

        let catalog = MetadataObject::new(MdoType::Catalog, "Номенклатура");
        config.add_metadata_object(catalog);

        assert_eq!(config.metadata_objects().len(), 1);
        assert!(config.has_metadata_object(MdoType::Catalog, "Номенклатура"));

        let found = config.find_metadata_object(MdoType::Catalog, "номенклатура");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Номенклатура");
    }
}
