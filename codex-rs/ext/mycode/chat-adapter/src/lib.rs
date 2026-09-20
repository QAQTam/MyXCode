//! Chat Completions transport adapter for Codex's canonical model boundary.
//!
//! Request translation lives in `codex-mycode-model-wire`. This crate owns the
//! HTTP request and the Chat SSE-to-ResponseEvent conversion.

mod client;
mod sse;

pub use client::CHAT_COMPLETIONS_PATH;
pub use client::ChatCompletionsClient;
pub use client::ChatOptions;
pub use sse::spawn_chat_stream;
