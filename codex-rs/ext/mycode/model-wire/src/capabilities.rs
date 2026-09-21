use codex_protocol::ToolName;
use codex_protocol::models::ResponseItem;
use codex_tools::ToolSpec;

use crate::ToolPlan;
use crate::ToolPlanError;

/// Provider features available to a model-wire adapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WireCapabilities {
    pub namespace_tools: bool,
    pub custom_tools: bool,
    pub responses_lite: bool,
    pub tool_search: bool,
    pub built_in_tools: bool,
    pub parallel_tool_calls: bool,
}

impl WireCapabilities {
    /// Full native Responses capabilities used by the OpenAI Responses API.
    pub const fn native_responses() -> Self {
        Self {
            namespace_tools: true,
            custom_tools: true,
            responses_lite: true,
            tool_search: true,
            built_in_tools: true,
            parallel_tool_calls: true,
        }
    }

    /// Conservative function-only capabilities shared by generic Responses
    /// and Chat Completions providers.
    pub const fn function_only() -> Self {
        Self {
            namespace_tools: false,
            custom_tools: false,
            responses_lite: false,
            tool_search: false,
            built_in_tools: false,
            parallel_tool_calls: true,
        }
    }
}

/// Transport implementation selected by a wire adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseTransportKind {
    Responses,
    ChatCompletions,
}

/// Wire adapter selected for a provider.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WireAdapter {
    #[default]
    ResponsesNative,
    ResponsesFunctionOnly,
    ChatCompletionsFunctionOnly,
}

impl WireAdapter {
    /// Returns the provider features exposed by this wire.
    pub const fn capabilities(self) -> WireCapabilities {
        match self {
            Self::ResponsesNative => WireCapabilities::native_responses(),
            Self::ResponsesFunctionOnly | Self::ChatCompletionsFunctionOnly => {
                WireCapabilities::function_only()
            }
        }
    }

    /// Returns the transport implementation used by this wire.
    pub const fn transport_kind(self) -> ResponseTransportKind {
        match self {
            Self::ResponsesNative | Self::ResponsesFunctionOnly => ResponseTransportKind::Responses,
            Self::ChatCompletionsFunctionOnly => ResponseTransportKind::ChatCompletions,
        }
    }

    /// Applies this wire's capabilities to the model-visible tool surface.
    pub fn tool_plan(
        self,
        tools: impl IntoIterator<Item = ToolSpec>,
    ) -> Result<ToolPlan, ToolPlanError> {
        ToolPlan::new(tools, self.capabilities())
    }

    /// Adapts a request-local copy of canonical history to this wire.
    pub fn adapt_history(
        self,
        plan: &ToolPlan,
        items: impl IntoIterator<Item = ResponseItem>,
    ) -> Vec<ResponseItem> {
        match self {
            Self::ResponsesNative => items.into_iter().collect(),
            Self::ResponsesFunctionOnly | Self::ChatCompletionsFunctionOnly => {
                plan.adapt_items(items)
            }
        }
    }

    /// Decodes a model-emitted tool name into the canonical namespace.
    pub fn decode_tool_name(self, plan: &ToolPlan, wire_name: &str) -> ToolName {
        match self {
            Self::ResponsesNative => ToolName::plain(wire_name),
            Self::ResponsesFunctionOnly | Self::ChatCompletionsFunctionOnly => plan
                .decode_flat_name(wire_name)
                .unwrap_or_else(|| ToolName::plain(wire_name)),
        }
    }
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
