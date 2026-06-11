use serde::{Deserialize, Serialize};

/// An integration service (`СервисИнтеграции`). Its message channels bind a
/// receive-message handler procedure that the platform invokes — those handlers
/// have no BSL call site, so consumers (e.g. `UnusedLocalMethod`) must treat the
/// bound procedure names as used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationService {
    name: String,
    channels: Vec<IntegrationServiceChannel>,
}

impl IntegrationService {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), channels: Vec::new() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn channels(&self) -> &[IntegrationServiceChannel] {
        &self.channels
    }

    /// Receive-message handler procedure names bound across all channels, skipping
    /// channels without a handler (e.g. send-only channels).
    pub fn receive_handlers(&self) -> impl Iterator<Item = &str> {
        self.channels.iter().filter_map(|c| {
            let h = c.receive_message_processing();
            (!h.is_empty()).then_some(h)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationServiceChannel {
    name: String,
    receive_message_processing: String,
}

impl IntegrationServiceChannel {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), receive_message_processing: String::new() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn receive_message_processing(&self) -> &str {
        &self.receive_message_processing
    }
}

#[derive(Debug, Default)]
pub struct IntegrationServiceBuilder {
    name: String,
    channels: Vec<IntegrationServiceChannel>,
}

impl IntegrationServiceBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn add_channel(mut self, channel: IntegrationServiceChannel) -> Self {
        self.channels.push(channel);
        self
    }

    pub fn build(self) -> IntegrationService {
        IntegrationService { name: self.name, channels: self.channels }
    }
}

#[derive(Debug, Default)]
pub struct IntegrationServiceChannelBuilder {
    name: String,
    receive_message_processing: String,
}

impl IntegrationServiceChannelBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn receive_message_processing(mut self, handler: impl Into<String>) -> Self {
        self.receive_message_processing = handler.into();
        self
    }

    pub fn build(self) -> IntegrationServiceChannel {
        IntegrationServiceChannel {
            name: self.name,
            receive_message_processing: self.receive_message_processing,
        }
    }
}
