//! Sandboxed adapter for the `read_file` tool.

use std::io::ErrorKind;

use codex_exec_server::GetMetadataOptions;
use codex_mycode_file_tools::read::MAX_OUTPUT_BYTES;
use codex_mycode_file_tools::read::ReadScanner;
use codex_mycode_file_tools::read::render_read_output;
use codex_mycode_file_tools::spec::create_read_file_tool;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::TruncationPolicy;
use codex_utils_output_truncation::with_serialization_allowance;
use futures::StreamExt;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::handlers::resolve_tool_environment;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::ToolName;
use codex_tools::ToolSpec;

struct ReadFileOutput {
    text: String,
}

impl ToolOutput for ReadFileOutput {
    fn log_output(&self) -> String {
        self.text.clone()
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn fallback_token_limit_override(&self) -> Option<usize> {
        Some(with_serialization_allowance(TruncationPolicy::Bytes(MAX_OUTPUT_BYTES)).token_budget())
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        FunctionToolOutput::from_text(self.text.clone(), Some(true))
            .to_response_item(call_id, payload)
    }
}

/// Handles `read_file` calls against the selected turn environment filesystem.
#[derive(Default)]
pub struct ReadFileHandler;

#[derive(Deserialize)]
struct ReadFileArgs {
    file_path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

impl ToolExecutor<ToolInvocation> for ReadFileHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("read_file")
    }

    fn spec(&self) -> ToolSpec {
        create_read_file_tool()
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(self.handle_call(invocation))
    }
}

impl CoreToolRuntime for ReadFileHandler {}

impl ReadFileHandler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            step_context,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "read_file handler received unsupported payload".to_string(),
                ));
            }
        };
        let ReadFileArgs {
            file_path,
            offset,
            limit,
        } = parse_arguments(&arguments)?;

        let requested_offset = offset.unwrap_or(1).max(1);
        let Some(turn_environment) = resolve_tool_environment(&step_context.environments, None)?
        else {
            return Err(FunctionCallError::RespondToModel(
                "read_file is unavailable in this session".to_string(),
            ));
        };
        let path_uri = turn_environment.cwd().join(&file_path).map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "unable to resolve `{file_path}` against environment cwd `{}`: {err}",
                turn_environment.cwd(),
            ))
        })?;
        let model_visible_path = path_uri.inferred_native_path_string();

        let fs = turn_environment.environment.get_filesystem();
        let sandbox = turn_environment.sandbox_context(/*additional_permissions*/ None);

        let metadata = match fs
            .get_metadata(&path_uri, GetMetadataOptions::default(), Some(&sandbox))
            .await
        {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "file not found: `{model_visible_path}`"
                )));
            }
            Err(err) => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "unable to access `{model_visible_path}`: {err}"
                )));
            }
        };
        if metadata.is_directory {
            return Err(FunctionCallError::RespondToModel(format!(
                "`{model_visible_path}` is a directory, not a file; use exec (e.g. `ls`) to list a directory"
            )));
        }
        if !metadata.is_file {
            return Err(FunctionCallError::RespondToModel(format!(
                "`{model_visible_path}` is not a regular file"
            )));
        }

        let mut scanner = ReadScanner::new(offset, limit);
        let mut stream = fs
            .read_file_stream(&path_uri, Some(&sandbox))
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "unable to read `{model_visible_path}`: {err}"
                ))
            })?;
        while !scanner.is_stopped() {
            let Some(chunk) = stream.next().await else {
                break;
            };
            let chunk = chunk.map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "unable to read `{model_visible_path}`: {err}"
                ))
            })?;
            scanner.feed(&chunk).map_err(|err| {
                FunctionCallError::RespondToModel(err.model_message(&model_visible_path))
            })?;
        }
        let outcome = scanner.finish().map_err(|err| {
            FunctionCallError::RespondToModel(err.model_message(&model_visible_path))
        })?;

        let text = render_read_output(&outcome, metadata.modified_at_ms, requested_offset);
        Ok(boxed_tool_output(ReadFileOutput { text }))
    }
}
