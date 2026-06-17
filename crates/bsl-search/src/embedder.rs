use crate::error::SearchError;
use crate::ports::EmbeddingGenerator;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct EmbedderConfig {
    pub base_url: String,
    pub model: String,
    pub dim: Option<usize>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
}

impl Default for EmbedderConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".to_owned(),
            model: "qwen3-embedding".to_owned(),
            dim: Some(1024),
            api_key: None,
            provider: None,
        }
    }
}

pub struct Embedder {
    config: EmbedderConfig,
    /// Resilient agent for the unattended batch indexing pass: a long global timeout, paired
    /// with [`Self::MAX_RETRIES`] in [`Self::embed_batch`].
    agent: ureq::Agent,
    /// Tight agent for interactive single-query embeds ([`Self::embed`]). A `search_code` caller
    /// is waiting and the engine mutex is held across the call, so the query embed must fail
    /// fast instead of inheriting the batch path's minutes-long timeout-and-retry budget.
    interactive_agent: ureq::Agent,
}

impl Clone for Embedder {
    fn clone(&self) -> Self {
        Self::new(self.config.clone())
    }
}

impl Embedder {
    /// Global timeout for an interactive query embed. Bounds how long [`Self::embed`] can hold
    /// the engine mutex, so one slow embed cannot stall every concurrent `search_code`.
    const INTERACTIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

    pub fn new(config: EmbedderConfig) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .build()
            .new_agent();
        let interactive_agent = ureq::Agent::config_builder()
            .timeout_global(Some(Self::INTERACTIVE_TIMEOUT))
            .build()
            .new_agent();
        Self { config, agent, interactive_agent }
    }

    pub fn dim(&self) -> usize {
        self.config.dim.unwrap_or(1024)
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// A clone of this embedder's configuration, so a caller can rebuild a standalone embedder
    /// (e.g. the off-lock overlay warmup) without reaching into private fields.
    pub fn config(&self) -> EmbedderConfig {
        self.config.clone()
    }

    const MAX_RETRIES: u32 = 10;

    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SearchError> {
        let mut last_err = None;
        for attempt in 0..Self::MAX_RETRIES {
            match self.embed_batch_once(texts) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt.min(6)));
                    tracing::warn!(
                        attempt = attempt + 1,
                        max = Self::MAX_RETRIES,
                        delay_ms = delay.as_millis() as u64,
                        "embedding batch failed, retrying: {e}"
                    );
                    std::thread::sleep(delay);
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| SearchError::Embedder("all retries exhausted".into())))
    }

    fn embed_batch_once(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SearchError> {
        self.embed_batch_once_with(&self.agent, texts)
    }

    fn embed_batch_once_with(
        &self,
        agent: &ureq::Agent,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, SearchError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/v1/embeddings", self.config.base_url);

        let provider_only = self.config.provider.as_deref().map(|s| vec![s]);
        let provider_routing =
            provider_only.as_deref().map(|only| ProviderRouting { only, allow_fallbacks: false });
        let request = EmbeddingRequest {
            model: &self.config.model,
            input: texts,
            dimensions: self.config.dim,
            provider: provider_routing,
        };

        let mut req = agent.post(&url);
        if let Some(ref key) = self.config.api_key {
            req = req.header("Authorization", &format!("Bearer {key}"));
        }
        let mut resp = match req.send_json(&request) {
            Ok(r) => r,
            Err(e) => {
                let detail = match e {
                    ureq::Error::StatusCode(code) => {
                        format!(
                            "HTTP {code} (batch_size={}, total_chars={})",
                            texts.len(),
                            texts.iter().map(|t| t.len()).sum::<usize>(),
                        )
                    }
                    other => format!("{other}"),
                };
                return Err(SearchError::Embedder(detail));
            }
        };

        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| SearchError::Embedder(format!("failed to read response body: {e}")))?;
        let response: EmbeddingResponse = serde_json::from_str(&body).map_err(|e| {
            let preview = if body.len() > 200 { &body[..200] } else { &body };
            SearchError::Embedder(format!("failed to parse response: {e}\n  body: {preview}"))
        })?;

        let mut data = response.data;
        data.sort_by_key(|d| d.index);

        let embeddings: Vec<Vec<f32>> = data.into_iter().map(|d| d.embedding).collect();

        if embeddings.len() != texts.len() {
            return Err(SearchError::Embedder(format!(
                "expected {} embeddings, got {}",
                texts.len(),
                embeddings.len()
            )));
        }

        Ok(embeddings)
    }

    /// Embed a single interactive query, fail-fast. Unlike [`Self::embed_batch`] (the resilient
    /// indexing path), this makes ONE attempt on the tight-timeout [`Self::interactive_agent`]:
    /// the caller is an interactive `search_code` holding the engine mutex, so a stuck embedding
    /// service must surface an error in seconds rather than retry for minutes and block every
    /// concurrent search. A transient failure is the caller's to retry as a whole search.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, SearchError> {
        let mut results = self.embed_batch_once_with(&self.interactive_agent, &[text])?;
        results.pop().ok_or_else(|| SearchError::Embedder("empty result".into()))
    }

    /// Embed a batch fail-fast on the interactive agent. For the workspace-overlay refresh, which
    /// runs while the engine mutex is held (an interactive semantic search or the warmup prime):
    /// it must NOT inherit the indexing path's minutes-long retry budget and stall every
    /// concurrent search. A transient failure just leaves those chunks un-embedded until the next
    /// refresh re-attempts them; lexical search stays available meanwhile.
    pub fn embed_batch_interactive(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SearchError> {
        self.embed_batch_once_with(&self.interactive_agent, texts)
    }

    pub fn health_check(&self) -> Result<(), SearchError> {
        let health_url = format!("{}/health", self.config.base_url);
        let models_url = format!("{}/v1/models", self.config.base_url);
        if self.agent.get(&health_url).call().is_err()
            && self.agent.get(&models_url).call().is_err()
        {
            return Err(SearchError::Embedder(format!(
                "embedding service not available at {}",
                self.config.base_url
            )));
        }
        Ok(())
    }
}

impl EmbeddingGenerator for Embedder {
    fn model_id(&self) -> &str {
        self.model()
    }

    fn dimension(&self) -> usize {
        self.dim()
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SearchError> {
        Self::embed_batch(self, texts)
    }
}

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [&'a str],
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderRouting<'a>>,
}

#[derive(Serialize)]
struct ProviderRouting<'a> {
    only: &'a [&'a str],
    allow_fallbacks: bool,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}
