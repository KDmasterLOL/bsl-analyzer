//! WebService XML parser

use crate::error::{MetadataError, Result};
use crate::web_service::{
    WebService, WebServiceBuilder, WebServiceOperationBuilder, WebServiceParameter,
};

use super::helpers::{child_text, find_child, find_mdo_element, parse_xml};

/// Parse WebService XML from Designer format
pub fn parse_web_service_xml(xml: &str, name: &str) -> Result<WebService> {
    let _span = tracing::debug_span!("parse_web_service_xml", name).entered();

    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No WebService element found".to_string()))?;

    let props = find_child(mdo, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("WebService missing Properties".to_string()))?;

    let svc_name = child_text(props, "Name").unwrap_or("").to_string();
    let namespace = child_text(props, "Namespace").unwrap_or("").to_string();

    let mut builder = WebServiceBuilder::new()
        .name(&svc_name)
        .namespace(&namespace)
        .uri(format!("WebServices/{}/Ext/Module.bsl", name));

    if let Some(child_objects) = find_child(mdo, "ChildObjects") {
        for operation_node in child_objects
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Operation")
        {
            let op_props = find_child(operation_node, "Properties").ok_or_else(|| {
                MetadataError::InvalidFormat("Operation missing Properties".to_string())
            })?;

            let op_name = child_text(op_props, "Name").unwrap_or("").to_string();
            let procedure_name = child_text(op_props, "ProcedureName").unwrap_or("").to_string();

            let mut op_builder =
                WebServiceOperationBuilder::new().name(&op_name).procedure_name(&procedure_name);

            if let Some(op_children) = find_child(operation_node, "ChildObjects") {
                for param_node in op_children
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "Parameter")
                {
                    let param_props = find_child(param_node, "Properties").ok_or_else(|| {
                        MetadataError::InvalidFormat("Parameter missing Properties".to_string())
                    })?;
                    let param_name = child_text(param_props, "Name").unwrap_or("").to_string();
                    let param = WebServiceParameter::new(&param_name);
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
