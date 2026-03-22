use std::fmt;

/// Unique identifier for a BSL module in the 1C debug protocol.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ModuleId {
    /// Extension name (empty string for main configuration)
    pub extension: String,
    /// Object UUID from metadata XML
    pub object_id: String,
    /// Property UUID — fixed constant per module type
    pub property_id: String,
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.extension.is_empty() {
            write!(f, "{}:{}", self.object_id, self.property_id)
        } else {
            write!(f, "{}:{}:{}", self.extension, self.object_id, self.property_id)
        }
    }
}

/// Debug step action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StepAction {
    /// Step to next line
    Next,
    /// Step into function call
    StepIn,
    /// Step out of current function
    StepOut,
    /// Continue execution
    Continue,
}

/// A single step in a variable path (for drilling into nested objects).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CalcPathItem {
    /// Root variable by name.
    Expression(String),
    /// Property of an object.
    Property(String),
    /// Index into a collection.
    Index(u32),
}

/// How to interpret variable children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ViewInterface {
    None,
    Context,
    Collection,
}

/// Type of debug target (client, server, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugTargetType {
    Unknown,
    Client,
    ManagedClient,
    WebClient,
    ComConnector,
    Server,
    ServerEmulation,
    WebService,
    HttpService,
    OData,
    Job,
    JobFileMode,
    MobileClient,
    MobileServer,
    MobileManagedClient,
}

impl DebugTargetType {
    pub fn xml_value(&self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Client => "Client",
            Self::ManagedClient => "ManagedClient",
            Self::WebClient => "WEBClient",
            Self::ComConnector => "COMConnector",
            Self::Server => "Server",
            Self::ServerEmulation => "ServerEmulation",
            Self::WebService => "WEBService",
            Self::HttpService => "HTTPService",
            Self::OData => "OData",
            Self::Job => "JOB",
            Self::JobFileMode => "JobFileMode",
            Self::MobileClient => "MobileClient",
            Self::MobileServer => "MobileServer",
            Self::MobileManagedClient => "MobileManagedClient",
        }
    }
}
