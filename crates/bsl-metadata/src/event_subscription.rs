use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSubscription {
    #[serde(rename = "uuid")]
    pub(crate) uuid: Uuid,

    #[serde(rename = "name")]
    pub(crate) name: String,

    #[serde(rename = "comment", default)]
    pub(crate) comment: Option<String>,

    #[serde(rename = "source")]
    pub(crate) source: String,

    #[serde(rename = "event")]
    pub(crate) event: String,

    #[serde(rename = "handler", default)]
    pub(crate) handler: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventSubscriptionHandler {
    pub module_name: String,
    pub method_name: String,
}

impl EventSubscriptionHandler {
    /// Heap bytes owned by this parsed handler: its module/method name strings.
    pub fn estimated_heap_size(&self) -> usize {
        self.module_name.capacity() + self.method_name.capacity()
    }
}

impl EventSubscription {
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

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn handler_string(&self) -> &str {
        &self.handler
    }

    pub fn event(&self) -> &str {
        &self.event
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn parse_handler(&self) -> Option<EventSubscriptionHandler> {
        if self.handler.is_empty() {
            return None;
        }

        let parts: Vec<&str> = self.handler.split('.').collect();

        if parts.len() < 2 || parts[0] != "CommonModule" {
            return None;
        }

        Some(EventSubscriptionHandler {
            module_name: parts[1].to_string(),
            method_name: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
        })
    }

    /// Heap bytes owned by this subscription, memoised by `ide-db`'s
    /// `parse_event_subscription_query` for Salsa's `heap_size` hook: its name
    /// plus the optional comment and its source/event/handler strings. New
    /// heap-owning fields must be added here too.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + self.comment.as_ref().map_or(0, String::capacity)
            + self.source.capacity()
            + self.event.capacity()
            + self.handler.capacity()
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
        assert_eq!(handler.method_name, "");
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
