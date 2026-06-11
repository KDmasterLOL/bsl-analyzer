use crate::error::{MetadataError, Result};
use crate::integration_service::{
    IntegrationService, IntegrationServiceBuilder, IntegrationServiceChannelBuilder,
};

use super::helpers::{child_text, find_child, find_mdo_element, parse_xml};

pub fn parse_integration_service_xml(xml: &str, name: &str) -> Result<IntegrationService> {
    let _span = tracing::debug_span!("parse_integration_service_xml", name).entered();

    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc).ok_or_else(|| {
        MetadataError::InvalidFormat("No IntegrationService element found".to_string())
    })?;

    let props = find_child(mdo, "Properties").ok_or_else(|| {
        MetadataError::InvalidFormat("IntegrationService missing Properties".to_string())
    })?;

    let svc_name = child_text(props, "Name").unwrap_or(name).to_string();

    let mut builder = IntegrationServiceBuilder::new().name(&svc_name);

    if let Some(child_objects) = find_child(mdo, "ChildObjects") {
        for channel_node in child_objects
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "IntegrationServiceChannel")
        {
            let Some(ch_props) = find_child(channel_node, "Properties") else {
                continue;
            };
            let ch_name = child_text(ch_props, "Name").unwrap_or("").to_string();
            let handler =
                child_text(ch_props, "ReceiveMessageProcessing").unwrap_or("").to_string();

            builder = builder.add_channel(
                IntegrationServiceChannelBuilder::new()
                    .name(&ch_name)
                    .receive_message_processing(&handler)
                    .build(),
            );
        }
    }

    let service = builder.build();

    tracing::debug!(
        service_name = %service.name(),
        channels = service.channels().len(),
        "parsed integration service"
    );

    Ok(service)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
    <IntegrationService uuid="c512a1cd-1240-4e46-8bad-8b7b27c5c25a">
        <Properties>
            <Name>ОбменСообщениями</Name>
        </Properties>
        <ChildObjects>
            <IntegrationServiceChannel uuid="1ef0581c-b1d8-4115-87f1-7856f6c06bb6">
                <Properties>
                    <Name>input_from_SM_normal_priority</Name>
                    <MessageDirection>Receive</MessageDirection>
                    <ReceiveMessageProcessing>ОбработатьСообщениеОбычныйПриоритет</ReceiveMessageProcessing>
                </Properties>
            </IntegrationServiceChannel>
            <IntegrationServiceChannel uuid="b017ac62-a4a2-47bd-b963-50e0764a7d4e">
                <Properties>
                    <Name>output_to_SM_high_priority</Name>
                    <MessageDirection>Send</MessageDirection>
                    <ReceiveMessageProcessing/>
                </Properties>
            </IntegrationServiceChannel>
        </ChildObjects>
    </IntegrationService>
</MetaDataObject>"#;

    #[test]
    fn test_parse_integration_service_xml() {
        let service = parse_integration_service_xml(SAMPLE_XML, "ОбменСообщениями").unwrap();

        assert_eq!(service.name(), "ОбменСообщениями");
        assert_eq!(service.channels().len(), 2);

        let handlers: Vec<_> = service.receive_handlers().collect();
        assert_eq!(handlers, vec!["ОбработатьСообщениеОбычныйПриоритет"]);
    }
}
