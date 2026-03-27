use std::collections::HashMap;
use tokio::sync::Mutex;

use crate::types::{Session, SessionConfig};
use crate::{NaparnikApi, NaparnikError};

pub struct SessionManager<A: NaparnikApi> {
    api: A,
    sessions: Mutex<HashMap<String, Session>>,
}

impl<A: NaparnikApi> SessionManager<A> {
    pub fn new(api: A) -> Self {
        Self { api, sessions: Mutex::new(HashMap::new()) }
    }

    pub async fn get_or_create(&self, config: &SessionConfig) -> Result<Session, NaparnikError> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(&config.configuration_name) {
            return Ok(session.clone());
        }
        let session = self.api.create_session(config).await?;
        sessions.insert(config.configuration_name.clone(), session.clone());
        Ok(session)
    }

    pub fn api(&self) -> &A {
        &self.api
    }
}
