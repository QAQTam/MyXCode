use codex_protocol::DEFAULT_FUNCTION_NAMESPACE;
use codex_protocol::ToolName;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use codex_tools::FreeformTool;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use std::collections::BTreeMap;
use thiserror::Error;

use crate::WireCapabilities;

/// Separator used to flatten namespaced tools into ordinary function names.
pub const FLAT_TOOL_NAME_SEPARATOR: &str = "__";

const MAX_FUNCTION_NAME_LEN: usize = 64;

/// A finalized model-visible tool surface plus the identities needed to
/// decode model calls back into canonical tool names.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolPlan {
    capabilities: WireCapabilities,
    tools: Vec<ToolSpec>,
    identities: BTreeMap<String, ToolName>,
    flattened_tool_count: usize,
    hidden_tool_count: usize,
}

impl ToolPlan {
    /// Applies capability-driven filtering and namespace flattening.
    pub fn new(
        specs: impl IntoIterator<Item = ToolSpec>,
        capabilities: WireCapabilities,
    ) -> Result<Self, ToolPlanError> {
        let mut plan = Self {
            capabilities,
            tools: Vec::new(),
            identities: BTreeMap::new(),
            flattened_tool_count: 0,
            hidden_tool_count: 0,
        };

        for spec in specs {
            match spec {
                ToolSpec::Function(tool) => {
                    let identity = ToolName::plain(tool.name.clone());
                    plan.push_function(tool.name.clone(), tool, identity)?;
                }
                ToolSpec::Namespace(namespace) => plan.push_namespace(namespace)?,
                ToolSpec::Freeform(tool) => plan.push_freeform(tool, None)?,
                spec @ ToolSpec::ToolSearch { .. } if capabilities.tool_search => {
                    plan.push_passthrough(spec);
                }
                spec @ ToolSpec::WebSearch { .. } if capabilities.built_in_tools => {
                    plan.push_passthrough(spec);
                }
                ToolSpec::ToolSearch { .. } | ToolSpec::WebSearch { .. } => {
                    plan.hidden_tool_count += 1;
                }
            }
        }

        Ok(plan)
    }

    pub fn capabilities(&self) -> WireCapabilities {
        self.capabilities
    }

    pub fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    pub fn identities(&self) -> &BTreeMap<String, ToolName> {
        &self.identities
    }

    /// Number of namespaced functions converted to flat wire names.
    pub fn flattened_tool_count(&self) -> usize {
        self.flattened_tool_count
    }

    /// Number of tools omitted because the selected wire cannot encode them.
    pub fn hidden_tool_count(&self) -> usize {
        self.hidden_tool_count
    }

    /// Decodes a flat function name emitted by a function-only wire.
    pub fn decode_flat_name(&self, flat_name: &str) -> Option<ToolName> {
        self.identities.get(flat_name).cloned()
    }

    /// Encodes a canonical tool identity into the name exposed on this wire.
    pub fn encode_tool_name(&self, tool_name: &ToolName) -> Option<String> {
        let normalized = tool_name.clone().with_default_namespace();
        self.identities.iter().find_map(|(wire_name, identity)| {
            (identity.clone().with_default_namespace() == normalized).then(|| wire_name.clone())
        })
    }

    /// Adapts stored conversation items to the features available on this wire.
    pub fn adapt_items(&self, items: impl IntoIterator<Item = ResponseItem>) -> Vec<ResponseItem> {
        items
            .into_iter()
            .filter_map(|item| self.adapt_item(item))
            .collect()
    }

    fn adapt_item(&self, item: ResponseItem) -> Option<ResponseItem> {
        match item {
            ResponseItem::FunctionCall {
                id,
                name,
                namespace,
                arguments,
                encrypted_function_args,
                call_id,
                internal_chat_message_metadata_passthrough,
            } => {
                let tool_name = ToolName::new(namespace, name).with_default_namespace();
                let (name, namespace) = if self.capabilities.namespace_tools {
                    (tool_name.name, tool_name.namespace)
                } else {
                    (
                        self.encode_tool_name(&tool_name).unwrap_or(tool_name.name),
                        None,
                    )
                };
                Some(ResponseItem::FunctionCall {
                    id,
                    name,
                    namespace,
                    arguments,
                    encrypted_function_args: self
                        .capabilities
                        .namespace_tools
                        .then_some(encrypted_function_args)
                        .flatten(),
                    call_id,
                    internal_chat_message_metadata_passthrough,
                })
            }
            ResponseItem::AgentMessage {
                id,
                author,
                recipient,
                content,
                internal_chat_message_metadata_passthrough,
            } if !self.capabilities.namespace_tools => {
                let text = plaintext_agent_message_content(&content).unwrap_or_else(|| {
                    format!(
                        "Agent message from {author} to {recipient} could not be delivered: \
                         the selected provider does not support encrypted agent messages."
                    )
                });
                Some(ResponseItem::Message {
                    id,
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText { text }],
                    phase: None,
                    internal_chat_message_metadata_passthrough,
                })
            }
            ResponseItem::CustomToolCall { .. } if !self.capabilities.custom_tools => None,
            ResponseItem::CustomToolCallOutput { .. } if !self.capabilities.custom_tools => None,
            ResponseItem::ToolSearchCall { .. } | ResponseItem::ToolSearchOutput { .. }
                if !self.capabilities.tool_search =>
            {
                None
            }
            ResponseItem::WebSearchCall { .. } if !self.capabilities.built_in_tools => None,
            ResponseItem::ImageGenerationCall { .. } if !self.capabilities.built_in_tools => None,
            ResponseItem::AdditionalTools { .. } if !self.capabilities.responses_lite => None,
            item => Some(item),
        }
    }

    fn push_passthrough(&mut self, spec: ToolSpec) {
        self.tools.push(spec);
    }

    fn push_function(
        &mut self,
        wire_name: String,
        tool: ResponsesApiTool,
        identity: ToolName,
    ) -> Result<(), ToolPlanError> {
        self.insert_identity(&wire_name, identity)?;
        self.tools.push(ToolSpec::Function(tool));
        Ok(())
    }

    fn push_freeform(
        &mut self,
        tool: FreeformTool,
        namespace: Option<&str>,
    ) -> Result<(), ToolPlanError> {
        if !self.capabilities.custom_tools {
            self.hidden_tool_count += 1;
            return Ok(());
        }
        let identity = ToolName::new(namespace.map(str::to_string), tool.name.clone());
        self.insert_identity(&tool.name, identity)?;
        self.tools.push(ToolSpec::Freeform(tool));
        Ok(())
    }

    fn push_namespace(
        &mut self,
        mut namespace: ResponsesApiNamespace,
    ) -> Result<(), ToolPlanError> {
        if self.capabilities.namespace_tools {
            if !self.capabilities.custom_tools {
                let hidden = namespace
                    .tools
                    .iter()
                    .filter(|tool| matches!(tool, ResponsesApiNamespaceTool::Custom(_)))
                    .count();
                self.hidden_tool_count += hidden;
                namespace
                    .tools
                    .retain(|tool| matches!(tool, ResponsesApiNamespaceTool::Function(_)));
            }
            let identity_namespace = Some(namespace.name.clone());
            for tool in &namespace.tools {
                match tool {
                    ResponsesApiNamespaceTool::Function(tool) => self.insert_identity_unchecked(
                        &flatten_tool_name(&namespace.name, &tool.name),
                        ToolName::new(identity_namespace.clone(), tool.name.clone()),
                    )?,
                    ResponsesApiNamespaceTool::Custom(tool) => self.insert_identity_unchecked(
                        &flatten_tool_name(&namespace.name, &tool.name),
                        ToolName::new(identity_namespace.clone(), tool.name.clone()),
                    )?,
                }
            }
            if !namespace.tools.is_empty() {
                self.tools.push(ToolSpec::Namespace(namespace));
            }
            return Ok(());
        }

        for tool in namespace.tools {
            match tool {
                ResponsesApiNamespaceTool::Function(tool) => {
                    let wire_name = flatten_tool_name(&namespace.name, &tool.name);
                    let identity = ToolName::namespaced(namespace.name.clone(), tool.name.clone());
                    let mut tool = tool;
                    tool.name = wire_name.clone();
                    self.push_function(wire_name, tool, identity)?;
                    self.flattened_tool_count += 1;
                }
                ResponsesApiNamespaceTool::Custom(_) => {
                    self.hidden_tool_count += 1;
                }
            }
        }
        Ok(())
    }

    fn insert_identity(
        &mut self,
        wire_name: &str,
        identity: ToolName,
    ) -> Result<(), ToolPlanError> {
        validate_wire_name(wire_name)?;
        self.insert_identity_unchecked(wire_name, identity)
    }

    fn insert_identity_unchecked(
        &mut self,
        key: &str,
        identity: ToolName,
    ) -> Result<(), ToolPlanError> {
        if self.identities.contains_key(key) {
            return Err(ToolPlanError::Collision {
                name: key.to_string(),
            });
        }
        self.identities.insert(key.to_string(), identity);
        Ok(())
    }
}

/// Error produced while finalizing a model-visible tool plan.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ToolPlanError {
    #[error("tool name cannot be empty")]
    EmptyName,
    #[error("tool name `{name}` exceeds the {max_len}-character wire limit")]
    NameTooLong { name: String, max_len: usize },
    #[error("multiple tools map to the wire name `{name}`")]
    Collision { name: String },
}

fn flatten_tool_name(namespace: &str, name: &str) -> String {
    if namespace == DEFAULT_FUNCTION_NAMESPACE {
        name.to_string()
    } else {
        format!("{namespace}{FLAT_TOOL_NAME_SEPARATOR}{name}")
    }
}

fn validate_wire_name(name: &str) -> Result<(), ToolPlanError> {
    if name.is_empty() {
        return Err(ToolPlanError::EmptyName);
    }
    if name.len() > MAX_FUNCTION_NAME_LEN {
        return Err(ToolPlanError::NameTooLong {
            name: name.to_string(),
            max_len: MAX_FUNCTION_NAME_LEN,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "tool_plan_tests.rs"]
mod tests;
