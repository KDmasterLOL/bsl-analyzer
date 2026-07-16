use crate::common_module::CommonModule;
use crate::defined_type::DefinedType;
use crate::error::Result;
use crate::event_subscription::EventSubscription;
use crate::http_service::HTTPService;
use crate::metadata_object::{AttributeType, MdoType, MetadataObject, Name};
use crate::register::Register;
use crate::role::Role;
use crate::scheduled_job::ScheduledJob;
use crate::traits::{MdObject, Module};
use crate::web_service::WebService;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use stdx::case::CaseExt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Configuration {
    #[serde(rename = "uuid", default = "Uuid::new_v4")]
    uuid: Uuid,

    #[serde(rename = "name")]
    name: String,

    #[serde(rename = "commonModules", default)]
    common_modules: Vec<CommonModule>,

    #[serde(rename = "metadataObjects", default)]
    metadata_objects: Vec<MetadataObject>,

    #[serde(rename = "registers", default)]
    registers: Vec<Register>,

    #[serde(rename = "eventSubscriptions", default)]
    event_subscriptions: Vec<EventSubscription>,

    #[serde(rename = "definedTypes", default)]
    defined_types: Vec<DefinedType>,

    #[serde(rename = "scheduledJobs", default)]
    scheduled_jobs: Vec<ScheduledJob>,

    #[serde(rename = "roles", default)]
    roles: Vec<Role>,

    #[serde(rename = "subsystems", default)]
    subsystems: Vec<crate::subsystem::Subsystem>,

    #[serde(rename = "httpServices", default)]
    http_services: Vec<HTTPService>,

    #[serde(rename = "webServices", default)]
    web_services: Vec<WebService>,

    #[serde(rename = "integrationServices", default)]
    integration_services: Vec<crate::integration_service::IntegrationService>,

    #[serde(skip)]
    uri_to_module: HashMap<String, usize>,

    /// Lowercased common-module URI -> index, so a case-insensitive URI lookup is
    /// O(1) instead of an O(all-modules) scan that re-lowercased every module's
    /// (Cyrillic) joined path on each file resolution.
    #[serde(skip)]
    uri_lower_to_common_module: HashMap<String, usize>,

    #[serde(skip)]
    name_to_common_module: HashMap<String, usize>,

    #[serde(skip)]
    name_to_register: HashMap<String, usize>,

    #[serde(skip)]
    name_to_event_subscription: HashMap<String, usize>,

    #[serde(skip)]
    name_to_defined_type: HashMap<String, usize>,

    #[serde(skip)]
    name_to_scheduled_job: HashMap<String, usize>,

    #[serde(skip)]
    name_to_role: HashMap<String, usize>,

    #[serde(skip)]
    name_to_http_service: HashMap<String, usize>,

    #[serde(skip)]
    name_to_web_service: HashMap<String, usize>,

    #[serde(skip)]
    name_to_integration_service: HashMap<String, usize>,

    /// `(mdo_type, lowercased name) -> index`, so [`Configuration::find_metadata_object`]
    /// is O(1) instead of an O(all-objects) scan that lowercased every object's
    /// (Cyrillic) name on each lookup.
    #[serde(skip)]
    metadata_objects_by_key: HashMap<(MdoType, String), usize>,

    #[serde(skip)]
    recorders_by_register: HashMap<(MdoType, Name), Vec<Name>>,

    #[serde(rename = "useManagedFormInOrdinaryApplication", default)]
    use_managed_form_in_ordinary_application: bool,

    #[serde(rename = "useOrdinaryFormInManagedApplication", default)]
    use_ordinary_form_in_managed_application: bool,
}

impl PartialEq for Configuration {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
            && self.name == other.name
            && self.common_modules == other.common_modules
            && self.metadata_objects == other.metadata_objects
            && self.registers == other.registers
            && self.event_subscriptions == other.event_subscriptions
            && self.defined_types == other.defined_types
            && self.scheduled_jobs == other.scheduled_jobs
            && self.roles == other.roles
            && self.subsystems == other.subsystems
            && self.http_services == other.http_services
            && self.web_services == other.web_services
            && self.integration_services == other.integration_services
            && self.use_managed_form_in_ordinary_application
                == other.use_managed_form_in_ordinary_application
            && self.use_ordinary_form_in_managed_application
                == other.use_ordinary_form_in_managed_application
    }
}

impl Configuration {
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
            subsystems: Vec::new(),
            uri_to_module: HashMap::new(),
            uri_lower_to_common_module: HashMap::new(),
            name_to_common_module: HashMap::new(),
            name_to_register: HashMap::new(),
            name_to_event_subscription: HashMap::new(),
            name_to_defined_type: HashMap::new(),
            name_to_scheduled_job: HashMap::new(),
            name_to_role: HashMap::new(),
            name_to_http_service: HashMap::new(),
            name_to_web_service: HashMap::new(),
            name_to_integration_service: HashMap::new(),
            metadata_objects_by_key: HashMap::new(),
            recorders_by_register: HashMap::new(),
            use_managed_form_in_ordinary_application: false,
            use_ordinary_form_in_managed_application: false,
            http_services: Vec::new(),
            web_services: Vec::new(),
            integration_services: Vec::new(),
        }
    }

    pub fn from_xml_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_xml_str(&content)
    }

    pub fn from_xml_str(xml: &str) -> Result<Self> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| {
            crate::error::MetadataError::InvalidFormat(format!("XML parse error: {}", e))
        })?;

        let root = doc.root_element();
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

    #[allow(dead_code)]
    fn build_caches(&mut self) {
        self.uri_to_module.clear();
        self.uri_lower_to_common_module.clear();
        self.name_to_common_module.clear();
        self.name_to_register.clear();
        self.name_to_event_subscription.clear();
        self.name_to_defined_type.clear();
        self.name_to_scheduled_job.clear();
        self.name_to_role.clear();
        self.name_to_http_service.clear();
        self.name_to_web_service.clear();
        self.name_to_integration_service.clear();
        self.metadata_objects_by_key.clear();
        self.recorders_by_register.clear();

        for (idx, object) in self.metadata_objects.iter().enumerate() {
            // First occurrence wins, matching the replaced `.iter().find()` scan.
            self.metadata_objects_by_key
                .entry((object.mdo_type, object.name.fold_lower()))
                .or_insert(idx);
        }

        for (idx, module) in self.common_modules.iter().enumerate() {
            if let Some(uri) = module.uri() {
                self.uri_to_module.insert(uri.to_string(), idx);
                // First occurrence wins, matching the replaced `.iter().find()` scan.
                self.uri_lower_to_common_module.entry(uri.fold_lower()).or_insert(idx);
            }
            self.name_to_common_module.insert(module.name().fold_lower(), idx);
        }

        for (idx, register) in self.registers.iter().enumerate() {
            self.name_to_register.insert(register.name().fold_lower(), idx);
        }

        for (idx, event_sub) in self.event_subscriptions.iter().enumerate() {
            self.name_to_event_subscription.insert(event_sub.name().fold_lower(), idx);
        }

        for (idx, defined_type) in self.defined_types.iter().enumerate() {
            self.name_to_defined_type.insert(defined_type.name().fold_lower(), idx);
        }

        for (idx, scheduled_job) in self.scheduled_jobs.iter().enumerate() {
            self.name_to_scheduled_job.insert(scheduled_job.name().fold_lower(), idx);
        }

        for (idx, role) in self.roles.iter().enumerate() {
            self.name_to_role.insert(role.name().fold_lower(), idx);
        }

        for (idx, http_service) in self.http_services.iter().enumerate() {
            self.name_to_http_service.insert(http_service.name().fold_lower(), idx);
        }

        for (idx, web_service) in self.web_services.iter().enumerate() {
            self.name_to_web_service.insert(web_service.name().fold_lower(), idx);
        }

        for (idx, integration_service) in self.integration_services.iter().enumerate() {
            self.name_to_integration_service.insert(integration_service.name().fold_lower(), idx);
        }

        for object in &self.metadata_objects {
            index_document_recorders(&mut self.recorders_by_register, object);
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn common_modules(&self) -> &[CommonModule] {
        &self.common_modules
    }

    pub fn find_common_module(&self, name: &str) -> Option<&CommonModule> {
        let name_lower = name.fold_lower();
        self.name_to_common_module.get(&name_lower).and_then(|&idx| self.common_modules.get(idx))
    }

    /// Case-insensitive lookup by root-relative URI. `uri_lower` must already be
    /// lowercased by the caller (the relative path of the module body file).
    pub fn find_common_module_by_uri_lower(&self, uri_lower: &str) -> Option<&CommonModule> {
        self.uri_lower_to_common_module.get(uri_lower).and_then(|&idx| self.common_modules.get(idx))
    }

    pub fn find_module_by_uri(&self, uri: &str) -> Option<&dyn Module> {
        self.uri_to_module
            .get(uri)
            .and_then(|&idx| self.common_modules.get(idx))
            .map(|cm| cm as &dyn Module)
    }

    pub fn find_child_by_uri(&self, uri: &str) -> Option<&dyn MdObject> {
        self.uri_to_module
            .get(uri)
            .and_then(|&idx| self.common_modules.get(idx))
            .map(|cm| cm as &dyn MdObject)
    }

    pub fn add_common_module(&mut self, module: CommonModule) {
        let idx = self.common_modules.len();

        if let Some(uri) = module.uri() {
            self.uri_to_module.insert(uri.to_string(), idx);
            // First occurrence wins, matching the replaced `.iter().find()` scan.
            self.uri_lower_to_common_module.entry(uri.fold_lower()).or_insert(idx);
        }
        self.name_to_common_module.insert(module.name().fold_lower(), idx);

        self.common_modules.push(module);
    }

    pub fn use_managed_form_in_ordinary_application(&self) -> bool {
        self.use_managed_form_in_ordinary_application
    }

    pub fn use_ordinary_form_in_managed_application(&self) -> bool {
        self.use_ordinary_form_in_managed_application
    }

    pub fn set_use_managed_form_in_ordinary_application(&mut self, value: bool) {
        self.use_managed_form_in_ordinary_application = value;
    }

    pub fn set_use_ordinary_form_in_managed_application(&mut self, value: bool) {
        self.use_ordinary_form_in_managed_application = value;
    }

    pub fn metadata_objects(&self) -> &[MetadataObject] {
        &self.metadata_objects
    }

    pub fn add_metadata_object(&mut self, object: MetadataObject) {
        index_document_recorders(&mut self.recorders_by_register, &object);
        let idx = self.metadata_objects.len();
        // First occurrence wins, matching the replaced `.iter().find()` scan: a
        // later same-(type,name) object (e.g. a case-only-differing extension
        // overlay) must not shadow the base object the scan would have returned.
        self.metadata_objects_by_key
            .entry((object.mdo_type, object.name.fold_lower()))
            .or_insert(idx);
        self.metadata_objects.push(object);
    }

    pub fn merge_extension_overlay(&mut self, extension: &Configuration) {
        for ext_obj in &extension.metadata_objects {
            if let Some(base_obj) = self.metadata_objects.iter_mut().find(|obj| {
                obj.mdo_type == ext_obj.mdo_type && obj.name.eq_ignore_ascii_case(&ext_obj.name)
            }) {
                base_obj.apply_extension_overlay(ext_obj);
            } else {
                self.add_metadata_object(ext_obj.clone());
            }
        }

        for ext_reg in &extension.registers {
            if let Some(idx) = self.registers.iter().position(|base| {
                base.mdo_type() == ext_reg.mdo_type()
                    && base.name().eq_ignore_ascii_case(ext_reg.name())
            }) {
                // An extension can add measurements/resources/attributes to a
                // borrowed register, so merge rather than ignore the adopted copy.
                self.registers[idx].apply_extension_overlay(ext_reg);
            } else {
                self.add_register(ext_reg.clone());
            }
        }

        for ext_defined_type in &extension.defined_types {
            if let Some(idx) = self
                .defined_types
                .iter()
                .position(|base| base.name().eq_ignore_ascii_case(ext_defined_type.name()))
            {
                // An extension can refine a borrowed defined type's composition, so
                // take the extension's underlying type rather than ignore it.
                self.defined_types[idx].apply_extension_overlay(ext_defined_type);
            } else {
                self.add_defined_type(ext_defined_type.clone());
            }
        }

        for ext_subsystem in &extension.subsystems {
            if let Some(base) = self
                .subsystems
                .iter_mut()
                .find(|s| s.name().fold_lower() == ext_subsystem.name().fold_lower())
            {
                // An extension can add objects/child subsystems to an existing subsystem.
                base.merge_from(ext_subsystem);
            } else {
                self.add_subsystem(ext_subsystem.clone());
            }
        }

        self.build_caches();
    }

    pub fn merged_with_extension(&self, extension: &Configuration) -> Self {
        let mut merged = self.clone();
        merged.merge_extension_overlay(extension);
        merged
    }

    pub fn has_metadata_object(&self, mdo_type: MdoType, name: &str) -> bool {
        let name_lower = name.fold_lower();

        let result = match mdo_type {
            MdoType::InformationRegister
            | MdoType::AccumulationRegister
            | MdoType::AccountingRegister
            | MdoType::CalculationRegister => self
                .registers
                .iter()
                .any(|reg| reg.mdo_type() == mdo_type && reg.name().fold_lower() == name_lower),
            _ => self
                .metadata_objects
                .iter()
                .any(|obj| obj.mdo_type == mdo_type && obj.name.fold_lower() == name_lower),
        };

        result
    }

    pub fn find_metadata_object(&self, mdo_type: MdoType, name: &str) -> Option<&MetadataObject> {
        let idx = *self.metadata_objects_by_key.get(&(mdo_type, name.fold_lower()))?;
        self.metadata_objects.get(idx)
    }

    pub fn find_constant_type(&self, name: &str) -> Option<&AttributeType> {
        self.find_metadata_object(MdoType::Constant, name)
            .and_then(|mdo| mdo.constant_type.as_ref())
    }

    pub fn registers(&self) -> &[Register] {
        &self.registers
    }

    pub fn find_register(&self, name: &str) -> Option<&Register> {
        let name_lower = name.fold_lower();
        self.name_to_register.get(&name_lower).and_then(|&idx| self.registers.get(idx))
    }

    pub fn find_register_by_type_and_name(
        &self,
        mdo_type: MdoType,
        name: &str,
    ) -> Option<&Register> {
        let name_lower = name.fold_lower();
        self.name_to_register
            .get(&name_lower)
            .and_then(|&idx| self.registers.get(idx).filter(|r| r.mdo_type() == mdo_type))
    }

    pub fn recorders_for_register(&self, parent: MdoType, name: &str) -> &[Name] {
        let key = (parent, name.fold_lower());
        self.recorders_by_register.get(&key).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn add_register(&mut self, register: Register) {
        let idx = self.registers.len();
        self.name_to_register.insert(register.name().fold_lower(), idx);
        self.registers.push(register);
    }

    pub fn event_subscriptions(&self) -> &[EventSubscription] {
        &self.event_subscriptions
    }

    pub fn find_event_subscription(&self, name: &str) -> Option<&EventSubscription> {
        let name_lower = name.fold_lower();
        self.name_to_event_subscription
            .get(&name_lower)
            .and_then(|&idx| self.event_subscriptions.get(idx))
    }

    pub(crate) fn add_event_subscription(&mut self, subscription: EventSubscription) {
        let idx = self.event_subscriptions.len();
        self.name_to_event_subscription.insert(subscription.name().fold_lower(), idx);
        self.event_subscriptions.push(subscription);
    }

    pub fn subsystems(&self) -> &[crate::subsystem::Subsystem] {
        &self.subsystems
    }

    pub(crate) fn add_subsystem(&mut self, subsystem: crate::subsystem::Subsystem) {
        self.subsystems.push(subsystem);
    }

    pub fn defined_types(&self) -> &[DefinedType] {
        &self.defined_types
    }

    pub fn find_defined_type(&self, name: &str) -> Option<&DefinedType> {
        let name_lower = name.fold_lower();
        self.name_to_defined_type.get(&name_lower).and_then(|&idx| self.defined_types.get(idx))
    }

    pub fn add_defined_type(&mut self, defined_type: DefinedType) {
        let idx = self.defined_types.len();
        self.name_to_defined_type.insert(defined_type.name().fold_lower(), idx);
        self.defined_types.push(defined_type);
    }

    pub fn scheduled_jobs(&self) -> &[ScheduledJob] {
        &self.scheduled_jobs
    }

    pub fn find_scheduled_job(&self, name: &str) -> Option<&ScheduledJob> {
        let name_lower = name.fold_lower();
        self.name_to_scheduled_job.get(&name_lower).and_then(|&idx| self.scheduled_jobs.get(idx))
    }

    pub(crate) fn add_scheduled_job(&mut self, job: ScheduledJob) {
        let idx = self.scheduled_jobs.len();
        self.name_to_scheduled_job.insert(job.name().fold_lower(), idx);
        self.scheduled_jobs.push(job);
    }

    pub fn roles(&self) -> &[Role] {
        &self.roles
    }

    pub fn find_role(&self, name: &str) -> Option<&Role> {
        let name_lower = name.fold_lower();
        self.name_to_role.get(&name_lower).and_then(|&idx| self.roles.get(idx))
    }

    pub fn add_role(&mut self, role: Role) {
        let idx = self.roles.len();
        self.name_to_role.insert(role.name().fold_lower(), idx);
        self.roles.push(role);
    }

    pub fn http_services(&self) -> &[HTTPService] {
        &self.http_services
    }

    pub fn find_http_service(&self, name: &str) -> Option<&HTTPService> {
        let name_lower = name.fold_lower();
        self.name_to_http_service.get(&name_lower).and_then(|&idx| self.http_services.get(idx))
    }

    pub(crate) fn add_http_service(&mut self, http_service: HTTPService) {
        let idx = self.http_services.len();
        self.name_to_http_service.insert(http_service.name().fold_lower(), idx);
        self.http_services.push(http_service);
    }

    pub fn web_services(&self) -> &[WebService] {
        &self.web_services
    }

    pub fn find_web_service(&self, name: &str) -> Option<&WebService> {
        let name_lower = name.fold_lower();
        self.name_to_web_service.get(&name_lower).and_then(|&idx| self.web_services.get(idx))
    }

    pub(crate) fn add_web_service(&mut self, web_service: WebService) {
        let idx = self.web_services.len();
        self.name_to_web_service.insert(web_service.name().fold_lower(), idx);
        self.web_services.push(web_service);
    }

    pub fn integration_services(&self) -> &[crate::integration_service::IntegrationService] {
        &self.integration_services
    }

    pub fn find_integration_service(
        &self,
        name: &str,
    ) -> Option<&crate::integration_service::IntegrationService> {
        let name_lower = name.fold_lower();
        self.name_to_integration_service
            .get(&name_lower)
            .and_then(|&idx| self.integration_services.get(idx))
    }

    pub(crate) fn add_integration_service(
        &mut self,
        integration_service: crate::integration_service::IntegrationService,
    ) {
        let idx = self.integration_services.len();
        self.name_to_integration_service.insert(integration_service.name().fold_lower(), idx);
        self.integration_services.push(integration_service);
    }

    /// Heap bytes owned by this configuration, memoised by `ide-db`'s
    /// `load_configuration`/`merged_configuration` for Salsa's `heap_size` hook:
    /// its name, every owned metadata-family vec (recursing into each entry's own
    /// owned payload), and every `name -> index` lookup table `build_caches`
    /// maintains alongside them. New heap-owning fields must be added here too.
    pub fn estimated_heap_size(&self) -> usize {
        let mut bytes = self.name.capacity();

        bytes += stdx::heap::vec_bytes::<CommonModule>(self.common_modules.len())
            + self.common_modules.iter().map(CommonModule::estimated_heap_size).sum::<usize>();
        bytes += stdx::heap::vec_bytes::<MetadataObject>(self.metadata_objects.len())
            + self.metadata_objects.iter().map(MetadataObject::estimated_heap_size).sum::<usize>();
        bytes += stdx::heap::vec_bytes::<Register>(self.registers.len())
            + self.registers.iter().map(Register::estimated_heap_size).sum::<usize>();
        bytes += stdx::heap::vec_bytes::<EventSubscription>(self.event_subscriptions.len())
            + self
                .event_subscriptions
                .iter()
                .map(EventSubscription::estimated_heap_size)
                .sum::<usize>();
        bytes += stdx::heap::vec_bytes::<DefinedType>(self.defined_types.len())
            + self.defined_types.iter().map(DefinedType::estimated_heap_size).sum::<usize>();
        bytes += stdx::heap::vec_bytes::<ScheduledJob>(self.scheduled_jobs.len())
            + self.scheduled_jobs.iter().map(ScheduledJob::estimated_heap_size).sum::<usize>();
        bytes += stdx::heap::vec_bytes::<Role>(self.roles.len())
            + self.roles.iter().map(Role::estimated_heap_size).sum::<usize>();
        bytes += stdx::heap::vec_bytes::<crate::subsystem::Subsystem>(self.subsystems.len())
            + self
                .subsystems
                .iter()
                .map(crate::subsystem::Subsystem::estimated_heap_size)
                .sum::<usize>();
        bytes += stdx::heap::vec_bytes::<HTTPService>(self.http_services.len())
            + self.http_services.iter().map(HTTPService::estimated_heap_size).sum::<usize>();
        bytes += stdx::heap::vec_bytes::<WebService>(self.web_services.len())
            + self.web_services.iter().map(WebService::estimated_heap_size).sum::<usize>();
        bytes += stdx::heap::vec_bytes::<crate::integration_service::IntegrationService>(
            self.integration_services.len(),
        ) + self
            .integration_services
            .iter()
            .map(crate::integration_service::IntegrationService::estimated_heap_size)
            .sum::<usize>();

        bytes += name_index_heap(&self.uri_to_module);
        bytes += name_index_heap(&self.uri_lower_to_common_module);
        bytes += name_index_heap(&self.name_to_common_module);
        bytes += name_index_heap(&self.name_to_register);
        bytes += name_index_heap(&self.name_to_event_subscription);
        bytes += name_index_heap(&self.name_to_defined_type);
        bytes += name_index_heap(&self.name_to_scheduled_job);
        bytes += name_index_heap(&self.name_to_role);
        bytes += name_index_heap(&self.name_to_http_service);
        bytes += name_index_heap(&self.name_to_web_service);
        bytes += name_index_heap(&self.name_to_integration_service);

        bytes +=
            stdx::heap::map_table_bytes::<(MdoType, String), usize>(
                self.metadata_objects_by_key.len(),
            ) + self.metadata_objects_by_key.keys().map(|(_, name)| name.capacity()).sum::<usize>();

        bytes += stdx::heap::map_table_bytes::<(MdoType, Name), Vec<Name>>(
            self.recorders_by_register.len(),
        ) + self
            .recorders_by_register
            .iter()
            .map(|((_, key_name), recorders)| {
                key_name.capacity()
                    + stdx::heap::vec_bytes::<Name>(recorders.len())
                    + recorders.iter().map(String::capacity).sum::<usize>()
            })
            .sum::<usize>();

        bytes
    }
}

/// Heap of a `name -> index` lookup table (every `Configuration` cache follows this
/// shape): the table itself plus the owned lowercased-name keys.
fn name_index_heap(map: &HashMap<String, usize>) -> usize {
    stdx::heap::map_table_bytes::<String, usize>(map.len())
        + map.keys().map(String::capacity).sum::<usize>()
}

fn index_document_recorders(
    recorders_by_register: &mut HashMap<(MdoType, Name), Vec<Name>>,
    object: &MetadataObject,
) {
    if object.mdo_type != MdoType::Document {
        return;
    }

    for (register_type, register_name) in object.register_records() {
        let key = (*register_type, register_name.fold_lower());
        let documents = recorders_by_register.entry(key).or_default();
        if !documents.iter().any(|name| name.eq_ignore_ascii_case(&object.name)) {
            documents.push(object.name.clone());
        }
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

        let found = config.find_common_module("TestModule");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), "TestModule");

        let found_ci = config.find_common_module("testmodule");
        assert!(found_ci.is_some());

        let found_uri = config.find_module_by_uri("CommonModules/TestModule/Ext/Module.bsl");
        assert!(found_uri.is_some());
        assert_eq!(found_uri.unwrap().name(), "TestModule");
    }

    #[test]
    fn find_common_module_by_uri_lower_is_case_insensitive_and_first_wins() {
        let mut config = Configuration::new("Test");

        // Populated via the incremental `add_common_module` (disk-loader) path, not
        // `build_caches`, so the folded URI index must be filled there too.
        let first = CommonModule::builder()
            .name("Первый")
            .uri(Some("CommonModules/Первый/Ext/Module.bsl"))
            .return_values_reuse(ReturnValueReuse::DuringRequest)
            .build();
        config.add_common_module(first);

        // A second module colliding on the (lowercased) URI must not displace the first.
        let shadow = CommonModule::builder()
            .name("Второй")
            .uri(Some("commonmodules/первый/ext/module.bsl"))
            .return_values_reuse(ReturnValueReuse::DuringRequest)
            .build();
        config.add_common_module(shadow);

        let needle = "CommonModules/Первый/Ext/Module.bsl".fold_lower();
        let found = config.find_common_module_by_uri_lower(&needle);
        assert!(found.is_some(), "folded URI index must be populated via add_common_module");
        assert_eq!(found.unwrap().name(), "Первый", "first occurrence wins on URI collision");

        assert!(config.find_common_module_by_uri_lower("nope/missing.bsl").is_none());
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
    fn merge_extension_overlay_merges_borrowed_register_fields() {
        use crate::dimension::Dimension;
        use crate::register::{Register, RegisterAttribute, RegisterResource};
        use uuid::Uuid;

        let mut base = Configuration::new("Base");
        base.add_register(
            Register::builder()
                .name("РегистрСведений1")
                .mdo_type(MdoType::InformationRegister)
                .add_dimension(Dimension::builder().name("Изм1").build())
                .add_resource(RegisterResource::new(Uuid::new_v4(), "Рес1"))
                .build(),
        );

        let mut extension = Configuration::new("Extension");
        extension.add_register(
            Register::builder()
                .name("РегистрСведений1")
                .mdo_type(MdoType::InformationRegister)
                .add_dimension(Dimension::builder().name("Изм2").build())
                .add_resource(RegisterResource::new(Uuid::new_v4(), "Рес2"))
                .add_attribute(RegisterAttribute::new(Uuid::new_v4(), "Рекв1"))
                .build(),
        );

        let merged = base.merged_with_extension(&extension);
        let reg = merged
            .find_register_by_type_and_name(MdoType::InformationRegister, "РегистрСведений1")
            .expect("merged register");

        // An extension that borrows the register adds its measurement, resource and
        // attribute — the base's own fields are preserved (not replaced wholesale).
        let dims: Vec<&str> = reg.dimensions().iter().map(|d| d.name()).collect();
        let res: Vec<&str> = reg.resources().iter().map(|r| r.name()).collect();
        let attrs: Vec<&str> = reg.attributes().iter().map(|a| a.name()).collect();
        assert_eq!(dims, ["Изм1", "Изм2"], "base + extension measurements");
        assert_eq!(res, ["Рес1", "Рес2"], "base + extension resources");
        assert_eq!(attrs, ["Рекв1"], "extension attribute added to the borrowed register");
    }

    #[test]
    fn merge_extension_overlay_refines_borrowed_defined_type() {
        use crate::defined_type::DefinedType;
        use uuid::Uuid;

        let mut base = Configuration::new("Base");
        base.add_defined_type(
            DefinedType::builder()
                .uuid(Uuid::new_v4())
                .name("ОпределяемыйТип1")
                .underlying_type(AttributeType::String { length: Some(10) })
                .build(),
        );

        let refined = AttributeType::Ref {
            mdo_type: MdoType::Catalog,
            name: "Номенклатура".into(),
        };
        let mut extension = Configuration::new("Extension");
        extension.add_defined_type(
            DefinedType::builder()
                .uuid(Uuid::new_v4())
                .name("ОпределяемыйТип1")
                .underlying_type(refined.clone())
                .build(),
        );

        let merged = base.merged_with_extension(&extension);
        let dt = merged.find_defined_type("ОпределяемыйТип1").expect("merged defined type");
        assert_eq!(dt.underlying_type(), &refined, "extension refinement of the defined type wins");
    }

    #[test]
    fn equality_reflects_subsystem_changes() {
        let base = Configuration::new("Cfg");

        let mut with_subsystem = base.clone();
        with_subsystem.add_subsystem(crate::subsystem::Subsystem::new("Продажи"));

        // A subsystem-only difference must be observable: Salsa relies on this
        // equality to decide whether a reload invalidates downstream consumers.
        assert_ne!(base, with_subsystem);

        let mut also_with_subsystem = base.clone();
        also_with_subsystem.add_subsystem(crate::subsystem::Subsystem::new("Продажи"));
        assert_eq!(with_subsystem, also_with_subsystem);
    }

    #[test]
    fn merge_extension_overlay_merges_subsystem_content_and_children() {
        let mut base = Configuration::new("Base");
        base.add_subsystem(
            crate::subsystem::Subsystem::new("Продажи")
                .with_content(vec![(MdoType::Catalog, "Номенклатура".to_string())])
                .with_child_subsystems(vec!["Договоры".to_string()]),
        );

        let mut extension = Configuration::new("Extension");
        extension.add_subsystem(
            crate::subsystem::Subsystem::new("продажи")
                .with_content(vec![(MdoType::Document, "ЗаказПокупателя".to_string())])
                .with_child_subsystems(vec!["договоры".to_string(), "Отчеты".to_string()]),
        );
        extension.add_subsystem(crate::subsystem::Subsystem::new("Сервис"));

        let merged = base.merged_with_extension(&extension);
        let sales = merged
            .subsystems()
            .iter()
            .find(|subsystem| subsystem.name().fold_lower() == "Продажи".fold_lower())
            .expect("merged subsystem");

        assert_eq!(
            sales.content(),
            &[
                (MdoType::Catalog, "Номенклатура".to_string()),
                (MdoType::Document, "ЗаказПокупателя".to_string()),
            ]
        );
        assert_eq!(sales.child_subsystems(), &["Договоры".to_string(), "Отчеты".to_string()]);
        assert!(merged
            .subsystems()
            .iter()
            .any(|subsystem| subsystem.name().fold_lower() == "Сервис".fold_lower()));
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

        let found = config.find_register("РегистрСведений1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), "РегистрСведений1");

        let found_ci = config.find_register("регистрсведений1");
        assert!(found_ci.is_some());

        let found_typed =
            config.find_register_by_type_and_name(MdoType::InformationRegister, "РегистрСведений1");
        assert!(found_typed.is_some());

        let not_found = config
            .find_register_by_type_and_name(MdoType::AccumulationRegister, "РегистрСведений1");
        assert!(not_found.is_none());
    }

    #[test]
    fn recorders_for_register_indexes_document_register_records() {
        let mut config = Configuration::new("Test");
        let mut document = MetadataObject::new(MdoType::Document, "Документ1");
        document.set_register_records(vec![
            (MdoType::InformationRegister, "РегистрСведений1".to_string()),
            (MdoType::AccumulationRegister, "РегистрНакопления1".to_string()),
        ]);

        config.add_metadata_object(document);

        assert_eq!(
            config.recorders_for_register(MdoType::InformationRegister, "РегистрСведений1"),
            &["Документ1".to_string()],
        );
        assert_eq!(
            config.recorders_for_register(MdoType::AccumulationRegister, "регистрнакопления1"),
            &["Документ1".to_string()],
        );
        assert!(config
            .recorders_for_register(MdoType::AccountingRegister, "РегистрБухгалтерии1")
            .is_empty());
    }

    #[test]
    fn overlay_merge_rebuilds_recorder_cache() {
        let doc_name = "Документ1";
        let mut base = Configuration::new("Base");
        let mut base_document = MetadataObject::new(MdoType::Document, doc_name);
        base_document.set_register_records(vec![(MdoType::InformationRegister, "A".to_string())]);
        base.add_metadata_object(base_document);

        let mut extension = Configuration::new("Extension");
        let mut overlay_document = MetadataObject::new(MdoType::Document, doc_name);
        overlay_document
            .set_register_records(vec![(MdoType::InformationRegister, "B".to_string())]);
        extension.add_metadata_object(overlay_document);

        let merged = base.merged_with_extension(&extension);

        assert_eq!(
            merged.recorders_for_register(MdoType::InformationRegister, "B"),
            &[doc_name.to_string()],
        );
        assert!(merged.recorders_for_register(MdoType::InformationRegister, "A").is_empty());
    }

    #[test]
    fn configuration_heap_counts_objects_and_name_indexes() {
        let mut config = Configuration::new("ТестоваяКонфигурация");
        let name_capacity = config.name.capacity();

        let mut catalog = MetadataObject::new(MdoType::Catalog, "Номенклатура");
        catalog.add_attribute(crate::metadata_object::Attribute {
            name: "Артикул".to_string(),
            name_en: None,
            attr_type: AttributeType::String { length: Some(25) },
        });
        let catalog_name_capacity = catalog.name.capacity();
        config.add_metadata_object(catalog);

        let module = CommonModule::builder().name("ОбщийМодуль1").global(true).build();
        // `name` is private, so probe via the public accessor: `.len()` is a safe
        // lower bound on the real (private) `.capacity()`.
        let module_name_floor = module.name().len();
        config.add_common_module(module);

        let owned_floor = name_capacity + catalog_name_capacity + module_name_floor;

        let bytes = config.estimated_heap_size();
        // At least the owned name strings on the objects themselves (the name
        // indexes and vec backing stores add further bytes on top); well under
        // 8 KiB for two small entries plus their lookup-table entries.
        assert!(bytes > owned_floor);
        assert!(bytes < 8 * 1024);
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
