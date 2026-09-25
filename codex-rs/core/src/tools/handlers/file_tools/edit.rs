//! Sandboxed adapter for the `edit_file` tool.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::Arc;

use codex_apply_patch::ApplyPatchFileChange;
use codex_exec_server::GetMetadataOptions;
use codex_exec_server::ReadFileOptions;
use codex_mycode_file_tools::edit::EditRequest;
use codex_mycode_file_tools::edit::plan_edit;
use codex_mycode_file_tools::spec::create_edit_file_tool;
use codex_protocol::protocol::FileChange;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ApplyPatchToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::apply_patch::apply_patch_file_update_mode;
use crate::tools::handlers::file_tools::runtime::FileMutation;
use crate::tools::handlers::file_tools::runtime::FileMutationRequest;
use crate::tools::handlers::file_tools::runtime::prepare_file_mutation_approval;
use crate::tools::handlers::file_tools::runtime::run_file_mutation;
use crate::tools::handlers::parse_arguments;
use crate::tools::handlers::resolve_tool_environment;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use crate::tools::sandboxing::ToolCtx;
use codex_tools::ToolName;
use codex_tools::ToolSpec;

const MAX_EDIT_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Handles `edit_file` calls against the selected turn environment filesystem.
#[derive(Default)]
pub struct EditFileHandler;

#[derive(Deserialize)]
struct EditFileArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

impl ToolExecutor<ToolInvocation> for EditFileHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("edit_file")
    }

    fn spec(&self) -> ToolSpec {
        create_edit_file_tool()
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(self.handle_call(invocation))
    }
}

impl CoreToolRuntime for EditFileHandler {}

impl EditFileHandler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            step_context,
            tracker,
            cancellation_token,
            call_id,
            tool_name,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "edit_file handler received unsupported payload".to_string(),
                ));
            }
        };
        let EditFileArgs {
            file_path,
            old_string,
            new_string,
            replace_all,
        } = parse_arguments(&arguments)?;

        if file_path.to_ascii_lowercase().ends_with(".ipynb") {
            return Err(FunctionCallError::RespondToModel(
                "edit_file does not support notebook (.ipynb) files; use apply_patch instead"
                    .to_string(),
            ));
        }

        let Some(turn_environment) = resolve_tool_environment(&step_context.environments, None)?
        else {
            return Err(FunctionCallError::RespondToModel(
                "edit_file is unavailable in this session".to_string(),
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
                "`{model_visible_path}` is a directory, not a file"
            )));
        }
        if !metadata.is_file {
            return Err(FunctionCallError::RespondToModel(format!(
                "`{model_visible_path}` is not a regular file"
            )));
        }
        if metadata.size > MAX_EDIT_FILE_BYTES {
            return Err(FunctionCallError::RespondToModel(format!(
                "`{model_visible_path}` is too large to edit ({} bytes; limit {MAX_EDIT_FILE_BYTES} bytes)",
                metadata.size
            )));
        }

        let original = fs
            .read_file_text(&path_uri, ReadFileOptions::default(), Some(&sandbox))
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "unable to read `{model_visible_path}`: {err}"
                ))
            })?;
        let plan = plan_edit(
            &original,
            &EditRequest {
                old_string,
                new_string,
                replace_all,
            },
            apply_patch_file_update_mode(&step_context.turn),
        )
        .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;

        let apply_patch_changes = Arc::new(HashMap::from([(
            path_uri.clone(),
            ApplyPatchFileChange::Update {
                unified_diff: plan.unified_diff.clone(),
                move_path: None,
                new_content: plan.updated.clone(),
            },
        )]));
        let protocol_changes = Arc::new(HashMap::from([(
            path_uri.to_path_buf(),
            FileChange::Update {
                unified_diff: plan.unified_diff.clone(),
                move_path: None,
            },
        )]));
        let (auto_approved, exec_approval_requirement) = prepare_file_mutation_approval(
            apply_patch_changes.as_ref(),
            turn_environment,
            &step_context,
        )?;

        let tool_ctx = ToolCtx {
            session,
            step_context: Arc::clone(&step_context),
            cancellation_token,
            call_id,
            tool_name,
        };
        let request = FileMutationRequest {
            turn_environment: turn_environment.clone(),
            mutation: FileMutation::Edit {
                path: path_uri.clone(),
                old_content: original,
                new_content: plan.updated,
            },
            protocol_changes,
            file_paths: vec![path_uri],
            approval_patch: plan.unified_diff,
            exec_approval_requirement,
            additional_permissions: None,
            permissions_preapproved: false,
        };
        run_file_mutation(request, tool_ctx, Some(&tracker), auto_approved).await?;
        Ok(boxed_tool_output(ApplyPatchToolOutput::from_text(
            plan.message,
        )))
    }
}
