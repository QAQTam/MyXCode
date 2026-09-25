//! Provider-neutral model request and tool-wire adaptation.
//!
//! This crate owns pure translation between Codex's canonical conversation
//! history and provider wire formats. It deliberately has no dependency on
//! sessions, tool execution, authentication, or transport.

mod canonical;
mod capabilities;
mod chat;
mod tool_plan;

pub use canonical::CanonicalEvent;
pub use canonical::CanonicalItem;
pub use canonical::CanonicalRequest;
pub use canonical::CanonicalToolCall;
pub use canonical::CanonicalToolChoice;
pub use canonical::CanonicalToolResult;
pub use capabilities::ResponseTransportKind;
pub use capabilities::WireAdapter;
pub use capabilities::WireCapabilities;
pub use chat::ChatWireError;
pub use chat::build_chat_request;
pub use tool_plan::FLAT_TOOL_NAME_SEPARATOR;
pub use tool_plan::ToolPlan;
pub use tool_plan::ToolPlanError;
