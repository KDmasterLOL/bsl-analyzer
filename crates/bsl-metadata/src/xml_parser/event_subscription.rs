use crate::error::{MetadataError, Result};
use crate::event_subscription::EventSubscription;

use super::helpers::{child_text, find_child, find_mdo_element, parse_uuid, parse_xml};

pub fn parse_event_subscription_xml(xml: &str) -> Result<EventSubscription> {
    let _span = tracing::debug_span!("parse_event_subscription_xml").entered();

    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc).ok_or_else(|| {
        MetadataError::InvalidFormat("No EventSubscription element found".to_string())
    })?;

    let uuid_str = mdo.attribute("uuid").unwrap_or("");
    let uuid = parse_uuid(uuid_str, "event subscription")?;

    let props = find_child(mdo, "Properties").ok_or_else(|| {
        MetadataError::InvalidFormat("EventSubscription missing Properties".to_string())
    })?;

    let name = child_text(props, "Name").unwrap_or("").to_string();
    let comment = child_text(props, "Comment").map(|s| s.to_string());
    let event = child_text(props, "Event").unwrap_or("").to_string();
    let handler = child_text(props, "Handler").unwrap_or("").to_string();

    let source = find_child(props, "Source")
        .map(|source_node| {
            let mut all_types: Vec<&str> = Vec::new();
            for child in source_node.children().filter(|n| n.is_element()) {
                match child.tag_name().name() {
                    "Type" | "TypeSet" => {
                        if let Some(text) = child.text() {
                            all_types.push(text);
                        }
                    }
                    _ => {}
                }
            }
            all_types.join(";")
        })
        .unwrap_or_default();

    let subscription = EventSubscription { uuid, name, comment, source, event, handler };

    tracing::debug!(
        subscription_name = %subscription.name(),
        uuid = %subscription.uuid,
        handler = %subscription.handler_string(),
        "parsed event subscription"
    );

    Ok(subscription)
}
