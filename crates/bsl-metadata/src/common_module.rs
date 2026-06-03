use crate::enums::{ModuleType, ObjectBelonging, ReturnValueReuse, SupportVariant};
use crate::traits::{MdObject, Module};
use serde::{Deserialize, Serialize};
use std::any::Any;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommonModule {
    #[serde(rename = "uuid")]
    uuid: Uuid,

    #[serde(rename = "name")]
    name: String,

    #[serde(rename = "comment", default, skip_serializing_if = "Option::is_none")]
    comment: Option<String>,

    #[serde(rename = "uri", default, skip_serializing_if = "Option::is_none")]
    uri: Option<String>,

    #[serde(rename = "objectBelonging", default)]
    object_belonging: ObjectBelonging,

    #[serde(rename = "supportVariant", default)]
    support_variant: SupportVariant,

    #[serde(rename = "protected", default)]
    protected: bool,

    #[serde(rename = "server", default)]
    server: bool,

    #[serde(rename = "global", default)]
    global: bool,

    #[serde(rename = "clientManagedApplication", default)]
    client_managed_application: bool,

    #[serde(rename = "clientOrdinaryApplication", default)]
    client_ordinary_application: bool,

    #[serde(rename = "externalConnection", default)]
    external_connection: bool,

    #[serde(rename = "serverCall", default)]
    server_call: bool,

    #[serde(rename = "privileged", default)]
    privileged: bool,

    #[serde(rename = "returnValuesReuse", default)]
    return_values_reuse: ReturnValueReuse,
}

impl CommonModule {
    pub fn builder() -> CommonModuleBuilder {
        CommonModuleBuilder::default()
    }

    pub fn return_values_reuse(&self) -> ReturnValueReuse {
        self.return_values_reuse
    }

    pub fn is_server(&self) -> bool {
        self.server
    }

    pub fn is_global(&self) -> bool {
        self.global
    }

    pub fn is_client_managed_application(&self) -> bool {
        self.client_managed_application
    }

    pub fn is_client_ordinary_application(&self) -> bool {
        self.client_ordinary_application
    }

    pub fn is_external_connection(&self) -> bool {
        self.external_connection
    }

    pub fn is_server_call(&self) -> bool {
        self.server_call
    }

    pub fn is_privileged(&self) -> bool {
        self.privileged
    }
}

impl MdObject for CommonModule {
    fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }

    fn object_belonging(&self) -> ObjectBelonging {
        self.object_belonging
    }

    fn support_variant(&self) -> SupportVariant {
        self.support_variant
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Module for CommonModule {
    fn module_type(&self) -> ModuleType {
        ModuleType::CommonModule
    }

    fn uri(&self) -> Option<&str> {
        self.uri.as_deref()
    }

    fn is_protected(&self) -> bool {
        self.protected
    }
}

#[derive(Debug, Default)]
pub struct CommonModuleBuilder {
    uuid: Option<Uuid>,
    name: Option<String>,
    comment: Option<String>,
    uri: Option<String>,
    object_belonging: ObjectBelonging,
    support_variant: SupportVariant,
    protected: bool,
    server: bool,
    global: bool,
    client_managed_application: bool,
    client_ordinary_application: bool,
    external_connection: bool,
    server_call: bool,
    privileged: bool,
    return_values_reuse: ReturnValueReuse,
}

impl CommonModuleBuilder {
    pub fn uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = Some(uuid);
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }

    pub fn uri(mut self, uri: Option<impl Into<String>>) -> Self {
        self.uri = uri.map(|s| s.into());
        self
    }

    pub fn return_values_reuse(mut self, reuse: ReturnValueReuse) -> Self {
        self.return_values_reuse = reuse;
        self
    }

    pub fn server(mut self, server: bool) -> Self {
        self.server = server;
        self
    }

    pub fn global(mut self, global: bool) -> Self {
        self.global = global;
        self
    }

    pub fn privileged(mut self, privileged: bool) -> Self {
        self.privileged = privileged;
        self
    }

    pub fn client_managed_application(mut self, value: bool) -> Self {
        self.client_managed_application = value;
        self
    }

    pub fn client_ordinary_application(mut self, value: bool) -> Self {
        self.client_ordinary_application = value;
        self
    }

    pub fn external_connection(mut self, value: bool) -> Self {
        self.external_connection = value;
        self
    }

    pub fn server_call(mut self, value: bool) -> Self {
        self.server_call = value;
        self
    }

    pub fn protected(mut self, value: bool) -> Self {
        self.protected = value;
        self
    }

    pub fn build(self) -> CommonModule {
        CommonModule {
            uuid: self.uuid.unwrap_or_else(Uuid::new_v4),
            name: self.name.unwrap_or_default(),
            comment: self.comment,
            uri: self.uri,
            object_belonging: self.object_belonging,
            support_variant: self.support_variant,
            protected: self.protected,
            server: self.server,
            global: self.global,
            client_managed_application: self.client_managed_application,
            client_ordinary_application: self.client_ordinary_application,
            external_connection: self.external_connection,
            server_call: self.server_call,
            privileged: self.privileged,
            return_values_reuse: self.return_values_reuse,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_module_builder() {
        let module = CommonModule::builder()
            .name("ТестовыйМодуль")
            .return_values_reuse(ReturnValueReuse::DuringRequest)
            .server(true)
            .global(true)
            .build();

        assert_eq!(module.name(), "ТестовыйМодуль");
        assert_eq!(module.return_values_reuse(), ReturnValueReuse::DuringRequest);
        assert!(module.is_server());
        assert!(module.is_global());
        assert_eq!(module.module_type(), ModuleType::CommonModule);
    }

    #[test]
    fn test_md_object_trait() {
        let module = CommonModule::builder().name("TestModule").build();

        assert_eq!(module.name(), "TestModule");
        assert_eq!(module.object_belonging(), ObjectBelonging::Own);
    }
}
