#![allow(missing_docs)]

pub mod common_module;
pub mod configuration;
pub mod defined_type;
pub mod dimension;
pub mod enums;
pub mod error;
pub mod event_subscription;
pub mod form;
pub mod http_service;
pub mod integration_service;
pub mod loader;
pub mod metadata_object;
pub mod metadata_resolver;
pub mod register;
pub mod role;
pub mod scheduled_job;
pub mod subsystem;
pub mod tabular_section;
pub mod traits;
pub mod web_service;
pub mod xml_parser;

pub use common_module::{CommonModule, CommonModuleBuilder};
pub use configuration::Configuration;
pub use defined_type::{DefinedType, DefinedTypeBuilder};
pub use dimension::{Dimension, DimensionBuilder};
pub use enums::{
    CodeSeries, FormType, ModuleType, ObjectBelonging, ReturnValueReuse, SupportVariant,
};
pub use error::{MetadataError, Result};
pub use event_subscription::{EventSubscription, EventSubscriptionHandler};
pub use form::{
    Form, FormAttribute, FormAttributeColumn, FormElement, FormElementKind, FormEventHandler,
};
pub use http_service::{
    HTTPService, HTTPServiceBuilder, HTTPServiceMethod, HTTPServiceMethodBuilder,
    HTTPServiceURLTemplate, HTTPServiceURLTemplateBuilder,
};
pub use integration_service::{
    IntegrationService, IntegrationServiceBuilder, IntegrationServiceChannel,
    IntegrationServiceChannelBuilder,
};
pub use loader::{
    discover_common_module_structure, discover_defined_type_structure, discover_metadata_structure,
    discover_register_structure, load_from_directory, parse_common_module_from_text,
    parse_defined_type_from_text, parse_metadata_object_from_texts, parse_register_from_text,
    DiscoveredCommonModule, DiscoveredDefinedType, DiscoveredMdo,
};
pub use metadata_object::{
    is_standard_attribute_name, Attribute, AttributeType, MdoType, MetadataObject, Name,
    PlatformValueType,
};
pub use metadata_resolver::{
    resolve_defined_type_terminal, MetadataResolver, QueryMetadataResolver,
};
pub use register::{
    AccumulationRegisterType, Register, RegisterAttribute, RegisterBuilder, RegisterPeriodicity,
    RegisterResource,
};
pub use role::{Role, RoleData, RoleObjectRef};
pub use scheduled_job::{ScheduledJob, ScheduledJobHandler};
pub use subsystem::Subsystem;
pub use tabular_section::{TabularSection, TabularSectionAttribute};
pub use traits::{MdObject, Module};
pub use uuid::Uuid;
pub use web_service::{
    WebService, WebServiceBuilder, WebServiceOperation, WebServiceOperationBuilder,
    WebServiceParameter,
};
