use bytes::Bytes;
use codex_api::ResponseEvent;
use codex_client::StreamResponse;
use codex_protocol::models::ResponseItem;
use futures::stream;
use http::HeaderMap;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;

use super::spawn_chat_stream;

fn sse_response(chunks: Vec<String>) -> StreamResponse {
    let body = chunks
        .into_iter()
        .map(|chunk| Ok(Bytes::from(chunk)))
        .collect::<Vec<_>>();
    StreamResponse {
        status: StatusCode::OK,
        headers: HeaderMap::new(),
        bytes: Box::pin(stream::iter(body)),
    }
}

#[tokio::test]
async fn merges_tool_call_deltas_into_one_function_call() {
    let first_arguments = r#"{"cmd":"printf "#;
    let second_arguments = r#"hello"}"#;
    let body = vec![
        format!(
            "data: {}\n\n",
            json!({
                "id": "chat-1",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call-1",
                            "type": "function",
                            "function": {
                                "name": "exec_command",
                                "arguments": first_arguments,
                            }
                        }]
                    },
                    "finish_reason": null
                }]
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "id": "chat-1",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": {
                                "arguments": second_arguments,
                            }
                        }]
                    },
                    "finish_reason": null
                }]
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "id": "chat-1",
                "choices": [{
                    "delta": {},
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                }
            })
        ),
        "data: [DONE]\n\n".to_string(),
    ];

    let mut stream = spawn_chat_stream(
        sse_response(body),
        Duration::from_secs(5),
        /*telemetry*/ None,
    );
    let mut events = Vec::new();
    while let Some(event) = stream.rx_event.recv().await {
        let done = matches!(event, Ok(ResponseEvent::Completed { .. }));
        events.push(event.expect("Chat stream event"));
        if done {
            break;
        }
    }

    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                    name,
                    arguments,
                    call_id,
                    ..
                }) => Some((name.as_str(), arguments.as_str(), call_id.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![("exec_command", r#"{"cmd":"printf hello"}"#, "call-1",)]
    );
    assert!(events.iter().any(|event| matches!(
        event,
        ResponseEvent::Completed {
            token_usage: Some(usage),
            ..
        } if usage.total_tokens == 15
    )));
}
