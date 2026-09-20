use codex_protocol::ToolName;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use codex_tools::ToolSpec;
use serde_json::Value;
use serde_json::json;
use thiserror::Error;

use crate::CanonicalRequest;
use crate::CanonicalToolChoice;
use crate::FLAT_TOOL_NAME_SEPARATOR;

/// Error produced while translating a canonical request to Chat Completions.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ChatWireError {
    #[error("Chat Completions is function-only and cannot encode tool `{0}`")]
    UnsupportedTool(String),
}

/// Converts a canonical function-only request into a Chat Completions body.
pub fn build_chat_request(request: &CanonicalRequest) -> Result<Value, ChatWireError> {
    let mut messages = Vec::new();
    if !request.instructions.is_empty() {
        messages.push(json!({"role": "system", "content": request.instructions}));
    }

    let mut last_assistant_text: Option<String> = None;
    for item in &request.items {
        match item {
            ResponseItem::Message { role, content, .. } => {
                let text = message_text(content);
                if role == "assistant" {
                    if last_assistant_text.as_deref() == Some(text.as_str()) {
                        continue;
                    }
                    last_assistant_text = Some(text.clone());
                }
                messages.push(json!({
                    "role": role,
                    "content": message_content(role, content, text),
                }));
            }
            ResponseItem::FunctionCall {
                name,
                namespace,
                arguments,
                call_id,
                ..
            } => {
                let name = encode_tool_name(&ToolName::new(namespace.clone(), name.clone()));
                push_tool_call_message(
                    &mut messages,
                    json!({
                        "id": call_id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments,
                        },
                    }),
                );
            }
            ResponseItem::FunctionCallOutput {
                call_id, output, ..
            } => {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id.clone().unwrap_or_default(),
                    "content": tool_output_content_value(output),
                }));
            }
            ResponseItem::Reasoning { content, .. } => {
                if let Some(reasoning) = content.as_deref().and_then(reasoning_text)
                    && let Some(Value::Object(object)) = messages.last_mut()
                    && object.get("role").and_then(Value::as_str) == Some("assistant")
                {
                    object.insert("reasoning".to_string(), Value::String(reasoning));
                }
            }
            ResponseItem::CustomToolCall { .. }
            | ResponseItem::CustomToolCallOutput { .. }
            | ResponseItem::AdditionalTools { .. }
            | ResponseItem::AgentMessage { .. }
            | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::ToolSearchOutput { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::ImageGenerationCall { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::ConfigurationUpdate { .. }
            | ResponseItem::CompactionTrigger { .. }
            | ResponseItem::ContextCompaction { .. }
            | ResponseItem::LocalShellCall { .. }
            | ResponseItem::Other => {}
        }
    }

    let tools = request
        .tools
        .iter()
        .map(chat_tool)
        .collect::<Result<Vec<_>, _>>()?;

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "tools": tools,
        "tool_choice": tool_choice(&request.tool_choice),
        "parallel_tool_calls": request.parallel_tool_calls,
        "stream": request.stream,
        "stream_options": {"include_usage": true},
    });
    if let Some(conversation_id) = request.provider_metadata.get("conversation_id") {
        body["user"] = json!(conversation_id);
    }
    Ok(body)
}

fn chat_tool(tool: &ToolSpec) -> Result<Value, ChatWireError> {
    let ToolSpec::Function(tool) = tool else {
        return Err(ChatWireError::UnsupportedTool(tool.name().to_string()));
    };
    Ok(json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        },
    }))
}

fn tool_choice(choice: &CanonicalToolChoice) -> Value {
    match choice {
        CanonicalToolChoice::Auto => json!("auto"),
        CanonicalToolChoice::None => json!("none"),
        CanonicalToolChoice::Required => json!("required"),
        CanonicalToolChoice::Function(name) => json!({
            "type": "function",
            "function": {"name": encode_tool_name(name)},
        }),
    }
}

fn encode_tool_name(name: &ToolName) -> String {
    if name.is_default_namespace() {
        name.name.clone()
    } else {
        format!(
            "{}{FLAT_TOOL_NAME_SEPARATOR}{}",
            name.namespace.as_deref().unwrap_or_default(),
            name.name
        )
    }
}

fn message_text(content: &[ContentItem]) -> String {
    content
        .iter()
        .filter_map(|item| match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                Some(text.as_str())
            }
            ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn message_content(role: &str, content: &[ContentItem], text: String) -> Value {
    if role == "assistant" {
        return json!(text);
    }

    let has_non_text = content.iter().any(|item| {
        matches!(
            item,
            ContentItem::InputImage { .. } | ContentItem::InputAudio { .. }
        )
    });
    if !has_non_text {
        return json!(text);
    }

    json!(
        content
            .iter()
            .map(content_item_value)
            .collect::<Vec<Value>>()
    )
}

fn content_item_value(item: &ContentItem) -> Value {
    match item {
        ContentItem::InputText { text } | ContentItem::OutputText { text } => {
            json!({"type": "text", "text": text})
        }
        ContentItem::InputImage {
            image: ImageReference::Inline { image_url },
            ..
        } => json!({"type": "image_url", "image_url": {"url": image_url}}),
        ContentItem::InputImage {
            image: ImageReference::File { .. },
            ..
        } => json!(item),
        ContentItem::InputAudio { .. } => json!(item),
    }
}

fn reasoning_text(content: &[ReasoningItemContent]) -> Option<String> {
    let text = content
        .iter()
        .map(|item| match item {
            ReasoningItemContent::ReasoningText { text } | ReasoningItemContent::Text { text } => {
                text.as_str()
            }
        })
        .collect::<Vec<_>>()
        .join("");
    (!text.is_empty()).then_some(text)
}

fn tool_output_content_value(output: &FunctionCallOutputPayload) -> Value {
    if let Some(items) = output.content_items() {
        json!(
            items
                .iter()
                .map(tool_output_item_value)
                .collect::<Vec<Value>>()
        )
    } else {
        json!(output.text_content().unwrap_or_default())
    }
}

fn tool_output_item_value(item: &FunctionCallOutputContentItem) -> Value {
    match item {
        FunctionCallOutputContentItem::InputText { text } => {
            json!({"type": "text", "text": text})
        }
        FunctionCallOutputContentItem::InputImage {
            image: ImageReference::Inline { image_url },
            ..
        } => json!({"type": "image_url", "image_url": {"url": image_url}}),
        FunctionCallOutputContentItem::InputImage {
            image: ImageReference::File { .. },
            ..
        } => json!(item),
        FunctionCallOutputContentItem::InputAudio { .. }
        | FunctionCallOutputContentItem::EncryptedContent { .. } => json!(item),
    }
}

fn push_tool_call_message(messages: &mut Vec<Value>, tool_call: Value) {
    if let Some(Value::Object(object)) = messages.last_mut()
        && object.get("role").and_then(Value::as_str) == Some("assistant")
    {
        if let Some(tool_calls) = object.get_mut("tool_calls").and_then(Value::as_array_mut) {
            tool_calls.push(tool_call);
        } else {
            object.insert("tool_calls".to_string(), Value::Array(vec![tool_call]));
        }
        return;
    }

    messages.push(json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [tool_call],
    }));
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;
