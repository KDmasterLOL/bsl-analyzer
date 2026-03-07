//! WebService XML parser

use crate::error::Result;
use crate::web_service::{
    WebService, WebServiceBuilder, WebServiceOperationBuilder, WebServiceParameter,
};

use super::serde_types::WebServiceRoot;

/// Parse WebService XML from Designer format
pub fn parse_web_service_xml(xml: &str, name: &str) -> Result<WebService> {
    let _span = tracing::debug_span!("parse_web_service_xml", name).entered();

    let root: WebServiceRoot = quick_xml::de::from_str(xml)?;
    let props = &root.web_service.properties;

    let mut builder = WebServiceBuilder::new()
        .name(&props.name)
        .namespace(&props.namespace)
        .uri(format!("WebServices/{}/Ext/Module.bsl", name));

    if let Some(child_objects) = &root.web_service.child_objects {
        for operation_xml in &child_objects.operations {
            let op_props = &operation_xml.properties;

            let mut op_builder = WebServiceOperationBuilder::new()
                .name(&op_props.name)
                .procedure_name(&op_props.procedure_name);

            if let Some(op_children) = &operation_xml.child_objects {
                for param_xml in &op_children.parameters {
                    let param = WebServiceParameter::new(&param_xml.properties.name);
                    op_builder = op_builder.add_parameter(param);
                }
            }

            builder = builder.add_operation(op_builder.build());
        }
    }

    let web_service = builder.build();

    tracing::debug!(
        service_name = %web_service.name(),
        namespace = %web_service.namespace(),
        operations = web_service.operations().len(),
        "parsed web service"
    );

    Ok(web_service)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <WebService uuid="0b4a4c9c-76e9-455c-9471-249051a8301d">
        <Properties>
            <Name>WebСервис1</Name>
            <Namespace>test.com</Namespace>
        </Properties>
        <ChildObjects>
            <Operation uuid="bc99d837-aee6-40ee-8940-3a81dddf477c">
                <Properties>
                    <Name>Операция1</Name>
                    <ProcedureName>Операция1</ProcedureName>
                </Properties>
                <ChildObjects/>
            </Operation>
            <Operation uuid="bc09d837-aee6-40ee-8940-3a81dddf477c">
                <Properties>
                    <Name>ОперацияБезОбработчика</Name>
                    <ProcedureName/>
                </Properties>
                <ChildObjects/>
            </Operation>
            <Operation uuid="bc19d837-aee6-40ee-8940-3a81dddf477c">
                <Properties>
                    <Name>ОперацияСНесуществующимОбработчиком</Name>
                    <ProcedureName>НесуществующийОбработчик1</ProcedureName>
                </Properties>
                <ChildObjects/>
            </Operation>
        </ChildObjects>
    </WebService>
</MetaDataObject>"#;

    #[test]
    fn test_parse_web_service_xml() {
        let web_service = parse_web_service_xml(SAMPLE_XML, "WebСервис1").unwrap();

        assert_eq!(web_service.name(), "WebСервис1");
        assert_eq!(web_service.namespace(), "test.com");
        assert_eq!(web_service.operations().len(), 3);

        let op1 = &web_service.operations()[0];
        assert_eq!(op1.name(), "Операция1");
        assert_eq!(op1.procedure_name(), "Операция1");
        assert!(!op1.is_handler_empty());

        let op2 = &web_service.operations()[1];
        assert_eq!(op2.name(), "ОперацияБезОбработчика");
        assert!(op2.is_handler_empty());

        let op3 = &web_service.operations()[2];
        assert_eq!(op3.name(), "ОперацияСНесуществующимОбработчиком");
        assert_eq!(op3.procedure_name(), "НесуществующийОбработчик1");
        assert!(!op3.is_handler_empty());
    }
}
