//! # BSL Metadata Infrastructure
//!
//! This crate provides structures and utilities for working with 1C:Enterprise
//! metadata (configurations, extensions, common modules, etc.) in Designer format.
//!
//!
//! ## Features
//!
//! - **Designer format support**: Load metadata from Designer format directory structure
//! - **Type-safe metadata access**: Strongly-typed API for all metadata objects
//! - **High performance**: Parsing optimized for large configurations (< 1 second load time)
//! - **Bilingual support**: Handles both Russian and English metadata
//!
//! ## Designer Format Structure
//!
//! Designer format uses a specific directory layout:
//!
//! ```text
//! Configuration.xml                      # Root configuration
//! ConfigDumpInfo.xml                     # Dump information
//!
//! CommonModules/
//! ├── <Name>.xml                         # XML NEXT TO folder
//! └── <Name>/                            # Folder with code
//!     └── Ext/
//!         └── Module.bsl                 # Code INSIDE Ext/
//!
//! Catalogs/
//! ├── <Name>.xml                         # XML NEXT TO folder
//! └── <Name>/                            # Folder with code
//!     └── Ext/
//!         ├── ManagerModule.bsl
//!         └── ObjectModule.bsl
//! ```
//!
//! ## Usage Example
//!
//! ```no_run
//! use bsl_metadata::{load_from_directory, Result};
//! use bsl_metadata::traits::{MdObject, Module};  // Import traits for methods
//!
//! fn main() -> Result<()> {
//!     // Load configuration from directory
//!     let config = load_from_directory("/path/to/configuration")?;
//!
//!     // Access common modules
//!     println!("Found {} common modules", config.common_modules().len());
//!
//!     // Find specific module
//!     if let Some(module) = config.find_common_module("ОбщегоНазначения") {
//!         println!("Module: {}", module.name());
//!         println!("Server: {}", module.is_server());
//!         println!("Global: {}", module.is_global());
//!
//!         if let Some(uri) = module.uri() {
//!             println!("URI: {}", uri);
//!         }
//!     }
//!
//!     // Access metadata objects
//!     for obj in config.metadata_objects() {
//!         println!("{:?}: {}", obj.mdo_type, obj.name);
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Performance
//!
//! - **Load time**: < 1 second for typical configuration (3-10 CommonModules)
//! - **Memory efficient**: Uses Arc-based sharing for efficient cloning
//! - **Salsa integration**: Designed for incremental computation with caching
//!
//! ## Integration with Salsa
//!
//! This crate is designed to work with Salsa for incremental computation.
//! All main structures implement `PartialEq` for efficient caching:
//!
//! ```no_run
//! # use bsl_metadata::Configuration;
//! # use std::sync::Arc;
//! // Salsa tracks changes efficiently
//! let config1 = Arc::new(Configuration::new("Config"));
//! let config2 = Arc::new(Configuration::new("Config"));
//!
//! // Configurations are comparable
//! assert_eq!(config1, config2);
//! ```

#![warn(missing_docs)]

pub mod common_module;
pub mod configuration;
pub mod defined_type;
pub mod dimension;
pub mod enums;
pub mod error;
pub mod event_subscription;
pub mod form;
pub mod http_service;
pub mod loader;
pub mod metadata_object;
pub mod metadata_resolver;
pub mod register;
pub mod role;
pub mod scheduled_job;
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
pub use loader::load_from_directory;
pub use metadata_object::{Attribute, AttributeType, MdoType, MetadataObject};
pub use metadata_resolver::{resolve_defined_type_terminal, MetadataResolver};
pub use register::{
    AccumulationRegisterType, Register, RegisterAttribute, RegisterBuilder, RegisterPeriodicity,
    RegisterResource,
};
pub use role::{Role, RoleData};
pub use scheduled_job::{ScheduledJob, ScheduledJobHandler};
pub use tabular_section::{TabularSection, TabularSectionAttribute};
pub use traits::{MdObject, Module};
// Re-export the `Uuid` type so downstream crates can construct
// `DefinedType`s in tests without taking a separate dependency on the
// `uuid` crate.
pub use uuid::Uuid;
pub use web_service::{
    WebService, WebServiceBuilder, WebServiceOperation, WebServiceOperationBuilder,
    WebServiceParameter,
};
