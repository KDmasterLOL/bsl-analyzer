use async_trait::async_trait;

use crate::types::{CompletionContext, CompletionResult, ItsAnswer, Session, SessionConfig};
use crate::NaparnikError;

#[async_trait]
pub trait NaparnikApi: Send + Sync {
    async fn create_session(&self, config: &SessionConfig) -> Result<Session, NaparnikError>;
    async fn complete(
        &self,
        session: &Session,
        ctx: &CompletionContext,
    ) -> Result<CompletionResult, NaparnikError>;
    async fn ask_its(&self, question: &str) -> Result<ItsAnswer, NaparnikError>;
}
