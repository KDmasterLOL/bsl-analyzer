use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tracing::{debug, warn};

use crate::types::{
    CompletionContext, CompletionResult, FinishReason, ItsAnswer, Session, SessionConfig,
};
use crate::{NaparnikApi, NaparnikError};

const BASE_URL: &str = "https://code.1c.ai";
const SESSION_TIMEOUT: Duration = Duration::from_secs(30);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);
const CHAT_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_RETRIES: u32 = 3;
const MAX_TOOL_ROUNDS: u32 = 10;

pub struct NaparnikHttpClient {
    client: Client,
    token: String,
}

impl NaparnikHttpClient {
    pub fn new(token: String) -> Result<Self, NaparnikError> {
        if token.is_empty() {
            return Err(NaparnikError::NoToken);
        }
        let client =
            Client::builder().timeout(CHAT_TIMEOUT).build().map_err(NaparnikError::Http)?;
        Ok(Self { client, token })
    }

    pub fn from_env() -> Result<Self, NaparnikError> {
        let token = std::env::var("NAPARNIK_TOKEN").map_err(|_| NaparnikError::NoToken)?;
        Self::new(token)
    }

    fn common_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&self.token).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("Unique-Id", HeaderValue::from_static("bsl-analyzer"));
        headers
    }

    fn chat_headers(&self) -> HeaderMap {
        let mut headers = self.common_headers();
        headers.insert("Origin", HeaderValue::from_static("https://code.1c.ai"));
        headers.insert("Referer", HeaderValue::from_static("https://code.1c.ai/chat//"));
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        headers.insert("Session-Id", HeaderValue::from_static(""));
        headers
    }

    async fn request_with_retry(
        &self,
        method: reqwest::Method,
        url: &str,
        body: &Value,
        timeout: Duration,
        headers: HeaderMap,
    ) -> Result<reqwest::Response, NaparnikError> {
        let mut last_err = None;
        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                let delay = Duration::from_secs(1 << attempt);
                debug!(attempt, ?delay, "retrying request");
                tokio::time::sleep(delay).await;
            }
            let result = self
                .client
                .request(method.clone(), url)
                .headers(headers.clone())
                .timeout(timeout)
                .json(body)
                .send()
                .await;

            match result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    if status == 429 || status == 423 || status.is_server_error() {
                        warn!(status = %status, attempt, "retryable error");
                        last_err = Some(NaparnikError::Api {
                            status: status.as_u16(),
                            body: resp.text().await.unwrap_or_default(),
                        });
                        continue;
                    }
                    let body_text = resp.text().await.unwrap_or_default();
                    return Err(NaparnikError::Api { status: status.as_u16(), body: body_text });
                }
                Err(e) => {
                    warn!(attempt, error = %e, "request failed");
                    last_err = Some(NaparnikError::Http(e));
                }
            }
        }
        Err(last_err.unwrap_or(NaparnikError::NoToken))
    }

    async fn read_sse_completion(
        &self,
        response: reqwest::Response,
    ) -> Result<CompletionResult, NaparnikError> {
        let mut text = String::new();
        let mut finish_reason = FinishReason::Stop;

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(NaparnikError::Http)?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim_end_matches('\r').to_string();
                buffer = buffer[line_end + 1..].to_string();

                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();
                    if data == "[DONE]" {
                        continue;
                    }
                    match serde_json::from_str::<Value>(data) {
                        Ok(v) => {
                            if let Some(t) = v["text"].as_str() {
                                text.push_str(t);
                            }
                            if let Some(r) = v["finish_reason"].as_str() {
                                finish_reason = FinishReason::from(r);
                            }
                        }
                        Err(e) => {
                            debug!(data, error = %e, "skipping unparseable SSE data");
                        }
                    }
                }
            }
        }

        Ok(CompletionResult { text, finish_reason })
    }

    async fn create_conversation(&self) -> Result<(String, Option<String>), NaparnikError> {
        let body = json!({
            "is_chat": true,
            "programming_language": "1C (BSL)",
            "script_language": "ru",
            "skill_name": "custom",
            "ui_language": "ru"
        });

        let resp = self
            .request_with_retry(
                reqwest::Method::POST,
                &format!("{BASE_URL}/chat_api/v1/conversations/"),
                &body,
                SESSION_TIMEOUT,
                self.chat_headers(),
            )
            .await?;

        let v: Value = resp.json().await.map_err(NaparnikError::Http)?;
        let conv_id = v["uuid"]
            .as_str()
            .ok_or_else(|| NaparnikError::SseParse("missing uuid in conversation".into()))?
            .to_string();
        let root_msg = v["root_message_uuid"].as_str().map(|s| s.to_string());

        debug!(conv_id, ?root_msg, "conversation created");
        Ok((conv_id, root_msg))
    }

    async fn send_chat_message(
        &self,
        conv_id: &str,
        parent_uuid: &Option<String>,
        role: &str,
        content: Value,
    ) -> Result<reqwest::Response, NaparnikError> {
        let body = json!({
            "role": role,
            "content": content,
            "parent_uuid": parent_uuid.as_deref(),
        });

        let url = format!("{BASE_URL}/chat_api/v1/conversations/{conv_id}/messages");
        self.request_with_retry(
            reqwest::Method::POST,
            &url,
            &body,
            CHAT_TIMEOUT,
            self.chat_headers(),
        )
        .await
    }

    async fn read_chat_sse(
        &self,
        response: reqwest::Response,
    ) -> Result<ChatSseResult, NaparnikError> {
        let mut text = String::new();
        let mut tool_calls: Vec<Value> = Vec::new();
        let mut last_uuid = String::new();

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(NaparnikError::Http)?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim_end_matches('\r').to_string();
                buffer = buffer[line_end + 1..].to_string();

                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();
                    if data == "[DONE]" || data.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(data) {
                        Ok(v) => {
                            if let Some(uuid) = v["uuid"].as_str() {
                                last_uuid = uuid.to_string();
                            }
                            if let Some(delta) = v.get("content_delta") {
                                if let Some(c) = delta["content"].as_str() {
                                    text.push_str(c);
                                }
                            }
                            if v.get("finished").and_then(|f| f.as_bool()) == Some(true) {
                                if let Some(content) = v.get("content") {
                                    if let Some(arr) = content["tool_calls"].as_array() {
                                        tool_calls.extend(arr.iter().cloned());
                                    }
                                    if let Some(c) = content["content"].as_str() {
                                        if text.is_empty() {
                                            text.push_str(c);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            debug!(data, error = %e, "skipping unparseable chat SSE");
                        }
                    }
                }
            }
        }

        Ok(ChatSseResult { text, tool_calls, last_uuid })
    }
}

struct ChatSseResult {
    text: String,
    tool_calls: Vec<Value>,
    last_uuid: String,
}

#[async_trait::async_trait]
impl NaparnikApi for NaparnikHttpClient {
    async fn create_session(&self, config: &SessionConfig) -> Result<Session, NaparnikError> {
        let body = json!({
            "service_parameters": {
                "min_delay": 300,
                "timeout": 5000,
                "prefix_length": 2000,
                "suffix_length": 1000,
                "language": config.script_language,
                "version": config.version
            },
            "user_parameters": {
                "code_completion_lines_count": 4,
                "tab_width": 4,
                "configuration_parameters": {
                    "configuration_name": config.configuration_name,
                    "type": "Configuration",
                    "script_language": config.script_language
                }
            }
        });

        let resp = self
            .request_with_retry(
                reqwest::Method::POST,
                &format!("{BASE_URL}/api/v1/create_session"),
                &body,
                SESSION_TIMEOUT,
                self.common_headers(),
            )
            .await?;

        let v: Value = resp.json().await.map_err(NaparnikError::Http)?;
        let session_id = v["session_id"]
            .as_str()
            .ok_or_else(|| NaparnikError::SseParse("missing session_id".into()))?
            .to_string();

        let sp = &v["service_parameters"];
        let prefix_length = sp["prefix_length"].as_u64().unwrap_or(2000) as usize;
        let suffix_length = sp["suffix_length"].as_u64().unwrap_or(1000) as usize;
        let max_new_tokens = sp["max_new_tokens"].as_u64().unwrap_or(180) as usize;

        debug!(session_id, prefix_length, suffix_length, max_new_tokens, "session created");
        Ok(Session { session_id, prefix_length, suffix_length, max_new_tokens })
    }

    async fn complete(
        &self,
        session: &Session,
        ctx: &CompletionContext,
    ) -> Result<CompletionResult, NaparnikError> {
        let environments: Vec<String> =
            ctx.cursor_environments.iter().map(|e| e.to_string()).collect();

        let body = json!({
            "local_context": {
                "prefix": ctx.prefix,
                "suffix": ctx.suffix,
                "path": ctx.path,
                "offset": ctx.offset,
                "script_language": ctx.script_language,
                "programing_language": "1с",
                "cursor_object": ctx.cursor_object,
                "current_method": ctx.current_method,
                "cursor_environments": environments,
                "related_objects": [],
                "related_functions": [],
                "proposals": []
            }
        });

        let mut headers = self.common_headers();
        headers.insert(
            "Session-Id",
            HeaderValue::from_str(&session.session_id)
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));

        let resp = self
            .request_with_retry(
                reqwest::Method::POST,
                &format!("{BASE_URL}/api/v1/complete"),
                &body,
                COMPLETION_TIMEOUT,
                headers,
            )
            .await?;

        self.read_sse_completion(resp).await
    }

    async fn ask_its(&self, question: &str) -> Result<ItsAnswer, NaparnikError> {
        let (conv_id, root_msg) = self.create_conversation().await?;
        let mut parent_uuid: Option<String> = root_msg;

        let content = json!({
            "content": {
                "instruction": question
            },
            "tools": []
        });

        let resp = self.send_chat_message(&conv_id, &parent_uuid, "user", content).await?;

        let mut result = self.read_chat_sse(resp).await?;
        let mut full_text = result.text;
        let mut had_tool_calls = false;

        for round in 0..MAX_TOOL_ROUNDS {
            if result.tool_calls.is_empty() {
                break;
            }
            had_tool_calls = true;
            debug!(round, count = result.tool_calls.len(), "processing tool calls");

            if !result.last_uuid.is_empty() {
                parent_uuid = Some(result.last_uuid.clone());
            }

            let tool_responses: Vec<Value> = result
                .tool_calls
                .iter()
                .filter_map(|tc| {
                    tc["id"].as_str().map(|id| {
                        json!({
                            "status": "accepted",
                            "tool_call_id": id,
                            "content": null
                        })
                    })
                })
                .collect();

            let resp = self
                .send_chat_message(&conv_id, &parent_uuid, "tool", json!(tool_responses))
                .await?;

            result = self.read_chat_sse(resp).await?;
            full_text.push_str(&result.text);

            if round == MAX_TOOL_ROUNDS - 1 && !result.tool_calls.is_empty() {
                return Err(NaparnikError::MaxRoundsExceeded);
            }
        }

        Ok(ItsAnswer { text: full_text, had_tool_calls })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CompletionContext, Environment, SessionConfig, TypeHint};

    fn client_from_env() -> Option<NaparnikHttpClient> {
        NaparnikHttpClient::from_env().ok()
    }

    #[tokio::test]
    #[ignore = "requires NAPARNIK_TOKEN"]
    async fn e2e_create_session() {
        let client = client_from_env().expect("NAPARNIK_TOKEN not set");
        let config = SessionConfig::default();
        let session = client.create_session(&config).await.unwrap();
        assert!(!session.session_id.is_empty());
        assert!(session.prefix_length > 0);
        assert!(session.max_new_tokens > 0);
        eprintln!("Session: {:?}", session);
    }

    #[tokio::test]
    #[ignore = "requires NAPARNIK_TOKEN"]
    async fn e2e_complete() {
        let client = client_from_env().expect("NAPARNIK_TOKEN not set");
        let config = SessionConfig::default();
        let session = client.create_session(&config).await.unwrap();

        let ctx = CompletionContext {
            prefix: "Функция ПолучитьДокументы()\n\tЗапрос = Новый Запрос;\n\tЗапрос.Текст = \"ВЫБРАТЬ Ссылка ИЗ Документ.РеализацияТоваровУслуг\";\n\tРезультат = Запрос.Выполнить();\n\t".into(),
            suffix: "\nКонецФункции".into(),
            path: "CommonModules/Тест/Ext/Module.bsl".into(),
            offset: 150,
            script_language: "Russian".into(),
            cursor_object: "Function".into(),
            current_method: String::new(),
            cursor_environments: vec![Environment::Server],
            type_hints: vec![],
        };

        let result = client.complete(&session, &ctx).await.unwrap();
        assert!(!result.text.is_empty());
        eprintln!(
            "Completion ({:?}): {}",
            result.finish_reason,
            &result.text[..result.text.len().min(200)]
        );
    }

    #[tokio::test]
    #[ignore = "requires NAPARNIK_TOKEN"]
    async fn e2e_complete_with_hints() {
        let client = client_from_env().expect("NAPARNIK_TOKEN not set");
        let config = SessionConfig::default();
        let session = client.create_session(&config).await.unwrap();

        let ctx = CompletionContext {
            prefix: "Процедура Тест()\n\tМассив = Новый Массив;\n\tМассив.".into(),
            suffix: "\nКонецПроцедуры".into(),
            path: "CommonModules/Тест/Ext/Module.bsl".into(),
            offset: 60,
            script_language: "Russian".into(),
            cursor_object: "Function".into(),
            current_method: String::new(),
            cursor_environments: vec![Environment::Server],
            type_hints: vec![TypeHint {
                variable_name: "Массив".into(),
                properties: vec![
                    "Добавить".into(),
                    "Количество".into(),
                    "Удалить".into(),
                    "Очистить".into(),
                ],
            }],
        };

        let result = client.complete(&session, &ctx).await.unwrap();
        assert!(!result.text.is_empty());
        eprintln!(
            "Completion with hints ({:?}): {}",
            result.finish_reason,
            &result.text[..result.text.len().min(200)]
        );
    }

    #[tokio::test]
    #[ignore = "requires NAPARNIK_TOKEN"]
    async fn e2e_ask_its() {
        let client = client_from_env().expect("NAPARNIK_TOKEN not set");
        let answer = client
            .ask_its("Как правильно использовать временные таблицы в пакетном запросе 1С?")
            .await
            .unwrap();
        assert!(!answer.text.is_empty());
        assert!(answer.had_tool_calls, "skill=custom should trigger tool calls");
        eprintln!(
            "ITS answer ({} chars, tools={}): {}...",
            answer.text.len(),
            answer.had_tool_calls,
            &answer.text[..answer.text.len().min(300)]
        );
    }
}
