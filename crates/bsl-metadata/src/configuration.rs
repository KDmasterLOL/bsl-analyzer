//! Configuration metadata root object

use crate::common_module::CommonModule;
use crate::defined_type::DefinedType;
use crate::error::Result;
use crate::event_subscription::EventSubscription;
use crate::http_service::HTTPService;
use crate::metadata_object::{AttributeType, MdoType, MetadataObject};
use crate::register::Register;
use crate::role::Role;
use crate::scheduled_job::ScheduledJob;
use crate::traits::{MdObject, Module};
use crate::web_service::WebService;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

/// Configuration - root metadata object
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

    /// Scheduled jobs (РегламентныеЗадания)
    #[serde(rename = "scheduledJobs", default)]
    scheduled_jobs: Vec<ScheduledJob>,

    /// Roles (Роли)
    #[serde(rename = "roles", default)]
    roles: Vec<Role>,

    /// HTTP services (HTTP-сервисы)
    #[serde(rename = "httpServices", default)]
    http_services: Vec<HTTPService>,

    /// Web services (Web-сервисы / SOAP)
    #[serde(rename = "webServices", default)]
    web_services: Vec<WebService>,

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

    /// Cache: Name -> ScheduledJob index mapping (not serialized)
    #[serde(skip)]
    name_to_scheduled_job: HashMap<String, usize>,

    /// Cache: Name -> Role index mapping (not serialized)
    #[serde(skip)]
    name_to_role: HashMap<String, usize>,

    /// Cache: Name -> HTTPService index mapping (not serialized)
    #[serde(skip)]
    name_to_http_service: HashMap<String, usize>,

    /// Cache: Name -> WebService index mapping (not serialized)
    #[serde(skip)]
    name_to_web_service: HashMap<String, usize>,

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
            && self.scheduled_jobs == other.scheduled_jobs
            && self.roles == other.roles
            && self.http_services == other.http_services
            && self.web_services == other.web_services
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
            scheduled_jobs: Vec::new(),
            roles: Vec::new(),
            uri_to_module: HashMap::new(),
            name_to_common_module: HashMap::new(),
            name_to_register: HashMap::new(),
            name_to_event_subscription: HashMap::new(),
            name_to_defined_type: HashMap::new(),
            name_to_scheduled_job: HashMap::new(),
            name_to_role: HashMap::new(),
            name_to_http_service: HashMap::new(),
            name_to_web_service: HashMap::new(),
            use_managed_form_in_ordinary_application: false,
            use_ordinary_form_in_managed_application: false,
            http_services: Vec::new(),
            web_services: Vec::new(),
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
        let doc = roxmltree::Document::parse(xml).map_err(|e| {
            crate::error::MetadataError::InvalidFormat(format!("XML parse error: {}", e))
        })?;

        let root = doc.root_element();
        // First child element of root is the Configuration element
        let config_node = root.children().find(|n| n.is_element()).ok_or_else(|| {
            crate::error::MetadataError::InvalidFormat("No Configuration element".to_string())
        })?;

        let uuid_str = config_node.attribute("uuid").unwrap_or("");
        let uuid = uuid_str.parse::<uuid::Uuid>().unwrap_or_else(|_| uuid::Uuid::new_v4());

        let (
            name,
            use_managed_form_in_ordinary_application,
            use_ordinary_form_in_managed_application,
        ) = if let Some(props) =
            config_node.children().find(|n| n.is_element() && n.tag_name().name() == "Properties")
        {
            let name = props
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "Name")
                .and_then(|n| n.text())
                .unwrap_or("")
                .to_string();
            let managed = props
                .children()
                .find(|n| {
                    n.is_element() && n.tag_name().name() == "UseManagedFormInOrdinaryApplication"
                })
                .and_then(|n| n.text())
                .is_some_and(|s| s.eq_ignore_ascii_case("true"));
            let ordinary = props
                .children()
                .find(|n| {
                    n.is_element() && n.tag_name().name() == "UseOrdinaryFormInManagedApplication"
                })
                .and_then(|n| n.text())
                .is_some_and(|s| s.eq_ignore_ascii_case("true"));
            (name, managed, ordinary)
        } else {
            (String::new(), false, false)
        };

        let mut config = Configuration::new(name);
        config.uuid = uuid;
        config.use_managed_form_in_ordinary_application = use_managed_form_in_ordinary_application;
        config.use_ordinary_form_in_managed_application = use_ordinary_form_in_managed_application;
        Ok(config)
    }

    /// Build internal caches for fast lookups after bulk-loading objects
    #[allow(dead_code)]
    fn build_caches(&mut self) {
        // Build URI -> Module mapping
        self.uri_to_module.clear();
        self.name_to_common_module.clear();
        self.name_to_register.clear();
        self.name_to_event_subscription.clear();
        self.name_to_defined_type.clear();
        self.name_to_scheduled_job.clear();
        self.name_to_role.clear();
        self.name_to_http_service.clear();
        self.name_to_web_service.clear();

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

        for (idx, scheduled_job) in self.scheduled_jobs.iter().enumerate() {
            self.name_to_scheduled_job.insert(scheduled_job.name().to_lowercase(), idx);
        }

        for (idx, role) in self.roles.iter().enumerate() {
            self.name_to_role.insert(role.name().to_lowercase(), idx);
        }

        for (idx, http_service) in self.http_services.iter().enumerate() {
            self.name_to_http_service.insert(http_service.name().to_lowercase(), idx);
        }

        for (idx, web_service) in self.web_services.iter().enumerate() {
            self.name_to_web_service.insert(web_service.name().to_lowercase(), idx);
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
    pub fn common_modules(&self) -> &[CommonModule] {
        &self.common_modules
    }

    /// Find common module by name (case-insensitive)
    pub fn find_common_module(&self, name: &str) -> Option<&CommonModule> {
        let name_lower = name.to_lowercase();
        self.name_to_common_module.get(&name_lower).and_then(|&idx| self.common_modules.get(idx))
    }

    /// Find module by URI
    pub fn find_module_by_uri(&self, uri: &str) -> Option<&dyn Module> {
        self.uri_to_module
            .get(uri)
            .and_then(|&idx| self.common_modules.get(idx))
            .map(|cm| cm as &dyn Module)
    }

    /// Find any metadata object by URI
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
    pub fn use_managed_form_in_ordinary_application(&self) -> bool {
        self.use_managed_form_in_ordinary_application
    }

    /// Check if ordinary forms are used in managed application
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

    /// Merge an extension configuration over this configuration.
    ///
    /// Designer extension dumps often contain only the changed part of an
    /// adopted object. The base object still owns platform-derived properties
    /// such as `Hierarchical`, while the extension object contributes extra
    /// attributes/tabular sections. Query analysis needs the effective object
    /// shape, so duplicate objects are merged instead of letting the partial
    /// extension object shadow the base one.
    pub fn merge_extension_overlay(&mut self, extension: &Configuration) {
        for ext_obj in &extension.metadata_objects {
            if let Some(base_obj) = self.metadata_objects.iter_mut().find(|obj| {
                obj.mdo_type == ext_obj.mdo_type && obj.name.eq_ignore_ascii_case(&ext_obj.name)
            }) {
                merge_metadata_object_overlay(base_obj, ext_obj);
            } else {
                self.add_metadata_object(ext_obj.clone());
            }
        }

        for ext_reg in &extension.registers {
            if self.find_register_by_type_and_name(ext_reg.mdo_type(), ext_reg.name()).is_none() {
                self.add_register(ext_reg.clone());
            }
        }

        for ext_defined_type in &extension.defined_types {
            if self.find_defined_type(ext_defined_type.name()).is_none() {
                self.add_defined_type(ext_defined_type.clone());
            }
        }
    }

    /// Return a new configuration with `extension` merged over `self`.
    pub fn merged_with_extension(&self, extension: &Configuration) -> Self {
        let mut merged = self.clone();
        merged.merge_extension_overlay(extension);
        merged
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

    /// Find a constant's declared value type, if any.
    ///
    /// Returns [`None`] both when the constant is not declared in this
    /// configuration **and** when it is declared without a parsed
    /// `<Type>` element. Callers that need to distinguish "missing
    /// constant" from "untyped constant" should use
    /// [`Self::find_metadata_object`] directly.
    pub fn find_constant_type(&self, name: &str) -> Option<&AttributeType> {
        self.find_metadata_object(MdoType::Constant, name)
            .and_then(|mdo| mdo.constant_type.as_ref())
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
    pub fn event_subscriptions(&self) -> &[EventSubscription] {
        &self.event_subscriptions
    }

    /// Find event subscription by name (case-insensitive)
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

    /// Add defined type.
    ///
    /// Public for symmetry with `add_metadata_object` / `add_register` —
    /// downstream crates' tests construct `Configuration` directly.
    pub fn add_defined_type(&mut self, defined_type: DefinedType) {
        let idx = self.defined_types.len();
        self.name_to_defined_type.insert(defined_type.name().to_lowercase(), idx);
        self.defined_types.push(defined_type);
    }

    /// Get all scheduled jobs
    pub fn scheduled_jobs(&self) -> &[ScheduledJob] {
        &self.scheduled_jobs
    }

    /// Find scheduled job by name (case-insensitive)
    pub fn find_scheduled_job(&self, name: &str) -> Option<&ScheduledJob> {
        let name_lower = name.to_lowercase();
        self.name_to_scheduled_job.get(&name_lower).and_then(|&idx| self.scheduled_jobs.get(idx))
    }

    /// Add scheduled job
    pub(crate) fn add_scheduled_job(&mut self, job: ScheduledJob) {
        let idx = self.scheduled_jobs.len();
        self.name_to_scheduled_job.insert(job.name().to_lowercase(), idx);
        self.scheduled_jobs.push(job);
    }

    /// Get all roles
    pub fn roles(&self) -> &[Role] {
        &self.roles
    }

    /// Find role by name (case-insensitive)
    pub fn find_role(&self, name: &str) -> Option<&Role> {
        let name_lower = name.to_lowercase();
        self.name_to_role.get(&name_lower).and_then(|&idx| self.roles.get(idx))
    }

    /// Add role
    pub(crate) fn add_role(&mut self, role: Role) {
        let idx = self.roles.len();
        self.name_to_role.insert(role.name().to_lowercase(), idx);
        self.roles.push(role);
    }

    /// Get all HTTP services
    pub fn http_services(&self) -> &[HTTPService] {
        &self.http_services
    }

    /// Find HTTP service by name (case-insensitive)
    pub fn find_http_service(&self, name: &str) -> Option<&HTTPService> {
        let name_lower = name.to_lowercase();
        self.name_to_http_service.get(&name_lower).and_then(|&idx| self.http_services.get(idx))
    }

    /// Add HTTP service
    pub(crate) fn add_http_service(&mut self, http_service: HTTPService) {
        let idx = self.http_services.len();
        self.name_to_http_service.insert(http_service.name().to_lowercase(), idx);
        self.http_services.push(http_service);
    }

    /// Get all web services (SOAP)
    pub fn web_services(&self) -> &[WebService] {
        &self.web_services
    }

    /// Find web service by name (case-insensitive)
    pub fn find_web_service(&self, name: &str) -> Option<&WebService> {
        let name_lower = name.to_lowercase();
        self.name_to_web_service.get(&name_lower).and_then(|&idx| self.web_services.get(idx))
    }

    /// Add web service
    pub(crate) fn add_web_service(&mut self, web_service: WebService) {
        let idx = self.web_services.len();
        self.name_to_web_service.insert(web_service.name().to_lowercase(), idx);
        self.web_services.push(web_service);
    }
}

fn merge_metadata_object_overlay(base: &mut MetadataObject, overlay: &MetadataObject) {
    if overlay.name_en.is_some() {
        base.name_en = overlay.name_en.clone();
    }
    if overlay.constant_type.is_some() {
        base.constant_type = overlay.constant_type.clone();
    }

    for attr in &overlay.attributes {
        base.attributes.retain(|existing| !existing.name.eq_ignore_ascii_case(&attr.name));
        base.attributes.push(attr.clone());
    }

    for tabular_section in &overlay.tabular_sections {
        base.tabular_sections
            .retain(|existing| !existing.name().eq_ignore_ascii_case(tabular_section.name()));
        base.tabular_sections.push(tabular_section.clone());
    }

    for child in &overlay.children {
        if let Some(base_child) = base.children.iter_mut().find(|existing| {
            existing.mdo_type == child.mdo_type && existing.name.eq_ignore_ascii_case(&child.name)
        }) {
            merge_metadata_object_overlay(base_child, child);
        } else {
            base.children.push(child.clone());
        }
    }

    for enum_value in &overlay.enum_values {
        base.enum_values.retain(|existing| !existing.name.eq_ignore_ascii_case(&enum_value.name));
        base.enum_values.push(enum_value.clone());
    }

    for predefined_item in &overlay.predefined_items {
        base.predefined_items
            .retain(|existing| !existing.name.eq_ignore_ascii_case(&predefined_item.name));
        base.predefined_items.push(predefined_item.clone());
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
    fn merge_extension_overlay_preserves_base_and_adds_extension_attributes() {
        let mut base = Configuration::new("Base");
        let mut base_catalog = MetadataObject::new(MdoType::Catalog, "Номенклатура");
        base_catalog.add_attribute(crate::metadata_object::Attribute {
            name: "Родитель".to_string(),
            name_en: Some("Parent".to_string()),
            attr_type: AttributeType::Ref {
                mdo_type: MdoType::Catalog,
                name: "Номенклатура".to_string(),
            },
        });
        base.add_metadata_object(base_catalog);

        let mut extension = Configuration::new("Extension");
        let mut extension_catalog = MetadataObject::new(MdoType::Catalog, "Номенклатура");
        extension_catalog.add_attribute(crate::metadata_object::Attribute {
            name: "БУС_Артикул".to_string(),
            name_en: None,
            attr_type: AttributeType::String { length: Some(25) },
        });
        extension.add_metadata_object(extension_catalog);

        let merged = base.merged_with_extension(&extension);
        let catalog =
            merged.find_metadata_object(MdoType::Catalog, "Номенклатура").expect("merged catalog");

        assert!(catalog.find_attribute("Родитель").is_some());
        assert!(catalog.find_attribute("БУС_Артикул").is_some());
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
