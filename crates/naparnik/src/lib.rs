mod client;
mod completion;
mod http_client;
mod session;
pub mod types;

pub use client::NaparnikApi;
pub use completion::InlineCompletionUseCase;
pub use http_client::NaparnikHttpClient;
pub use session::SessionManager;

#[derive(Debug, thiserror::Error)]
pub enum NaparnikError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("SSE parse error: {0}")]
    SseParse(String),
    #[error("No token: set NAPARNIK_TOKEN environment variable")]
    NoToken,
    #[error("Session not found for configuration: {0}")]
    SessionNotFound(String),
    #[error("Max tool call rounds exceeded")]
    MaxRoundsExceeded,
}
