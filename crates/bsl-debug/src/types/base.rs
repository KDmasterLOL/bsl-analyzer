use std::fmt;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ModuleId {
    pub extension: String,
    pub object_id: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StepAction {
    Next,
    StepIn,
    StepOut,
    Continue,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CalcPathItem {
    Expression(String),
    Property(String),
    Index(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ViewInterface {
    None,
    Context,
    Collection,
}

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
