//! HTTPService XML parser

use crate::error::{MetadataError, Result};
use crate::http_service::{
    HTTPService, HTTPServiceBuilder, HTTPServiceMethodBuilder, HTTPServiceURLTemplateBuilder,
};

use super::helpers::{child_text, find_child, find_mdo_element, parse_xml};

/// Parse HTTPService XML from Designer format
pub fn parse_http_service_xml(xml: &str, name: &str) -> Result<HTTPService> {
    let _span = tracing::debug_span!("parse_http_service_xml", name).entered();

    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No HTTPService element found".to_string()))?;

    let props = find_child(mdo, "Properties").ok_or_else(|| {
        MetadataError::InvalidFormat("HTTPService missing Properties".to_string())
    })?;

    let svc_name = child_text(props, "Name").unwrap_or("").to_string();
    let root_url = child_text(props, "RootURL").unwrap_or("").to_string();

    let mut builder = HTTPServiceBuilder::new()
        .name(&svc_name)
        .root_url(&root_url)
        .uri(format!("HTTPServices/{}/Ext/Module.bsl", name));

    if let Some(child_objects) = find_child(mdo, "ChildObjects") {
        for url_template_node in child_objects
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "URLTemplate")
        {
            let tpl_props = find_child(url_template_node, "Properties").ok_or_else(|| {
                MetadataError::InvalidFormat("URLTemplate missing Properties".to_string())
            })?;

            let tpl_name = child_text(tpl_props, "Name").unwrap_or("").to_string();
            let template = child_text(tpl_props, "Template").unwrap_or("").to_string();

            let mut template_builder =
                HTTPServiceURLTemplateBuilder::new().name(&tpl_name).template(&template);

            if let Some(tpl_children) = find_child(url_template_node, "ChildObjects") {
                for method_node in tpl_children
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "Method")
                {
                    let method_props = find_child(method_node, "Properties").ok_or_else(|| {
                        MetadataError::InvalidFormat("Method missing Properties".to_string())
                    })?;

                    let method_name = child_text(method_props, "Name").unwrap_or("").to_string();
                    let http_method =
                        child_text(method_props, "HTTPMethod").unwrap_or("").to_string();
                    let handler = child_text(method_props, "Handler").unwrap_or("").to_string();

                    let method = HTTPServiceMethodBuilder::new()
                        .name(&method_name)
                        .http_method(&http_method)
                        .handler(&handler)
                        .build();
                    template_builder = template_builder.add_method(method);
                }
            }

            builder = builder.add_url_template(template_builder.build());
        }
    }

    let http_service = builder.build();

    tracing::debug!(
        service_name = %http_service.name(),
        root_url = %http_service.root_url(),
        url_templates = http_service.url_templates().len(),
        "parsed HTTP service"
    );

    Ok(http_service)
}
