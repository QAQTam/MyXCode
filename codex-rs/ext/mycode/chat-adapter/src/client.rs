use codex_api::ApiError;
use codex_api::Compression;
use codex_api::EndpointSession;
use codex_api::Provider;
use codex_api::RequestTelemetry;
use codex_api::ResponseStream;
use codex_api::SharedAuthProvider;
use codex_api::SseTelemetry;
use codex_api::build_session_headers;
use codex_api::insert_header;
use codex_api::subagent_header;
use codex_client::EncodedJsonBody;
use codex_client::HttpTransport;
use codex_client::RequestCompression;
use codex_mycode_model_wire::ToolPlan;
use codex_protocol::protocol::SessionSource;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use serde_json::Value;
use std::sync::Arc;
use tracing::instrument;

use crate::spawn_chat_stream;

/// Provider-relative path for Chat Completions streaming calls.
pub const CHAT_COMPLETIONS_PATH: &str = "chat/completions";

/// Chat Completions transport bound to one provider and auth snapshot.
pub struct ChatCompletionsClient<T: HttpTransport> {
    session: EndpointSession<T>,
    sse_telemetry: Option<Arc<dyn SseTelemetry>>,
}

#[derive(Default)]
pub struct ChatOptions {
    pub session_id: Option<String>,
    pub session_source: Option<SessionSource>,
    pub extra_headers: HeaderMap,
    pub compression: Compression,
    pub tool_plan: Option<ToolPlan>,
}

impl<T: HttpTransport> ChatCompletionsClient<T> {
    pub fn new(transport: T, provider: Provider, auth: SharedAuthProvider) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
            sse_telemetry: None,
        }
    }

    pub fn with_telemetry(
        self,
        request: Option<Arc<dyn RequestTelemetry>>,
        sse: Option<Arc<dyn SseTelemetry>>,
    ) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
            sse_telemetry: sse,
        }
    }

    #[instrument(
        name = "chat.stream_request",
        level = "info",
        skip_all,
        fields(
            transport = "chat_http",
            http.method = "POST",
            api.path = CHAT_COMPLETIONS_PATH
        )
    )]
    pub async fn stream_request(
        &self,
        body: Value,
        options: ChatOptions,
    ) -> Result<ResponseStream, ApiError> {
        let ChatOptions {
            session_id,
            session_source,
            extra_headers,
            compression,
            tool_plan,
        } = options;
        let mut headers = extra_headers;
        headers.extend(build_session_headers(session_id, None));
        if let Some(subagent) = subagent_header(&session_source) {
            insert_header(&mut headers, "x-openai-subagent", &subagent);
        }
        self.stream(body, headers, compression, tool_plan).await
    }

    async fn stream(
        &self,
        body: Value,
        headers: HeaderMap,
        compression: Compression,
        tool_plan: Option<ToolPlan>,
    ) -> Result<ResponseStream, ApiError> {
        let body = EncodedJsonBody::encode(&body)
            .map_err(|error| ApiError::Stream(format!("failed to encode chat request: {error}")))?;
        let request_compression = match compression {
            Compression::None => RequestCompression::None,
            Compression::Zstd => RequestCompression::Zstd,
        };

        let stream_response = self
            .session
            .stream_encoded_json_with(
                Method::POST,
                CHAT_COMPLETIONS_PATH,
                headers,
                Some(body),
                |request| {
                    request.headers.insert(
                        http::header::ACCEPT,
                        HeaderValue::from_static("text/event-stream"),
                    );
                    request.compression = request_compression;
                },
            )
            .await?;

        Ok(spawn_chat_stream(
            stream_response,
            self.session.provider().stream_idle_timeout,
            self.sse_telemetry.clone(),
            tool_plan,
        ))
    }
}
