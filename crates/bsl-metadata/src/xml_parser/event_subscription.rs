//! EventSubscription XML parser

use crate::error::Result;
use crate::event_subscription::EventSubscription;

use super::helpers::parse_uuid;
use super::serde_types::EventSubscriptionRoot;

/// Parse EventSubscription XML from Designer format
///
/// # Arguments
///
/// * `xml` - XML content as string
///
/// # Returns
///
/// Parsed `EventSubscription` structure
///
/// # Example
///
/// ```no_run
/// # use bsl_metadata::xml_parser::parse_event_subscription_xml;
/// let xml = std::fs::read_to_string("EventSubscriptions/MySubscription.xml")?;
/// let subscription = parse_event_subscription_xml(&xml)?;
/// # Ok::<(), bsl_metadata::MetadataError>(())
/// ```
pub fn parse_event_subscription_xml(xml: &str) -> Result<EventSubscription> {
    let _span = tracing::debug_span!("parse_event_subscription_xml").entered();

    let root: EventSubscriptionRoot = quick_xml::de::from_str(xml)?;
    let uuid = parse_uuid(&root.event_subscription.uuid, "event subscription")?;

    let subscription = EventSubscription {
        uuid,
        name: root.event_subscription.properties.name,
        comment: root.event_subscription.properties.comment,
        source: root.event_subscription.properties.source.as_string(),
        event: root.event_subscription.properties.event,
        handler: root.event_subscription.properties.handler,
    };

    tracing::debug!(
        subscription_name = %subscription.name(),
        uuid = %subscription.uuid,
        handler = %subscription.handler_string(),
        "parsed event subscription"
    );

    Ok(subscription)
}
