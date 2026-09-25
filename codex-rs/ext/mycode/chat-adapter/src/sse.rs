use codex_api::ApiError;
use codex_api::ResponseEvent;
use codex_api::ResponseStream;
use codex_api::SseTelemetry;
use codex_client::StreamResponse;
use codex_mycode_model_wire::ToolPlan;
use codex_protocol::ToolName;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::Stream;
use futures::StreamExt;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;
use tracing::debug;

/// Converts a Chat Completions SSE response into Codex's response stream.
pub fn spawn_chat_stream(
    stream_response: StreamResponse,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    tool_plan: Option<ToolPlan>,
) -> ResponseStream {
    let upstream_request_id = stream_response
        .headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent, ApiError>>(1600);
    tokio::spawn(async move {
        process_chat_sse(
            stream_response.bytes,
            tx_event,
            idle_timeout,
            telemetry,
            tool_plan,
        )
        .await;
    });
    ResponseStream {
        rx_event,
        upstream_request_id,
    }
}

#[derive(Default, Debug)]
struct ToolCallState {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

async fn process_chat_sse<S>(
    stream: S,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    tool_plan: Option<ToolPlan>,
) where
    S: Stream<Item = Result<bytes::Bytes, codex_client::TransportError>> + Unpin,
{
    let mut stream = stream.eventsource();
    let mut tool_calls = BTreeMap::<usize, ToolCallState>::new();
    let mut tool_call_order = Vec::<usize>::new();
    let mut assistant_text = String::new();
    let mut assistant_item_added = false;
    let mut reasoning_text = String::new();
    let mut reasoning_item_added = false;
    let mut response_id = String::new();
    let mut usage = None;
    let mut end_turn = None;

    loop {
        let start = Instant::now();
        let response = timeout(idle_timeout, stream.next()).await;
        if let Some(telemetry) = telemetry.as_ref() {
            telemetry.on_sse_poll(&response, start.elapsed());
        }

        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(error))) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(error.to_string())))
                    .await;
                return;
            }
            Ok(None) => {
                flush_chat_state(
                    &tx_event,
                    &mut assistant_text,
                    &mut assistant_item_added,
                    &mut reasoning_text,
                    &mut reasoning_item_added,
                    &mut tool_calls,
                    &mut tool_call_order,
                    tool_plan.as_ref(),
                )
                .await;
                let _ = tx_event
                    .send(Ok(ResponseEvent::Completed {
                        response_id,
                        token_usage: usage,
                        usage_metadata: None,
                        end_turn,
                    }))
                    .await;
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "idle timeout waiting for Chat Completions SSE".to_string(),
                    )))
                    .await;
                return;
            }
        };

        let data = sse.data.trim();
        if data == "[DONE]" || data == "DONE" {
            flush_chat_state(
                &tx_event,
                &mut assistant_text,
                &mut assistant_item_added,
                &mut reasoning_text,
                &mut reasoning_item_added,
                &mut tool_calls,
                &mut tool_call_order,
                tool_plan.as_ref(),
            )
            .await;
            let _ = tx_event
                .send(Ok(ResponseEvent::Completed {
                    response_id,
                    token_usage: usage,
                    usage_metadata: None,
                    end_turn,
                }))
                .await;
            return;
        }

        let value: Value = match serde_json::from_str(data) {
            Ok(value) => value,
            Err(error) => {
                debug!("skipping invalid Chat Completions SSE payload: {error}");
                continue;
            }
        };
        if let Some(error) = value.get("error") {
            let _ = tx_event.send(Err(normalize_chat_error(error))).await;
            return;
        }
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            response_id = id.to_string();
        }
        if let Some(value_usage) = value.get("usage").and_then(parse_usage) {
            usage = Some(value_usage);
        }

        let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            continue;
        };

        if let Some(delta) = choice.get("delta") {
            if let Some(reasoning) = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .and_then(Value::as_str)
                && !reasoning.is_empty()
            {
                if !reasoning_item_added {
                    flush_assistant_item(&tx_event, &mut assistant_text, &mut assistant_item_added)
                        .await;
                    let _ = tx_event
                        .send(Ok(ResponseEvent::OutputItemAdded(
                            ResponseItem::Reasoning {
                                id: None,
                                summary: Vec::new(),
                                content: Some(Vec::new()),
                                encrypted_content: None,
                                internal_chat_message_metadata_passthrough: None,
                            },
                        )))
                        .await;
                    reasoning_item_added = true;
                }
                reasoning_text.push_str(reasoning);
                let _ = tx_event
                    .send(Ok(ResponseEvent::ReasoningContentDelta {
                        delta: reasoning.to_string(),
                        content_index: 0,
                    }))
                    .await;
            }
            if let Some(content) = delta.get("content").and_then(Value::as_str)
                && !content.is_empty()
            {
                if !assistant_item_added {
                    flush_reasoning_item(&tx_event, &mut reasoning_text, &mut reasoning_item_added)
                        .await;
                    let _ = tx_event
                        .send(Ok(ResponseEvent::OutputItemAdded(ResponseItem::Message {
                            id: None,
                            role: "assistant".to_string(),
                            content: Vec::new(),
                            phase: None,
                            internal_chat_message_metadata_passthrough: None,
                        })))
                        .await;
                    assistant_item_added = true;
                }
                assistant_text.push_str(content);
                let _ = tx_event
                    .send(Ok(ResponseEvent::OutputTextDelta(content.to_string())))
                    .await;
            }
            if let Some(tool_call_deltas) = delta.get("tool_calls").and_then(Value::as_array) {
                for tool_call in tool_call_deltas {
                    let index = tool_call
                        .get("index")
                        .and_then(Value::as_u64)
                        .and_then(|index| usize::try_from(index).ok())
                        .unwrap_or(tool_call_order.len());
                    if !tool_calls.contains_key(&index) {
                        tool_call_order.push(index);
                    }
                    let state = tool_calls.entry(index).or_default();
                    if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                        state.id.get_or_insert_with(|| id.to_string());
                    }
                    if let Some(function) = tool_call.get("function") {
                        if let Some(name) = function.get("name").and_then(Value::as_str) {
                            state.name.get_or_insert_with(|| name.to_string());
                        }
                        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                            state.arguments.push_str(arguments);
                            let _ = tx_event
                                .send(Ok(ResponseEvent::ToolCallInputDelta {
                                    item_id: format!("tool-call-{index}"),
                                    call_id: state.id.clone(),
                                    delta: arguments.to_string(),
                                }))
                                .await;
                        }
                    }
                }
            }
        }

        match choice.get("finish_reason").and_then(Value::as_str) {
            Some("tool_calls") => {
                flush_tool_calls(
                    &tx_event,
                    &mut tool_calls,
                    &mut tool_call_order,
                    tool_plan.as_ref(),
                )
                .await;
            }
            Some("stop") => {
                end_turn = Some(true);
                flush_chat_state(
                    &tx_event,
                    &mut assistant_text,
                    &mut assistant_item_added,
                    &mut reasoning_text,
                    &mut reasoning_item_added,
                    &mut tool_calls,
                    &mut tool_call_order,
                    tool_plan.as_ref(),
                )
                .await;
            }
            Some("length") => {
                let _ = tx_event.send(Err(ApiError::ContextWindowExceeded)).await;
                return;
            }
            Some("content_filter") => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "response blocked by content filter".to_string(),
                    )))
                    .await;
                return;
            }
            _ => {}
        }
    }
}

fn normalize_chat_error(error: &Value) -> ApiError {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Chat Completions provider returned an error")
        .to_string();
    match error.get("code").and_then(Value::as_str) {
        Some("context_length_exceeded" | "context_window_exceeded") => {
            ApiError::ContextWindowExceeded
        }
        Some("insufficient_quota" | "quota_exceeded") => ApiError::QuotaExceeded,
        Some("server_is_overloaded" | "overloaded") => {
            ApiError::ServerOverloaded { retry_after: None }
        }
        _ => match error.get("type").and_then(Value::as_str) {
            Some("invalid_request_error") => ApiError::InvalidRequest { message },
            _ => ApiError::Stream(message),
        },
    }
}

async fn flush_chat_state(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    assistant_text: &mut String,
    assistant_item_added: &mut bool,
    reasoning_text: &mut String,
    reasoning_item_added: &mut bool,
    tool_calls: &mut BTreeMap<usize, ToolCallState>,
    tool_call_order: &mut Vec<usize>,
    tool_plan: Option<&ToolPlan>,
) {
    flush_reasoning_item(tx_event, reasoning_text, reasoning_item_added).await;
    flush_assistant_item(tx_event, assistant_text, assistant_item_added).await;
    flush_tool_calls(tx_event, tool_calls, tool_call_order, tool_plan).await;
}

async fn flush_reasoning_item(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    reasoning_text: &mut String,
    reasoning_item_added: &mut bool,
) {
    if reasoning_text.is_empty() {
        return;
    }
    let _ = tx_event
        .send(Ok(ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
            id: None,
            summary: Vec::new(),
            content: Some(vec![ReasoningItemContent::ReasoningText {
                text: std::mem::take(reasoning_text),
            }]),
            encrypted_content: None,
            internal_chat_message_metadata_passthrough: None,
        })))
        .await;
    *reasoning_item_added = false;
}

async fn flush_assistant_item(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    assistant_text: &mut String,
    assistant_item_added: &mut bool,
) {
    if assistant_text.is_empty() {
        return;
    }
    let _ = tx_event
        .send(Ok(ResponseEvent::OutputItemDone(ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: std::mem::take(assistant_text),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        })))
        .await;
    *assistant_item_added = false;
}

async fn flush_tool_calls(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    tool_calls: &mut BTreeMap<usize, ToolCallState>,
    tool_call_order: &mut Vec<usize>,
    tool_plan: Option<&ToolPlan>,
) {
    for index in tool_call_order.drain(..) {
        let Some(state) = tool_calls.remove(&index) else {
            continue;
        };
        let Some(wire_name) = state.name else {
            debug!("skipping Chat Completions tool call at index {index} without a name");
            continue;
        };
        let name = tool_plan
            .and_then(|plan| plan.decode_flat_name(&wire_name))
            .unwrap_or_else(|| ToolName::plain(wire_name));
        let _ = tx_event
            .send(Ok(ResponseEvent::OutputItemDone(
                ResponseItem::FunctionCall {
                    id: None,
                    name: name.name,
                    namespace: name.namespace,
                    arguments: state.arguments,
                    encrypted_function_args: None,
                    call_id: state.id.unwrap_or_else(|| format!("tool-call-{index}")),
                    internal_chat_message_metadata_passthrough: None,
                },
            )))
            .await;
    }
}

fn parse_usage(value: &Value) -> Option<TokenUsage> {
    let input_tokens = value.get("prompt_tokens")?.as_i64()?;
    let output_tokens = value.get("completion_tokens")?.as_i64()?;
    let total_tokens = value
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(input_tokens + output_tokens);
    Some(TokenUsage {
        input_tokens,
        cached_input_tokens: value
            .get("prompt_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cache_write_input_tokens: 0,
        output_tokens,
        reasoning_output_tokens: value
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        total_tokens,
        codex_rollout_budget_units: None,
    })
}

#[cfg(test)]
#[path = "sse_tests.rs"]
mod tests;
