use codex_protocol::ToolName;
use codex_protocol::models::ResponseItem;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;

use super::ResponseTransportKind;
use super::WireAdapter;
use super::WireCapabilities;

fn function(name: &str) -> ResponsesApiTool {
    ResponsesApiTool {
        name: name.to_string(),
        description: String::new(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::default(),
        output_schema: None,
    }
}

#[test]
fn adapters_expose_expected_capabilities_and_transport() {
    let cases = [
        (
            WireAdapter::ResponsesNative,
            WireCapabilities::native_responses(),
            ResponseTransportKind::Responses,
        ),
        (
            WireAdapter::ResponsesFunctionOnly,
            WireCapabilities::function_only(),
            ResponseTransportKind::Responses,
        ),
        (
            WireAdapter::ChatCompletionsFunctionOnly,
            WireCapabilities::function_only(),
            ResponseTransportKind::ChatCompletions,
        ),
    ];

    for (adapter, capabilities, transport_kind) in cases {
        assert_eq!(adapter.capabilities(), capabilities);
        assert_eq!(adapter.transport_kind(), transport_kind);
    }
}

#[test]
fn function_only_adapter_adapts_history_and_decodes_tool_names() {
    let adapter = WireAdapter::ChatCompletionsFunctionOnly;
    let plan = adapter
        .tool_plan([ToolSpec::Namespace(ResponsesApiNamespace {
            name: "collaboration".to_string(),
            description: String::new(),
            tools: vec![ResponsesApiNamespaceTool::Function(function("spawn_agent"))],
        })])
        .expect("tool plan should be valid");
    let call = ResponseItem::FunctionCall {
        id: None,
        name: "spawn_agent".to_string(),
        namespace: Some("collaboration".to_string()),
        arguments: "{}".to_string(),
        encrypted_function_args: None,
        call_id: "call-1".to_string(),
        internal_chat_message_metadata_passthrough: None,
    };

    assert_eq!(
        adapter.adapt_history(&plan, [call]),
        vec![ResponseItem::FunctionCall {
            id: None,
            name: "collaboration__spawn_agent".to_string(),
            namespace: None,
            arguments: "{}".to_string(),
            encrypted_function_args: None,
            call_id: "call-1".to_string(),
            internal_chat_message_metadata_passthrough: None,
        }]
    );
    assert_eq!(
        adapter.decode_tool_name(&plan, "collaboration__spawn_agent"),
        ToolName::namespaced("collaboration", "spawn_agent")
    );
}

#[test]
fn native_adapter_preserves_history_and_plain_tool_names() {
    let adapter = WireAdapter::ResponsesNative;
    let plan = adapter.tool_plan([]).expect("tool plan should be valid");
    let call = ResponseItem::FunctionCall {
        id: None,
        name: "spawn_agent".to_string(),
        namespace: None,
        arguments: "{}".to_string(),
        encrypted_function_args: None,
        call_id: "call-1".to_string(),
        internal_chat_message_metadata_passthrough: None,
    };

    assert_eq!(adapter.adapt_history(&plan, [call.clone()]), vec![call]);
    assert_eq!(
        adapter.decode_tool_name(&plan, "spawn_agent"),
        ToolName::plain("spawn_agent")
    );
}
