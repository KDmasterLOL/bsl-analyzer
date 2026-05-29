use crate::enums::{ModuleType, ObjectBelonging, SupportVariant};
use std::any::Any;
use uuid::Uuid;

pub trait MdObject: Any {
    fn uuid(&self) -> &Uuid;

    fn name(&self) -> &str;

    fn comment(&self) -> Option<&str>;

    fn object_belonging(&self) -> ObjectBelonging;

    fn support_variant(&self) -> SupportVariant;

    fn as_any(&self) -> &dyn Any;
}

pub trait Module: MdObject {
    fn module_type(&self) -> ModuleType;

    fn uri(&self) -> Option<&str>;

    fn is_protected(&self) -> bool;
}
