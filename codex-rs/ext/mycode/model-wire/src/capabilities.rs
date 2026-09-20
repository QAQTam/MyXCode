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

/// Wire adapter selected for a provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireAdapter {
    ResponsesNative,
    ResponsesFunctionOnly,
    ChatCompletionsFunctionOnly,
}

impl WireAdapter {
    pub const fn responses(capabilities: WireCapabilities) -> Self {
        if capabilities.namespace_tools
            || capabilities.custom_tools
            || capabilities.responses_lite
            || capabilities.tool_search
            || capabilities.built_in_tools
        {
            Self::ResponsesNative
        } else {
            Self::ResponsesFunctionOnly
        }
    }

    pub const fn chat_completions() -> Self {
        Self::ChatCompletionsFunctionOnly
    }
}
