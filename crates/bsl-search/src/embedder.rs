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
    agent: ureq::Agent,
}

impl Clone for Embedder {
    fn clone(&self) -> Self {
        Self::new(self.config.clone())
    }
}

impl Embedder {
    pub fn new(config: EmbedderConfig) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .build()
            .new_agent();
        Self { config, agent }
    }

    pub fn dim(&self) -> usize {
        self.config.dim.unwrap_or(1024)
    }

    pub fn model(&self) -> &str {
        &self.config.model
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

        let mut req = self.agent.post(&url);
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

    pub fn embed(&self, text: &str) -> Result<Vec<f32>, SearchError> {
        let mut results = self.embed_batch(&[text])?;
        results.pop().ok_or_else(|| SearchError::Embedder("empty result".into()))
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
