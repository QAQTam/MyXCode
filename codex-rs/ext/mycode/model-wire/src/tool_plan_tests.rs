use codex_protocol::ToolName;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;

use super::FLAT_TOOL_NAME_SEPARATOR;
use super::ToolPlan;
use crate::WireCapabilities;

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
fn function_only_flattens_namespaces() {
    let plan = ToolPlan::new(
        [ToolSpec::Namespace(ResponsesApiNamespace {
            name: "collaboration".to_string(),
            description: String::new(),
            tools: vec![ResponsesApiNamespaceTool::Function(function("spawn_agent"))],
        })],
        WireCapabilities::function_only(),
    )
    .expect("tool plan should be valid");

    assert_eq!(
        plan.tools(),
        &[ToolSpec::Function(ResponsesApiTool {
            name: format!("collaboration{FLAT_TOOL_NAME_SEPARATOR}spawn_agent"),
            ..function("spawn_agent")
        })]
    );
    assert_eq!(
        plan.decode_flat_name("collaboration__spawn_agent"),
        Some(ToolName::namespaced("collaboration", "spawn_agent"))
    );
    assert_eq!(plan.flattened_tool_count(), 1);
    assert_eq!(plan.hidden_tool_count(), 0);
}

#[test]
fn function_only_hides_custom_and_builtin_tools() {
    let plan = ToolPlan::new(
        [
            ToolSpec::Freeform(codex_tools::FreeformTool {
                name: "apply_patch".to_string(),
                description: String::new(),
                defer_loading: None,
                format: codex_tools::FreeformToolFormat {
                    r#type: "grammar".to_string(),
                    syntax: "lark".to_string(),
                    definition: "start: /.*/".to_string(),
                },
            }),
            ToolSpec::WebSearch {
                external_web_access: None,
                indexed_web_access: None,
                filters: None,
                user_location: None,
                search_context_size: None,
                search_content_types: None,
            },
        ],
        WireCapabilities::function_only(),
    )
    .expect("tool plan should be valid");

    assert_eq!(plan.tools(), &[]);
    assert_eq!(plan.flattened_tool_count(), 0);
    assert_eq!(plan.hidden_tool_count(), 2);
}

#[test]
fn native_responses_keeps_namespaces_and_custom_tools() {
    let spec = ToolSpec::Namespace(ResponsesApiNamespace {
        name: "collaboration".to_string(),
        description: String::new(),
        tools: vec![ResponsesApiNamespaceTool::Function(function("spawn_agent"))],
    });
    let plan = ToolPlan::new([spec.clone()], WireCapabilities::native_responses())
        .expect("tool plan should be valid");

    assert_eq!(plan.tools(), &[spec]);
}

#[test]
fn native_responses_keeps_built_in_tools() {
    let spec = ToolSpec::WebSearch {
        external_web_access: Some(true),
        indexed_web_access: None,
        filters: None,
        user_location: None,
        search_context_size: None,
        search_content_types: None,
    };
    let plan = ToolPlan::new([spec.clone()], WireCapabilities::native_responses())
        .expect("tool plan should be valid");

    assert_eq!(plan.tools(), &[spec]);
}

#[test]
fn native_responses_hides_namespaced_custom_tools_when_unsupported() {
    let plan = ToolPlan::new(
        [ToolSpec::Namespace(ResponsesApiNamespace {
            name: "files".to_string(),
            description: String::new(),
            tools: vec![
                ResponsesApiNamespaceTool::Function(function("read")),
                ResponsesApiNamespaceTool::Custom(codex_tools::FreeformTool {
                    name: "patch".to_string(),
                    description: String::new(),
                    defer_loading: None,
                    format: codex_tools::FreeformToolFormat {
                        r#type: "grammar".to_string(),
                        syntax: "lark".to_string(),
                        definition: "start: /.*/".to_string(),
                    },
                }),
            ],
        })],
        WireCapabilities {
            namespace_tools: true,
            custom_tools: false,
            ..WireCapabilities::native_responses()
        },
    )
    .expect("tool plan");

    assert_eq!(
        plan.tools(),
        &[ToolSpec::Namespace(ResponsesApiNamespace {
            name: "files".to_string(),
            description: String::new(),
            tools: vec![ResponsesApiNamespaceTool::Function(function("read"))],
        })]
    );
    assert_eq!(plan.hidden_tool_count(), 1);
}

#[test]
fn function_only_adapts_plaintext_agent_messages_to_user_messages() {
    let plan = ToolPlan::new([], WireCapabilities::function_only()).expect("tool plan");
    let message = ResponseItem::AgentMessage {
        id: None,
        author: "/root".to_string(),
        recipient: "/root/worker".to_string(),
        content: vec![AgentMessageInputContent::InputText {
            text: "Message Type: NEW_TASK\nPayload:\ndo work".to_string(),
        }],
        internal_chat_message_metadata_passthrough: None,
    };

    assert_eq!(
        plan.adapt_items([message]),
        vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "Message Type: NEW_TASK\nPayload:\ndo work".to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }]
    );
}

#[test]
fn function_only_reports_encrypted_agent_messages_as_undeliverable() {
    let plan = ToolPlan::new([], WireCapabilities::function_only()).expect("tool plan");
    let message = ResponseItem::AgentMessage {
        id: None,
        author: "/root".to_string(),
        recipient: "/root/worker".to_string(),
        content: vec![AgentMessageInputContent::EncryptedContent {
            encrypted_content: "ciphertext".to_string(),
        }],
        internal_chat_message_metadata_passthrough: None,
    };

    assert_eq!(
        plan.adapt_items([message]),
        vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "Agent message from /root to /root/worker could not be delivered: \
                       the selected provider does not support encrypted agent messages."
                    .to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }]
    );
}

#[test]
fn native_responses_preserves_agent_messages() {
    let plan = ToolPlan::new([], WireCapabilities::native_responses()).expect("tool plan");
    let message = ResponseItem::AgentMessage {
        id: None,
        author: "/root".to_string(),
        recipient: "/root/worker".to_string(),
        content: vec![AgentMessageInputContent::InputText {
            text: "do work".to_string(),
        }],
        internal_chat_message_metadata_passthrough: None,
    };

    assert_eq!(plan.adapt_items([message.clone()]), vec![message]);
}

#[test]
fn function_only_adapts_calls_and_preserves_tool_results() {
    let plan = ToolPlan::new(
        [ToolSpec::Namespace(ResponsesApiNamespace {
            name: "collaboration".to_string(),
            description: String::new(),
            tools: vec![ResponsesApiNamespaceTool::Function(function("spawn_agent"))],
        })],
        WireCapabilities::function_only(),
    )
    .expect("tool plan");
    let call = ResponseItem::FunctionCall {
        id: None,
        name: "spawn_agent".to_string(),
        namespace: Some("collaboration".to_string()),
        arguments: "{}".to_string(),
        encrypted_function_args: None,
        call_id: "call-1".to_string(),
        internal_chat_message_metadata_passthrough: None,
    };
    let output = ResponseItem::FunctionCallOutput {
        id: None,
        call_id: Some("call-1".to_string()),
        name: None,
        namespace: None,
        output: FunctionCallOutputPayload::from_text("ok".to_string()),
        internal_chat_message_metadata_passthrough: None,
    };

    assert_eq!(
        plan.adapt_items([call, output.clone()]),
        vec![
            ResponseItem::FunctionCall {
                id: None,
                name: "collaboration__spawn_agent".to_string(),
                namespace: None,
                arguments: "{}".to_string(),
                encrypted_function_args: None,
                call_id: "call-1".to_string(),
                internal_chat_message_metadata_passthrough: None,
            },
            output,
        ]
    );
}
