//! Traits for BSL metadata objects

use crate::enums::{ModuleType, ObjectBelonging, SupportVariant};
use std::any::Any;
use uuid::Uuid;

/// Base trait for all metadata objects
pub trait MdObject: Any {
    /// Get unique object identifier
    fn uuid(&self) -> &Uuid;

    /// Get object name
    fn name(&self) -> &str;

    /// Get object comment
    fn comment(&self) -> Option<&str>;

    /// Get object belonging (own or adopted)
    fn object_belonging(&self) -> ObjectBelonging;

    /// Get support variant
    fn support_variant(&self) -> SupportVariant;

    /// Allow downcasting to concrete types
    fn as_any(&self) -> &dyn Any;
}

/// Trait for module-like metadata objects
pub trait Module: MdObject {
    /// Get module type
    fn module_type(&self) -> ModuleType;

    /// Get module URI (path to .bsl file)
    fn uri(&self) -> Option<&str>;

    /// Check if module is password-protected
    fn is_protected(&self) -> bool;
}
