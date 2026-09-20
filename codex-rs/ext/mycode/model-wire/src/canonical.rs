use std::collections::BTreeMap;

use codex_protocol::ToolName;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use codex_tools::ToolSpec;

/// Canonical request sent to a wire adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalRequest {
    pub model: String,
    pub instructions: String,
    pub items: Vec<ResponseItem>,
    pub tools: Vec<ToolSpec>,
    pub tool_choice: CanonicalToolChoice,
    pub parallel_tool_calls: bool,
    pub stream: bool,
    pub provider_metadata: BTreeMap<String, String>,
}

/// Canonical tool selection for a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalToolChoice {
    Auto,
    None,
    Required,
    Function(ToolName),
}

/// Canonical conversation item at the wire boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum CanonicalItem {
    Item(ResponseItem),
    ToolCall(CanonicalToolCall),
    ToolResult(CanonicalToolResult),
}

/// Canonical function or custom tool call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalToolCall {
    pub call_id: String,
    pub name: ToolName,
    pub arguments: String,
}

/// Canonical tool result associated with a call.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalToolResult {
    pub call_id: String,
    pub name: Option<ToolName>,
    pub output: FunctionCallOutputPayload,
}

/// Canonical streaming event emitted by a wire adapter.
#[derive(Clone, Debug, PartialEq)]
pub enum CanonicalEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStarted {
        call_id: String,
        name: ToolName,
    },
    ToolCallArgumentsDelta {
        call_id: String,
        delta: String,
    },
    ToolCallCompleted(CanonicalToolCall),
    Usage(TokenUsage),
    Completed {
        response_id: String,
        end_turn: Option<bool>,
    },
    Error(String),
}
