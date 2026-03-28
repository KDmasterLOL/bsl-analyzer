use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub configuration_name: String,
    pub script_language: String,
    pub version: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            configuration_name: "Configuration".into(),
            script_language: "Russian".into(),
            version: "1.0.0".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub prefix_length: usize,
    pub suffix_length: usize,
    pub max_new_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct CompletionContext {
    pub prefix: String,
    pub suffix: String,
    pub path: String,
    pub offset: usize,
    pub script_language: String,
    pub cursor_object: String,
    pub current_method: String,
    pub cursor_environments: Vec<Environment>,
    pub type_hints: Vec<TypeHint>,
}

#[derive(Debug, Clone)]
pub struct TypeHint {
    pub variable_name: String,
    pub properties: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Environment {
    Server,
    Client,
    ExternalConn,
    MobileServer,
    MobileClient,
    ThickClient,
    ThinClient,
    WebClient,
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Server => write!(f, "SERVER"),
            Self::Client => write!(f, "CLIENT"),
            Self::ExternalConn => write!(f, "EXTERNALCONN"),
            Self::MobileServer => write!(f, "MOBILESERVER"),
            Self::MobileClient => write!(f, "MOBILECLIENT"),
            Self::ThickClient => write!(f, "THICKCLIENT"),
            Self::ThinClient => write!(f, "THINCLIENT"),
            Self::WebClient => write!(f, "WEBCLIENT"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompletionResult {
    pub text: String,
    pub finish_reason: FinishReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    Length,
    Cycling,
    Unknown(String),
}

impl From<&str> for FinishReason {
    fn from(s: &str) -> Self {
        match s {
            "stop" => Self::Stop,
            "length" => Self::Length,
            "cycling" => Self::Cycling,
            other => Self::Unknown(other.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ItsAnswer {
    pub text: String,
    pub had_tool_calls: bool,
}
