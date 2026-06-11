use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};

use anyhow::Result;
use lsp_server::{ErrorCode, Notification, Request, RequestId, Response};
use salsa::Database as _;
use serde::{de::DeserializeOwned, Serialize};

use crate::frozen_context::{FrozenFilePaths, LatencyRequestContext};
use crate::global_state::{GlobalState, Task};

pub struct RequestDispatcher<'a> {
    pub req: Option<Request>,
    pub global_state: &'a mut GlobalState,
}

impl RequestDispatcher<'_> {
    pub fn on_sync_mut<R>(
        &mut self,
        f: fn(&mut GlobalState, R::Params) -> Result<R::Result>,
    ) -> &mut Self
    where
        R: lsp_types::request::Request,
        R::Params: DeserializeOwned + fmt::Debug,
        R::Result: Serialize,
    {
        let (req, params) = match self.parse_request::<R>() {
            Some(it) => it,
            None => return self,
        };

        tracing::debug!("Handling {} (id: {})", R::METHOD, req.id);

        let result = f(self.global_state, params);
        let response = result_to_response::<R>(req.id, result);

        self.global_state.respond(response);
        self
    }

    pub fn on_sync<R>(
        &mut self,
        f: fn(crate::global_state::GlobalStateSnapshot, R::Params) -> Result<R::Result>,
    ) -> &mut Self
    where
        R: lsp_types::request::Request,
        R::Params: DeserializeOwned + fmt::Debug,
        R::Result: Serialize,
    {
        let (req, params) = match self.parse_request::<R>() {
            Some(it) => it,
            None => return self,
        };

        tracing::debug!("Handling {} (id: {})", R::METHOD, req.id);

        let snapshot = self.global_state.snapshot();
        let result = f(snapshot, params);
        let response = result_to_response::<R>(req.id, result);

        self.global_state.respond(response);
        self
    }

    pub fn on_latency<R>(
        &mut self,
        f: fn(LatencyRequestContext, R::Params) -> Result<R::Result>,
    ) -> &mut Self
    where
        R: lsp_types::request::Request,
        R::Params: DeserializeOwned + fmt::Debug + Send + 'static,
        R::Result: Serialize + Send + 'static,
    {
        let (req, params) = match self.parse_request::<R>() {
            Some(it) => it,
            None => return self,
        };

        tracing::debug!("Handling {} on task pool (id: {})", R::METHOD, req.id);

        // While the workspace is still loading, analysis would run over a
        // half-loaded source root (wrong results), and its db clone is exactly
        // what every streamed VFS batch's revision bump must wait on — the wait
        // that freezes the load. Decline instead: ContentModified is the
        // benign "state changed, retry if still needed" code clients swallow
        // silently, and the vfs_done finalize replays diagnostics and requests
        // a semantic-tokens refresh, so nothing is permanently lost.
        if !self.global_state.vfs_done {
            tracing::debug!(method = R::METHOD, id = ?req.id, "declining request: workspace loading");
            self.global_state.respond(Response::new_err(
                req.id,
                ErrorCode::ContentModified as i32,
                "workspace is still loading".to_string(),
            ));
            return self;
        }

        // A saturated task pool must not park the event loop in `try_spawn`'s
        // bounded queue (nor pin another db clone into a queued closure).
        // Checked before building the context: the event loop is the only job
        // producer, so capacity observed here cannot be taken by anyone else.
        if !self.global_state.task_pool.pool.has_capacity() {
            tracing::warn!(method = R::METHOD, id = ?req.id, "declining request: task pool saturated");
            self.global_state.respond(Response::new_err(
                req.id,
                ErrorCode::ContentModified as i32,
                "server is busy, please retry".to_string(),
            ));
            return self;
        }

        let db = self.global_state.analysis_host.raw_database().clone();
        let token = db.cancellation_token();
        let analysis = ide::Analysis::from_database(db);

        if let Some(prev) = self.global_state.request_tokens.insert(req.id.clone(), token) {
            tracing::warn!(request_id = ?req.id, "duplicate LSP request id; cancelling previous");
            prev.cancel();
        }

        let ctx = LatencyRequestContext {
            analysis,
            workspace_root: self.global_state.workspace_root.clone(),
            project: self.global_state.project.clone(),
            diagnostics_config: self.global_state.diagnostics_config().clone(),
            position_encoding: self.global_state.position_encoding,
            task_sender: self.global_state.task_pool.pool.sender.clone(),
            mem_docs: self.global_state.mem_docs.freeze(),
            file_paths: FrozenFilePaths::freeze(&self.global_state.vfs.read()),
        };

        let id = req.id;
        let job_id = id.clone();
        let spawned = self.global_state.task_pool.pool.try_spawn(move || {
            let response = run_latency_handler::<R>(job_id, ctx, params, f);
            Task::RequestResult { response }
        });
        if let Err(err) = spawned {
            // Unreachable after the capacity check above; kept so a queue
            // hiccup degrades to a declined request instead of a lost one.
            tracing::warn!(method = R::METHOD, request_id = ?id, ?err, "task pool rejected request job");
            self.global_state.request_tokens.remove(&id);
            self.global_state.respond(Response::new_err(
                id,
                ErrorCode::ContentModified as i32,
                "server is busy, please retry".to_string(),
            ));
        }
        self
    }

    pub fn finish(&mut self) {
        if let Some(req) = self.req.take() {
            tracing::error!("Unhandled request: {}", req.method);
            let response = Response::new_err(
                req.id,
                ErrorCode::MethodNotFound as i32,
                format!("Method not found: {}", req.method),
            );
            self.global_state.respond(response);
        }
    }

    fn parse_request<R>(&mut self) -> Option<(Request, R::Params)>
    where
        R: lsp_types::request::Request,
        R::Params: DeserializeOwned + fmt::Debug,
    {
        let req = self.req.as_ref()?;

        if req.method != R::METHOD {
            return None;
        }

        let req = self.req.take().unwrap();

        let params = match serde_json::from_value::<R::Params>(req.params.clone()) {
            Ok(params) => params,
            Err(err) => {
                tracing::error!("Failed to parse params for {}: {}", R::METHOD, err);
                let response = Response::new_err(
                    req.id.clone(),
                    ErrorCode::InvalidParams as i32,
                    format!("Invalid params: {}", err),
                );
                self.global_state.respond(response);
                return None;
            }
        };

        Some((req, params))
    }
}

pub struct NotificationDispatcher<'a> {
    pub not: Option<Notification>,
    pub global_state: &'a mut GlobalState,
}

impl NotificationDispatcher<'_> {
    pub fn on_sync_mut<N>(
        &mut self,
        f: fn(&mut GlobalState, N::Params) -> Result<()>,
    ) -> Result<&mut Self>
    where
        N: lsp_types::notification::Notification,
        N::Params: DeserializeOwned + fmt::Debug,
    {
        let not = match self.not.as_ref() {
            Some(it) => it,
            None => return Ok(self),
        };

        if not.method != N::METHOD {
            return Ok(self);
        }

        let not = self.not.take().unwrap();

        tracing::debug!("Handling notification: {}", N::METHOD);

        let params = match serde_json::from_value::<N::Params>(not.params.clone()) {
            Ok(params) => params,
            Err(err) => {
                tracing::error!("Failed to parse notification params for {}: {}", N::METHOD, err);
                return Ok(self);
            }
        };

        f(self.global_state, params)?;

        Ok(self)
    }

    pub fn finish(&mut self) {
        if let Some(not) = &self.not {
            if !not.method.starts_with("$/") {
                tracing::warn!("Unhandled notification: {}", not.method);
            }
        }
    }
}

fn result_to_response<R>(id: lsp_server::RequestId, result: Result<R::Result>) -> Response
where
    R: lsp_types::request::Request,
    R::Result: Serialize,
{
    match result {
        Ok(result) => {
            let result = serde_json::to_value(result).unwrap_or_else(|err| {
                tracing::error!("Failed to serialize result for {}: {}", R::METHOD, err);
                serde_json::Value::Null
            });
            Response::new_ok(id, result)
        }
        Err(err) => {
            tracing::error!("Request {} failed: {:?}", R::METHOD, err);
            Response::new_err(id, ErrorCode::InternalError as i32, format!("{:#}", err))
        }
    }
}

fn run_latency_handler<R>(
    id: RequestId,
    ctx: LatencyRequestContext,
    params: R::Params,
    f: fn(LatencyRequestContext, R::Params) -> Result<R::Result>,
) -> Response
where
    R: lsp_types::request::Request,
    R::Result: Serialize,
{
    let inner_id = id.clone();
    let inner = move || -> Response {
        match salsa::Cancelled::catch(AssertUnwindSafe(|| f(ctx, params))) {
            Ok(result) => result_to_response::<R>(inner_id, result),
            Err(_cancelled) => Response::new_err(
                inner_id,
                ErrorCode::RequestCanceled as i32,
                "request cancelled".to_string(),
            ),
        }
    };

    match catch_unwind(AssertUnwindSafe(inner)) {
        Ok(response) => response,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&'static str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic>".to_string());
            tracing::error!(
                request_id = ?id,
                method = R::METHOD,
                panic = %msg,
                backtrace = %std::backtrace::Backtrace::force_capture(),
                "async LSP handler panicked"
            );
            Response::new_err(
                id,
                ErrorCode::InternalError as i32,
                "internal handler panic".to_string(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use lsp_types::request::{HoverRequest, Request as _, Shutdown};
    use std::time::Duration;

    fn hover_params() -> lsp_types::HoverParams {
        lsp_types::HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: lsp_types::Url::parse("file:///test.bsl").unwrap(),
                },
                position: lsp_types::Position { line: 0, character: 0 },
            },
            work_done_progress_params: Default::default(),
        }
    }

    #[test]
    fn test_request_dispatcher_shutdown() {
        let (sender, _receiver) = unbounded();
        let mut state = GlobalState::new(sender);

        let req = Request::new(
            lsp_server::RequestId::from(1),
            Shutdown::METHOD.to_string(),
            serde_json::Value::Null,
        );

        RequestDispatcher { req: Some(req), global_state: &mut state }
            .on_sync_mut::<Shutdown>(|state, ()| {
                state.shutdown_requested = true;
                Ok(())
            })
            .finish();

        assert!(state.shutdown_requested);
    }

    #[test]
    fn on_latency_declines_with_content_modified_while_loading() {
        let (sender, receiver) = unbounded();
        let mut state = GlobalState::new(sender);
        assert!(!state.vfs_done, "default GlobalState models a loading workspace");

        let req = Request::new(
            lsp_server::RequestId::from(5),
            HoverRequest::METHOD.to_string(),
            serde_json::to_value(hover_params()).unwrap(),
        );

        RequestDispatcher { req: Some(req), global_state: &mut state }
            .on_latency::<HoverRequest>(|_ctx, _p| Ok(None))
            .finish();

        // Declined synchronously: no job spawned, no token registered.
        assert!(state.request_tokens.is_empty(), "no cancellation token for a declined request");
        assert!(
            state.task_pool.receiver.try_recv().is_err(),
            "no task pool job for a declined request"
        );
        let msg = receiver.try_recv().expect("immediate response");
        let lsp_server::Message::Response(response) = msg else {
            panic!("expected a response, got {msg:?}");
        };
        assert_eq!(response.id, lsp_server::RequestId::from(5));
        let err = response.error.expect("loading decline is an error response");
        assert_eq!(err.code, ErrorCode::ContentModified as i32);
    }

    #[test]
    fn on_latency_happy_path() {
        let (sender, _receiver) = unbounded();
        let mut state = GlobalState::new(sender);
        state.vfs_done = true;

        let req = Request::new(
            lsp_server::RequestId::from(42),
            HoverRequest::METHOD.to_string(),
            serde_json::to_value(hover_params()).unwrap(),
        );

        let receiver = state.task_pool.receiver.clone();
        RequestDispatcher { req: Some(req), global_state: &mut state }
            .on_latency::<HoverRequest>(|_ctx, _p| Ok(None))
            .finish();

        let task = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("task pool did not produce RequestResult");
        let Task::RequestResult { response } = task else {
            panic!("expected RequestResult, got {task:?}");
        };
        assert_eq!(response.id, lsp_server::RequestId::from(42));
        assert!(response.error.is_none(), "happy path must not set error");
        assert_eq!(response.result, Some(serde_json::Value::Null));
    }

    #[test]
    fn on_latency_panic_becomes_internal_error() {
        let (sender, _receiver) = unbounded();
        let mut state = GlobalState::new(sender);
        state.vfs_done = true;

        let req = Request::new(
            lsp_server::RequestId::from(7),
            HoverRequest::METHOD.to_string(),
            serde_json::to_value(hover_params()).unwrap(),
        );

        let receiver = state.task_pool.receiver.clone();
        RequestDispatcher { req: Some(req), global_state: &mut state }
            .on_latency::<HoverRequest>(|_ctx, _p| panic!("handler boom"))
            .finish();

        let task = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("task pool did not produce RequestResult");
        let Task::RequestResult { response } = task else {
            panic!("expected RequestResult, got {task:?}");
        };
        assert_eq!(response.id, lsp_server::RequestId::from(7));
        let err = response.error.expect("panic must produce error");
        assert_eq!(err.code, ErrorCode::InternalError as i32);
        assert!(state.request_tokens.contains_key(&lsp_server::RequestId::from(7)));
    }

    #[test]
    fn on_latency_duplicate_id_cancels_previous() {
        let (sender, _receiver) = unbounded();
        let mut state = GlobalState::new(sender);
        state.vfs_done = true;

        let id = lsp_server::RequestId::from(99);

        let req1 = Request::new(
            id.clone(),
            HoverRequest::METHOD.to_string(),
            serde_json::to_value(hover_params()).unwrap(),
        );
        RequestDispatcher { req: Some(req1), global_state: &mut state }
            .on_latency::<HoverRequest>(|_ctx, _p| Ok(None))
            .finish();

        let first_token =
            state.request_tokens.get(&id).expect("first dispatch must register token").clone();

        let req2 = Request::new(
            id.clone(),
            HoverRequest::METHOD.to_string(),
            serde_json::to_value(hover_params()).unwrap(),
        );
        RequestDispatcher { req: Some(req2), global_state: &mut state }
            .on_latency::<HoverRequest>(|_ctx, _p| Ok(None))
            .finish();

        assert!(
            first_token.is_cancelled(),
            "dispatching a duplicate id must cancel the previous token"
        );
        let current =
            state.request_tokens.get(&id).expect("second dispatch must register a new token");
        assert!(!current.is_cancelled(), "new token must still be live");
    }
}
