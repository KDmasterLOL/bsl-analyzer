//! HTTPService XML parser

use crate::error::Result;
use crate::http_service::{
    HTTPService, HTTPServiceBuilder, HTTPServiceMethodBuilder, HTTPServiceURLTemplateBuilder,
};

use super::serde_types::HTTPServiceRoot;

/// Parse HTTPService XML from Designer format
pub fn parse_http_service_xml(xml: &str, name: &str) -> Result<HTTPService> {
    let _span = tracing::debug_span!("parse_http_service_xml", name).entered();

    let root: HTTPServiceRoot = quick_xml::de::from_str(xml)?;
    let props = &root.http_service.properties;

    let mut builder = HTTPServiceBuilder::new()
        .name(&props.name)
        .root_url(&props.root_url)
        .uri(format!("HTTPServices/{}/Ext/Module.bsl", name));

    if let Some(child_objects) = &root.http_service.child_objects {
        for url_template_xml in &child_objects.url_templates {
            let url_props = &url_template_xml.properties;

            let mut template_builder = HTTPServiceURLTemplateBuilder::new()
                .name(&url_props.name)
                .template(&url_props.template);

            if let Some(template_children) = &url_template_xml.child_objects {
                for method_xml in &template_children.methods {
                    let method_props = &method_xml.properties;
                    let method = HTTPServiceMethodBuilder::new()
                        .name(&method_props.name)
                        .http_method(&method_props.http_method)
                        .handler(&method_props.handler)
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
