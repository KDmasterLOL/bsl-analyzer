//! EventSubscription metadata object
//!
//! Represents 1C:Enterprise EventSubscription metadata.
//! EventSubscriptions define handlers for system events (OnWrite, BeforeWrite, etc.).
//!
//! ## Structure
//!
//! - Name: Unique subscription name
//! - Handler: Format `CommonModule.ModuleName.MethodName`
//! - Source: Event source (e.g., `cfg:CatalogObject.Catalog1`)
//! - Event: Event type (e.g., `OnWrite`, `BeforeWrite`)
//!
//! ## Note
//!
//! Unlike CommonModules, EventSubscriptions have NO code files - only XML metadata.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// EventSubscription metadata object
///
/// Java equivalent: `com.github._1c_syntax.bsl.mdo.EventSubscription`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSubscription {
    /// UUID
    #[serde(rename = "uuid")]
    pub(crate) uuid: Uuid,

    /// Subscription name
    #[serde(rename = "name")]
    pub(crate) name: String,

    /// Comment (optional)
    #[serde(rename = "comment", default)]
    pub(crate) comment: Option<String>,

    /// Event source (e.g., "cfg:DocumentObject.Document1")
    #[serde(rename = "source")]
    pub(crate) source: String,

    /// Event type (e.g., "OnWrite", "BeforeWrite")
    #[serde(rename = "event")]
    pub(crate) event: String,

    /// Handler path: "CommonModule.ModuleName.MethodName"
    /// Can be empty if not configured
    #[serde(rename = "handler", default)]
    pub(crate) handler: String,
}

/// Parsed handler (CommonModule.ModuleName.MethodName)
///
/// Represents a parsed event subscription handler path.
#[derive(Debug, Clone, PartialEq)]
pub struct EventSubscriptionHandler {
    /// Common module name
    pub module_name: String,

    /// Method name (can be empty if handler is malformed)
    pub method_name: String,
}

impl EventSubscription {
    /// Create new EventSubscription
    #[cfg(test)]
    pub fn new(name: impl Into<String>, handler: impl Into<String>) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            name: name.into(),
            comment: None,
            source: String::new(),
            event: String::new(),
            handler: handler.into(),
        }
    }

    /// Get subscription name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get handler string
    pub fn handler_string(&self) -> &str {
        &self.handler
    }

    /// Get event type
    pub fn event(&self) -> &str {
        &self.event
    }

    /// Get source
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Parse handler string into components
    ///
    /// Returns:
    /// - `None` if handler is empty
    /// - `Some(Handler)` with empty method_name if malformed (e.g., "CommonModule.Module")
    /// - `Some(Handler)` with full data if valid (e.g., "CommonModule.Module.Method")
    ///
    /// ## Examples
    ///
    /// ```
    /// # use bsl_metadata::{EventSubscription, EventSubscriptionHandler};
    /// // Valid handler
    /// let sub = EventSubscription::new("Test", "CommonModule.MyModule.MyMethod");
    /// let handler = sub.parse_handler().unwrap();
    /// assert_eq!(handler.module_name, "MyModule");
    /// assert_eq!(handler.method_name, "MyMethod");
    ///
    /// // Malformed (missing method name)
    /// let sub = EventSubscription::new("Test", "CommonModule.MyModule");
    /// let handler = sub.parse_handler().unwrap();
    /// assert_eq!(handler.module_name, "MyModule");
    /// assert_eq!(handler.method_name, "");
    ///
    /// // Empty
    /// let sub = EventSubscription::new("Test", "");
    /// assert!(sub.parse_handler().is_none());
    /// ```
    pub fn parse_handler(&self) -> Option<EventSubscriptionHandler> {
        if self.handler.is_empty() {
            return None;
        }

        let parts: Vec<&str> = self.handler.split('.').collect();

        // Must start with "CommonModule" and have at least module name
        if parts.len() < 2 || parts[0] != "CommonModule" {
            return None;
        }

        Some(EventSubscriptionHandler {
            module_name: parts[1].to_string(),
            method_name: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_handler_full() {
        let sub = EventSubscription::new(
            "TestSubscription",
            "CommonModule.ПервыйОбщийМодуль.ВерсионированиеПриЗаписи",
        );

        let handler = sub.parse_handler().unwrap();
        assert_eq!(handler.module_name, "ПервыйОбщийМодуль");
        assert_eq!(handler.method_name, "ВерсионированиеПриЗаписи");
    }

    #[test]
    fn test_parse_handler_malformed_missing_method() {
        let sub = EventSubscription::new("TestSubscription", "CommonModule.ОбщийПодпискиНаСобытия");

        let handler = sub.parse_handler().unwrap();
        assert_eq!(handler.module_name, "ОбщийПодпискиНаСобытия");
        assert_eq!(handler.method_name, ""); // Empty!
    }

    #[test]
    fn test_parse_handler_empty() {
        let sub = EventSubscription::new("TestSubscription", "");
        assert!(sub.parse_handler().is_none());
    }

    #[test]
    fn test_parse_handler_invalid_prefix() {
        let sub = EventSubscription::new("TestSubscription", "InvalidPrefix.Module.Method");
        assert!(sub.parse_handler().is_none());
    }

    #[test]
    fn test_parse_handler_only_common_module() {
        let sub = EventSubscription::new("TestSubscription", "CommonModule");
        assert!(sub.parse_handler().is_none());
    }

    #[test]
    fn test_event_subscription_accessors() {
        let sub = EventSubscription::new("TestSub", "CommonModule.M.F");
        assert_eq!(sub.name(), "TestSub");
        assert_eq!(sub.handler_string(), "CommonModule.M.F");
    }
}
