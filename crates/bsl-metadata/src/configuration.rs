//! Configuration metadata root object
//!
//! Ported from <https://github.com/1c-syntax/mdclasses>

use crate::common_module::CommonModule;
use crate::defined_type::DefinedType;
use crate::error::Result;
use crate::event_subscription::EventSubscription;
use crate::metadata_object::{MdoType, MetadataObject};
use crate::register::Register;
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

    /// Metadata objects (Catalogs, Documents, etc.)
    #[serde(rename = "metadataObjects", default)]
    metadata_objects: Vec<MetadataObject>,

    /// Registers (Information, Accumulation, Accounting, Calculation)
    #[serde(rename = "registers", default)]
    registers: Vec<Register>,

    /// Event subscriptions
    #[serde(rename = "eventSubscriptions", default)]
    event_subscriptions: Vec<EventSubscription>,

    /// Defined types (ОпределяемыеТипы)
    #[serde(rename = "definedTypes", default)]
    defined_types: Vec<DefinedType>,

    /// Cache: URI -> Module index mapping (not serialized)
    #[serde(skip)]
    uri_to_module: HashMap<String, usize>,

    /// Cache: Name -> Common Module index mapping (not serialized)
    #[serde(skip)]
    name_to_common_module: HashMap<String, usize>,

    /// Cache: Name -> Register index mapping (not serialized)
    #[serde(skip)]
    name_to_register: HashMap<String, usize>,

    /// Cache: Name -> EventSubscription index mapping (not serialized)
    #[serde(skip)]
    name_to_event_subscription: HashMap<String, usize>,

    /// Cache: Name -> DefinedType index mapping (not serialized)
    #[serde(skip)]
    name_to_defined_type: HashMap<String, usize>,

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
            && self.registers == other.registers
            && self.event_subscriptions == other.event_subscriptions
            && self.defined_types == other.defined_types
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
            registers: Vec::new(),
            event_subscriptions: Vec::new(),
            defined_types: Vec::new(),
            uri_to_module: HashMap::new(),
            name_to_common_module: HashMap::new(),
            name_to_register: HashMap::new(),
            name_to_event_subscription: HashMap::new(),
            name_to_defined_type: HashMap::new(),
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
        self.name_to_register.clear();
        self.name_to_event_subscription.clear();
        self.name_to_defined_type.clear();

        for (idx, module) in self.common_modules.iter().enumerate() {
            if let Some(uri) = module.uri() {
                self.uri_to_module.insert(uri.to_string(), idx);
            }
            self.name_to_common_module.insert(module.name().to_lowercase(), idx);
        }

        for (idx, register) in self.registers.iter().enumerate() {
            self.name_to_register.insert(register.name().to_lowercase(), idx);
        }

        for (idx, event_sub) in self.event_subscriptions.iter().enumerate() {
            self.name_to_event_subscription.insert(event_sub.name().to_lowercase(), idx);
        }

        for (idx, defined_type) in self.defined_types.iter().enumerate() {
            self.name_to_defined_type.insert(defined_type.name().to_lowercase(), idx);
        }
    }

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

        // Check if this is a register type - registers are stored separately
        let result = match mdo_type {
            MdoType::InformationRegister
            | MdoType::AccumulationRegister
            | MdoType::AccountingRegister
            | MdoType::CalculationRegister => {
                // Search in registers
                self.registers.iter().any(|reg| {
                    reg.mdo_type() == mdo_type && reg.name().to_lowercase() == name_lower
                })
            }
            _ => {
                // Search in metadata_objects
                self.metadata_objects
                    .iter()
                    .any(|obj| obj.mdo_type == mdo_type && obj.name.to_lowercase() == name_lower)
            }
        };

        result
    }

    /// Find metadata object by type and name
    pub fn find_metadata_object(&self, mdo_type: MdoType, name: &str) -> Option<&MetadataObject> {
        let name_lower = name.to_lowercase();
        self.metadata_objects
            .iter()
            .find(|obj| obj.mdo_type == mdo_type && obj.name.to_lowercase() == name_lower)
    }

    /// Get all registers
    pub fn registers(&self) -> &[Register] {
        &self.registers
    }

    /// Find register by name (case-insensitive)
    pub fn find_register(&self, name: &str) -> Option<&Register> {
        let name_lower = name.to_lowercase();
        self.name_to_register.get(&name_lower).and_then(|&idx| self.registers.get(idx))
    }

    /// Find register by type and name (case-insensitive)
    pub fn find_register_by_type_and_name(
        &self,
        mdo_type: MdoType,
        name: &str,
    ) -> Option<&Register> {
        let name_lower = name.to_lowercase();
        self.name_to_register
            .get(&name_lower)
            .and_then(|&idx| self.registers.get(idx).filter(|r| r.mdo_type() == mdo_type))
    }

    /// Add register
    pub fn add_register(&mut self, register: Register) {
        let idx = self.registers.len();
        self.name_to_register.insert(register.name().to_lowercase(), idx);
        self.registers.push(register);
    }

    /// Get all event subscriptions
    ///
    /// Java equivalent: `getEventSubscriptions()`
    pub fn event_subscriptions(&self) -> &[EventSubscription] {
        &self.event_subscriptions
    }

    /// Find event subscription by name (case-insensitive)
    ///
    /// Java equivalent: `findEventSubscription(String)`
    pub fn find_event_subscription(&self, name: &str) -> Option<&EventSubscription> {
        let name_lower = name.to_lowercase();
        self.name_to_event_subscription
            .get(&name_lower)
            .and_then(|&idx| self.event_subscriptions.get(idx))
    }

    /// Add event subscription
    pub(crate) fn add_event_subscription(&mut self, subscription: EventSubscription) {
        let idx = self.event_subscriptions.len();
        self.name_to_event_subscription.insert(subscription.name().to_lowercase(), idx);
        self.event_subscriptions.push(subscription);
    }

    /// Get all defined types
    pub fn defined_types(&self) -> &[DefinedType] {
        &self.defined_types
    }

    /// Find defined type by name (case-insensitive)
    pub fn find_defined_type(&self, name: &str) -> Option<&DefinedType> {
        let name_lower = name.to_lowercase();
        self.name_to_defined_type.get(&name_lower).and_then(|&idx| self.defined_types.get(idx))
    }

    /// Add defined type
    pub(crate) fn add_defined_type(&mut self, defined_type: DefinedType) {
        let idx = self.defined_types.len();
        self.name_to_defined_type.insert(defined_type.name().to_lowercase(), idx);
        self.defined_types.push(defined_type);
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

    #[test]
    fn test_add_and_find_register() {
        use crate::register::Register;

        let mut config = Configuration::new("Test");

        let register = Register::builder()
            .name("РегистрСведений1")
            .mdo_type(MdoType::InformationRegister)
            .build();

        config.add_register(register);

        assert_eq!(config.registers().len(), 1);

        // Find by name
        let found = config.find_register("РегистрСведений1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), "РегистрСведений1");

        // Find by name case-insensitive
        let found_ci = config.find_register("регистрсведений1");
        assert!(found_ci.is_some());

        // Find by type and name
        let found_typed =
            config.find_register_by_type_and_name(MdoType::InformationRegister, "РегистрСведений1");
        assert!(found_typed.is_some());

        // Wrong type
        let not_found = config
            .find_register_by_type_and_name(MdoType::AccumulationRegister, "РегистрСведений1");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_multiple_register_types() {
        use crate::register::Register;

        let mut config = Configuration::new("Test");

        let info_reg = Register::builder()
            .name("РегистрСведений1")
            .mdo_type(MdoType::InformationRegister)
            .build();

        let accum_reg = Register::builder()
            .name("РегистрНакопления1")
            .mdo_type(MdoType::AccumulationRegister)
            .build();

        config.add_register(info_reg);
        config.add_register(accum_reg);

        assert_eq!(config.registers().len(), 2);

        let info_found = config.find_register("РегистрСведений1");
        assert!(info_found.is_some());
        assert!(info_found.unwrap().is_information_register());

        let accum_found = config.find_register("РегистрНакопления1");
        assert!(accum_found.is_some());
        assert!(accum_found.unwrap().is_accumulation_register());
    }
}
