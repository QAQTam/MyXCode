use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

use super::ChatWireError;
use super::build_chat_request;
use crate::CanonicalRequest;
use crate::CanonicalToolChoice;

fn function(name: &str) -> ToolSpec {
    ToolSpec::Function(ResponsesApiTool {
        name: name.to_string(),
        description: "test tool".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::default(),
        output_schema: None,
    })
}

fn message(role: &str, text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: role.to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn builds_function_only_request_and_groups_tool_calls() {
    let request = CanonicalRequest {
        model: "gpt-test".to_string(),
        instructions: "instructions".to_string(),
        items: vec![
            message("user", "hello"),
            ResponseItem::FunctionCall {
                id: None,
                name: "spawn_agent".to_string(),
                namespace: Some("collaboration".to_string()),
                arguments: "{}".to_string(),
                encrypted_function_args: None,
                call_id: "call-1".to_string(),
                internal_chat_message_metadata_passthrough: None,
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "send_message".to_string(),
                namespace: Some("collaboration".to_string()),
                arguments: r#"{"message":"hi"}"#.to_string(),
                encrypted_function_args: None,
                call_id: "call-2".to_string(),
                internal_chat_message_metadata_passthrough: None,
            },
            ResponseItem::FunctionCallOutput {
                id: None,
                call_id: Some("call-1".to_string()),
                name: None,
                namespace: None,
                output: FunctionCallOutputPayload::from_text("ok".to_string()),
                internal_chat_message_metadata_passthrough: None,
            },
            ResponseItem::FunctionCallOutput {
                id: None,
                call_id: Some("call-2".to_string()),
                name: None,
                namespace: None,
                output: FunctionCallOutputPayload::from_text("done".to_string()),
                internal_chat_message_metadata_passthrough: None,
            },
        ],
        tools: vec![
            function("collaboration__spawn_agent"),
            function("collaboration__send_message"),
        ],
        tool_choice: CanonicalToolChoice::Auto,
        parallel_tool_calls: true,
        stream: true,
        provider_metadata: BTreeMap::from([(
            "conversation_id".to_string(),
            "thread-1".to_string(),
        )]),
    };

    let body = build_chat_request(&request).expect("chat request");
    assert_eq!(body["model"], "gpt-test");
    assert_eq!(body["user"], "thread-1");
    assert_eq!(
        body["tools"][0]["function"]["name"],
        "collaboration__spawn_agent"
    );
    assert_eq!(
        body["messages"][0],
        json!({"role": "system", "content": "instructions"})
    );
    assert_eq!(
        body["messages"][1],
        json!({"role": "user", "content": "hello"})
    );
    assert_eq!(body["messages"][2]["role"], "assistant");
    assert_eq!(body["messages"][2]["content"], serde_json::Value::Null);
    assert_eq!(
        body["messages"][2]["tool_calls"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        body["messages"][2]["tool_calls"][0]["function"]["name"],
        "collaboration__spawn_agent"
    );
    assert_eq!(body["messages"][3]["role"], "tool");
    assert_eq!(body["messages"][3]["tool_call_id"], "call-1");
    assert_eq!(body["messages"][4]["role"], "tool");
    assert_eq!(body["messages"][4]["tool_call_id"], "call-2");
}

#[test]
fn rejects_non_function_tools() {
    let request = CanonicalRequest {
        model: "gpt-test".to_string(),
        instructions: String::new(),
        items: Vec::new(),
        tools: vec![ToolSpec::ToolSearch {
            execution: "client".to_string(),
            description: String::new(),
            parameters: JsonSchema::default(),
        }],
        tool_choice: CanonicalToolChoice::Auto,
        parallel_tool_calls: true,
        stream: true,
        provider_metadata: Default::default(),
    };

    assert_eq!(
        build_chat_request(&request),
        Err(ChatWireError::UnsupportedTool("tool_search".to_string()))
    );
}

#[test]
fn groups_assistant_commentary_with_following_tool_call() {
    let request = CanonicalRequest {
        model: "gpt-test".to_string(),
        instructions: String::new(),
        items: vec![
            message("assistant", "I will delegate this."),
            ResponseItem::FunctionCall {
                id: None,
                name: "spawn_agent".to_string(),
                namespace: None,
                arguments: r#"{"task_name":"worker"}"#.to_string(),
                encrypted_function_args: None,
                call_id: "call-1".to_string(),
                internal_chat_message_metadata_passthrough: None,
            },
        ],
        tools: vec![function("spawn_agent")],
        tool_choice: CanonicalToolChoice::Auto,
        parallel_tool_calls: false,
        stream: true,
        provider_metadata: Default::default(),
    };

    let body = build_chat_request(&request).expect("chat request");
    assert_eq!(
        body["messages"],
        json!([{
            "role": "assistant",
            "content": "I will delegate this.",
            "tool_calls": [{
                "id": "call-1",
                "type": "function",
                "function": {
                    "name": "spawn_agent",
                    "arguments": "{\"task_name\":\"worker\"}",
                },
            }],
        }])
    );
}
