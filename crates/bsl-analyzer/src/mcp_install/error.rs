use thiserror::Error;

use crate::mcp_install::model::{InstallScope, InstallTarget};

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("server name must not be empty")]
    EmptyName,

    #[error("target '{target}' does not support '{scope}' scope")]
    UnsupportedScope { target: InstallTarget, scope: InstallScope },

    #[error("target 'all' does not support '{scope}' scope")]
    UnsupportedAllScope { scope: InstallScope },

    #[error("server '{name}' already exists in {location}")]
    AlreadyExists { name: String, target: InstallTarget, scope: InstallScope, location: String },

    #[error("failed to run '{program}': binary not found in PATH")]
    TargetBinaryNotFound { program: String },

    #[error("failed to inspect existing MCP server '{name}' for target '{target}': {message}")]
    InspectionFailed { target: InstallTarget, name: String, message: String },

    #[error("{program} exited with code {status}: {message}")]
    ExternalCommandFailed { program: String, status: i32, message: String },

    #[error("cannot determine user home directory")]
    HomeDirectoryUnavailable,

    #[error("failed to read config file {path}: {message}")]
    ConfigRead { path: String, message: String },

    #[error("failed to write config file {path}: {message}")]
    ConfigWrite { path: String, message: String },

    #[error("failed to parse {format} config file {path}: {message}")]
    ConfigParse { path: String, format: &'static str, message: String },

    #[error("{what} in {path} must be a JSON object")]
    InvalidJsonShape { path: String, what: String },
}

impl InstallError {
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::AlreadyExists { .. } => {
                Some("rerun with --force to replace the existing MCP entry")
            }
            Self::TargetBinaryNotFound { .. } => {
                Some("install the target CLI first or add it to PATH")
            }
            Self::UnsupportedScope { .. } | Self::UnsupportedAllScope { .. } => {
                Some("choose a supported --scope for this --target")
            }
            Self::ConfigParse { .. } | Self::InvalidJsonShape { .. } => {
                Some("inspect the existing config file and fix invalid syntax before retrying")
            }
            _ => None,
        }
    }
}
