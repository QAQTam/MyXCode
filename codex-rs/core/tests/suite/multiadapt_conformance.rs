use anyhow::Context;
use anyhow::Result;
use codex_core::config::Config;
use codex_model_provider_info::RESPONSE_ADAPTER_HEADER;
use codex_protocol::AgentPath;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::Op;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

const TOOL_CALL_ID: &str = "call-1";
const TOOL_NAME: &str = "exec_command";
const TOOL_OUTPUT: &str = "conformance";

#[derive(Clone, Copy, Debug)]
enum AdapterProfile {
    ResponsesNative,
    ResponsesFunctionOnly,
    ChatCompletionsFunctionOnly,
}

impl AdapterProfile {
    fn configure(self, config: &mut Config) {
        match self {
            Self::ResponsesNative => {}
            Self::ResponsesFunctionOnly => {
                config
                    .model_provider
                    .http_headers
                    .get_or_insert_default()
                    .insert(
                        RESPONSE_ADAPTER_HEADER.to_string(),
                        "responses_function_only".into(),
                    );
            }
            Self::ChatCompletionsFunctionOnly => {
                config
                    .model_provider
                    .http_headers
                    .get_or_insert_default()
                    .insert(
                        RESPONSE_ADAPTER_HEADER.to_string(),
                        "chat_completions".into(),
                    );
            }
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::ResponsesNative => "responses-native",
            Self::ResponsesFunctionOnly => "responses-function-only",
            Self::ChatCompletionsFunctionOnly => "chat-completions-function-only",
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_native_round_trips_canonical_tool_identity() -> Result<()> {
    run_tool_round_trip(AdapterProfile::ResponsesNative).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_function_only_round_trips_canonical_tool_identity() -> Result<()> {
    run_tool_round_trip(AdapterProfile::ResponsesFunctionOnly).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_completions_function_only_round_trips_canonical_tool_identity() -> Result<()> {
    run_tool_round_trip(AdapterProfile::ChatCompletionsFunctionOnly).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_function_only_round_trips_multiple_tool_calls() -> Result<()> {
    run_multi_tool_round_trip(AdapterProfile::ResponsesFunctionOnly).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_completions_function_only_round_trips_multiple_tool_calls() -> Result<()> {
    run_multi_tool_round_trip(AdapterProfile::ChatCompletionsFunctionOnly).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switching_from_chat_to_responses_resumes_canonical_history() -> Result<()> {
    let server = responses::start_mock_server().await;
    mount_chat_tool_call(&server).await;
    mount_chat_completion(&server).await;
    mount_chat_completion(&server).await;

    let initial = test_codex()
        .with_config(|config| AdapterProfile::ChatCompletionsFunctionOnly.configure(config))
        .build_with_auto_env(&server)
        .await?;
    initial.submit_turn("run the chat conformance tool").await?;
    initial.submit_turn("second chat turn").await?;

    let responses_mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("resp-after-switch"),
            responses::ev_assistant_message("msg-after-switch", "done"),
            responses::ev_completed("resp-after-switch"),
        ]),
    )
    .await;

    let resumed = test_codex()
        .with_config(|config| AdapterProfile::ResponsesFunctionOnly.configure(config))
        .restart(&server, &initial)
        .await?;
    resumed.submit_turn("continue on responses").await?;

    let request = responses_mock.single_request();
    assert_eq!(request.path(), "/v1/responses");
    assert!(request.has_function_call(TOOL_CALL_ID));
    let tool_output = request
        .function_call_output_text(TOOL_CALL_ID)
        .context("Responses history should contain the Chat tool result")?;
    assert!(
        tool_output.contains(TOOL_OUTPUT),
        "tool output did not contain {TOOL_OUTPUT:?}: {tool_output:?}"
    );
    assert!(
        request
            .input()
            .iter()
            .all(|item| item.get("tool_calls").is_none()),
        "Responses input should not contain Chat-shaped tool_calls"
    );
    assert!(request.body_contains_text("second chat turn"));
    assert!(request.body_contains_text("continue on responses"));

    let requests = server
        .received_requests()
        .await
        .context("mock server should retain received requests")?;
    let chat_request_count = requests
        .iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .count();
    let responses_request_count = requests
        .iter()
        .filter(|request| request.url.path() == "/v1/responses")
        .count();
    assert_eq!(chat_request_count, 3);
    assert_eq!(responses_request_count, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switching_from_responses_to_chat_resumes_canonical_history() -> Result<()> {
    let server = responses::start_mock_server().await;
    let arguments = tool_arguments(TOOL_OUTPUT);
    responses::mount_function_call_agent_response(&server, TOOL_CALL_ID, &arguments, TOOL_NAME)
        .await;
    responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("resp-second"),
            responses::ev_assistant_message("msg-second", "done"),
            responses::ev_completed("resp-second"),
        ]),
    )
    .await;

    let initial = test_codex()
        .with_config(|config| AdapterProfile::ResponsesFunctionOnly.configure(config))
        .build_with_auto_env(&server)
        .await?;
    initial
        .submit_turn("run the responses conformance tool")
        .await?;
    initial.submit_turn("second responses turn").await?;

    mount_chat_completion(&server).await;
    let resumed = test_codex()
        .with_config(|config| AdapterProfile::ChatCompletionsFunctionOnly.configure(config))
        .restart(&server, &initial)
        .await?;
    resumed.submit_turn("continue on chat").await?;

    let requests = server
        .received_requests()
        .await
        .context("mock server should retain received requests")?;
    let chat_requests = requests
        .iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .collect::<Vec<_>>();
    assert_eq!(chat_requests.len(), 1);
    let body: Value = serde_json::from_slice(&chat_requests[0].body)?;
    assert_chat_tool_output(&body, TOOL_CALL_ID, TOOL_OUTPUT)?;
    let messages = body["messages"]
        .as_array()
        .context("Chat request messages should be an array")?;
    assert!(messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("user")
            && message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.contains("second responses turn"))
    }));
    assert!(messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("user")
            && message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.contains("continue on chat"))
    }));
    assert!(body.get("input").is_none());

    let responses_request_count = requests
        .iter()
        .filter(|request| request.url.path() == "/v1/responses")
        .count();
    assert_eq!(responses_request_count, 3);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagent_delivery_on_function_only_wires() -> Result<()> {
    let server = responses::start_mock_server().await;
    responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("resp-first"),
            responses::ev_assistant_message("msg-first", "ready"),
            responses::ev_completed("resp-first"),
        ]),
    )
    .await;
    let second_response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("resp-second"),
            responses::ev_assistant_message("msg-second", "done"),
            responses::ev_completed("resp-second"),
        ]),
    )
    .await;
    let test = test_codex()
        .with_config(|config| AdapterProfile::ResponsesFunctionOnly.configure(config))
        .build_with_auto_env(&server)
        .await?;

    test.submit_turn("wait for a subagent").await?;
    test.codex
        .submit(Op::InterAgentCommunication {
            communication: InterAgentCommunication::new(
                AgentPath::try_from("/root/worker").expect("worker path should parse"),
                AgentPath::root(),
                Vec::new(),
                "subagent delivery payload".to_string(),
                /*trigger_turn*/ true,
            ),
            start_options: Default::default(),
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let body = second_response.single_request().body_json();
    let input = body["input"]
        .as_array()
        .context("Responses input should be an array")?;
    assert!(input.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("message")
            && item.get("role").and_then(Value::as_str) == Some("user")
            && item
                .get("content")
                .is_some_and(|content| content.to_string().contains("subagent delivery payload"))
    }));
    assert!(
        input
            .iter()
            .all(|item| item.get("type").and_then(Value::as_str) != Some("agent_message"))
    );
    Ok(())
}

async fn run_tool_round_trip(profile: AdapterProfile) -> Result<()> {
    let server = responses::start_mock_server().await;
    let result = match profile {
        AdapterProfile::ResponsesNative | AdapterProfile::ResponsesFunctionOnly => {
            run_responses_tool_round_trip(&server, profile).await
        }
        AdapterProfile::ChatCompletionsFunctionOnly => {
            run_chat_tool_round_trip(&server, profile).await
        }
    };
    result.with_context(|| format!("adapter profile {}", profile.name()))
}

async fn run_multi_tool_round_trip(profile: AdapterProfile) -> Result<()> {
    let server = responses::start_mock_server().await;
    let result = match profile {
        AdapterProfile::ResponsesFunctionOnly => {
            run_responses_multi_tool_round_trip(&server, profile).await
        }
        AdapterProfile::ChatCompletionsFunctionOnly => {
            run_chat_multi_tool_round_trip(&server, profile).await
        }
        AdapterProfile::ResponsesNative => unreachable!("native Responses is covered separately"),
    };
    result.with_context(|| format!("multi-tool adapter profile {}", profile.name()))
}

async fn run_responses_multi_tool_round_trip(
    server: &MockServer,
    profile: AdapterProfile,
) -> Result<()> {
    let first_arguments = tool_arguments("conformance-one");
    let second_arguments = tool_arguments("conformance-two");
    let function_call = responses::mount_sse_once(
        server,
        responses::sse(vec![
            responses::ev_response_created("resp-multi-1"),
            responses::ev_function_call("call-1", TOOL_NAME, &first_arguments),
            responses::ev_function_call("call-2", TOOL_NAME, &second_arguments),
            responses::ev_completed("resp-multi-1"),
        ]),
    )
    .await;
    let completion = responses::mount_sse_once(
        server,
        responses::sse(vec![
            responses::ev_assistant_message("msg-multi-1", "done"),
            responses::ev_completed("resp-multi-2"),
        ]),
    )
    .await;
    let test = test_codex()
        .with_config(move |config| profile.configure(config))
        .build_with_auto_env(server)
        .await?;

    test.submit_turn("run two conformance tools").await?;

    let first_body = function_call.single_request().body_json();
    assert_responses_function_tool(&first_body, profile);
    assert_no_native_responses_tools(&first_body);

    let second_request = completion.single_request();
    assert_tool_output(&second_request, "call-1", "conformance-one")?;
    assert_tool_output(&second_request, "call-2", "conformance-two")?;
    Ok(())
}

async fn run_chat_multi_tool_round_trip(
    server: &MockServer,
    profile: AdapterProfile,
) -> Result<()> {
    mount_chat_multi_tool_call(server).await;
    mount_chat_completion(server).await;

    let test = test_codex()
        .with_config(move |config| profile.configure(config))
        .build_with_auto_env(server)
        .await?;
    test.submit_turn("run two conformance tools").await?;

    let requests = server
        .received_requests()
        .await
        .context("mock server should retain received requests")?;
    let chat_requests = requests
        .iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .collect::<Vec<_>>();
    assert_eq!(chat_requests.len(), 2);

    let second_body: Value = serde_json::from_slice(&chat_requests[1].body)?;
    assert_chat_tool_output(&second_body, "call-1", "conformance-one")?;
    assert_chat_tool_output(&second_body, "call-2", "conformance-two")?;
    Ok(())
}

fn tool_arguments(output: &str) -> String {
    json!({"cmd": format!("printf {output}")}).to_string()
}

fn assert_tool_output(
    request: &core_test_support::responses::ResponsesRequest,
    call_id: &str,
    expected: &str,
) -> Result<()> {
    let output = request
        .function_call_output_text(call_id)
        .with_context(|| format!("missing tool result for {call_id}"))?;
    assert!(
        output.contains(expected),
        "tool output for {call_id} did not contain {expected:?}: {output:?}"
    );
    Ok(())
}

fn assert_chat_tool_output(body: &Value, call_id: &str, expected: &str) -> Result<()> {
    let tool_result = body["messages"]
        .as_array()
        .context("Chat request messages should be an array")?
        .iter()
        .find(|message| {
            message.get("role").and_then(Value::as_str) == Some("tool")
                && message.get("tool_call_id").and_then(Value::as_str) == Some(call_id)
        })
        .with_context(|| format!("missing Chat tool result for {call_id}"))?;
    let output = tool_result["content"]
        .as_str()
        .with_context(|| format!("Chat tool result for {call_id} should be a string"))?;
    assert!(
        output.contains(expected),
        "Chat tool output for {call_id} did not contain {expected:?}: {output:?}"
    );
    Ok(())
}

async fn run_responses_tool_round_trip(server: &MockServer, profile: AdapterProfile) -> Result<()> {
    let arguments = json!({"cmd": format!("printf {TOOL_OUTPUT}")}).to_string();
    let mocks =
        responses::mount_function_call_agent_response(server, TOOL_CALL_ID, &arguments, TOOL_NAME)
            .await;
    let test = test_codex()
        .with_config(move |config| profile.configure(config))
        .build_with_auto_env(server)
        .await?;

    test.submit_turn("run the conformance tool").await?;

    let first_request = mocks.function_call.single_request();
    let first_body = first_request.body_json();
    assert_responses_function_tool(&first_body, profile);
    if matches!(profile, AdapterProfile::ResponsesFunctionOnly) {
        assert_no_native_responses_tools(&first_body);
    }

    let second_request = mocks.completion.single_request();
    let tool_output = second_request
        .function_call_output_text(TOOL_CALL_ID)
        .context("second Responses request should contain the tool result")?;
    assert!(
        tool_output.contains(TOOL_OUTPUT),
        "tool output did not contain {TOOL_OUTPUT:?}: {tool_output:?}"
    );
    Ok(())
}

async fn run_chat_tool_round_trip(server: &MockServer, profile: AdapterProfile) -> Result<()> {
    mount_chat_tool_call(server).await;
    mount_chat_completion(server).await;

    let test = test_codex()
        .with_config(move |config| profile.configure(config))
        .build_with_auto_env(server)
        .await?;
    test.submit_turn("run the conformance tool").await?;

    let requests = server
        .received_requests()
        .await
        .context("mock server should retain received requests")?;
    let chat_requests = requests
        .iter()
        .filter(|request| request.url.path() == "/v1/chat/completions")
        .collect::<Vec<_>>();
    assert_eq!(chat_requests.len(), 2);

    let first_body: Value = serde_json::from_slice(&chat_requests[0].body)?;
    assert_chat_function_tool(&first_body);

    let second_body: Value = serde_json::from_slice(&chat_requests[1].body)?;
    let tool_result = second_body["messages"]
        .as_array()
        .context("Chat request messages should be an array")?
        .iter()
        .find(|message| {
            message.get("role").and_then(Value::as_str) == Some("tool")
                && message.get("tool_call_id").and_then(Value::as_str) == Some(TOOL_CALL_ID)
        })
        .context("second Chat request should contain the tool result")?;
    let tool_output = tool_result["content"]
        .as_str()
        .context("Chat tool result content should be a string")?;
    assert!(
        tool_output.contains(TOOL_OUTPUT),
        "tool output did not contain {TOOL_OUTPUT:?}: {tool_output:?}"
    );
    Ok(())
}

fn assert_responses_function_tool(body: &Value, profile: AdapterProfile) {
    let tools = body["tools"]
        .as_array()
        .expect("Responses request tools should be an array");
    assert!(
        tools.iter().any(|tool| {
            tool.get("type").and_then(Value::as_str) == Some("function")
                && tool.get("name").and_then(Value::as_str) == Some(TOOL_NAME)
        }),
        "{profile:?} did not expose {TOOL_NAME} as a function tool: {tools:?}"
    );
}

fn assert_no_native_responses_tools(body: &Value) {
    let tools = body["tools"]
        .as_array()
        .expect("Responses request tools should be an array");
    for tool in tools {
        let tool_type = tool.get("type").and_then(Value::as_str);
        assert!(
            matches!(tool_type, Some("function")),
            "function-only Responses request exposed a native tool: {tool}"
        );
    }
}

fn assert_chat_function_tool(body: &Value) {
    let tools = body["tools"]
        .as_array()
        .expect("Chat request tools should be an array");
    assert!(
        tools.iter().any(|tool| {
            tool.get("type").and_then(Value::as_str) == Some("function")
                && tool["function"].get("name").and_then(Value::as_str) == Some(TOOL_NAME)
        }),
        "Chat request did not expose {TOOL_NAME} as a function tool: {tools:?}"
    );
    assert!(
        tools.iter().all(|tool| {
            tool.get("type").and_then(Value::as_str) == Some("function")
                && tool.get("function").is_some()
        }),
        "Chat request exposed a non-function tool: {tools:?}"
    );
}

async fn mount_chat_tool_call(server: &MockServer) {
    let arguments = json!({"cmd": format!("printf {TOOL_OUTPUT}")}).to_string();
    let split_at = arguments
        .find(TOOL_OUTPUT)
        .expect("tool output should appear in arguments");
    let first_arguments = &arguments[..split_at];
    let second_arguments = &arguments[split_at..];

    let body = chat_sse(vec![
        json!({
            "id": "chat-1",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": TOOL_CALL_ID,
                        "type": "function",
                        "function": {
                            "name": TOOL_NAME,
                            "arguments": first_arguments,
                        }
                    }]
                },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chat-1",
            "choices": [{
                "index": 0,
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
        }),
        json!({
            "id": "chat-1",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }),
    ]);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(chat_sse_response(body))
        .up_to_n_times(1)
        .mount(server)
        .await;
}

async fn mount_chat_multi_tool_call(server: &MockServer) {
    let first_arguments = tool_arguments("conformance-one");
    let second_arguments = tool_arguments("conformance-two");
    let body = chat_sse(vec![
        json!({
            "id": "chat-multi-1",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "index": 0,
                            "id": "call-1",
                            "type": "function",
                            "function": {
                                "name": TOOL_NAME,
                                "arguments": first_arguments,
                            }
                        },
                        {
                            "index": 1,
                            "id": "call-2",
                            "type": "function",
                            "function": {
                                "name": TOOL_NAME,
                                "arguments": second_arguments,
                            }
                        }
                    ]
                },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chat-multi-1",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }),
    ]);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(chat_sse_response(body))
        .up_to_n_times(1)
        .mount(server)
        .await;
}

async fn mount_chat_completion(server: &MockServer) {
    let body = chat_sse(vec![
        json!({
            "id": "chat-2",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": "done"
                },
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chat-2",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 2,
                "total_tokens": 22
            }
        }),
    ]);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(chat_sse_response(body))
        .up_to_n_times(1)
        .mount(server)
        .await;
}

fn chat_sse(chunks: Vec<Value>) -> String {
    use std::fmt::Write as _;

    let mut body = String::new();
    for chunk in chunks {
        writeln!(&mut body, "data: {chunk}\n").expect("writing to a String cannot fail");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

fn chat_sse_response(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(body)
}
