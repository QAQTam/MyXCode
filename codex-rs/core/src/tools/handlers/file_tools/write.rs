//! Sandboxed adapter for the create-only `write_file` tool.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::Arc;

use codex_apply_patch::ApplyPatchFileChange;
use codex_exec_server::GetMetadataOptions;
use codex_mycode_file_tools::spec::create_write_file_tool;
use codex_mycode_file_tools::write::WritePlan;
use codex_protocol::protocol::FileChange;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ApplyPatchToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
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

/// Handles `write_file` calls against the selected turn environment filesystem.
#[derive(Default)]
pub struct WriteFileHandler;

#[derive(Deserialize)]
struct WriteFileArgs {
    file_path: String,
    #[serde(default)]
    content: String,
}

impl ToolExecutor<ToolInvocation> for WriteFileHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("write_file")
    }

    fn spec(&self) -> ToolSpec {
        create_write_file_tool()
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(self.handle_call(invocation))
    }
}

impl CoreToolRuntime for WriteFileHandler {}

impl WriteFileHandler {
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
                    "write_file handler received unsupported payload".to_string(),
                ));
            }
        };
        let WriteFileArgs { file_path, content } = parse_arguments(&arguments)?;
        let plan = WritePlan { content };

        let Some(turn_environment) = resolve_tool_environment(&step_context.environments, None)?
        else {
            return Err(FunctionCallError::RespondToModel(
                "write_file is unavailable in this session".to_string(),
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
        match fs
            .get_metadata(&path_uri, GetMetadataOptions::default(), Some(&sandbox))
            .await
        {
            Ok(metadata) if metadata.is_directory => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "`{model_visible_path}` is a directory; write_file cannot replace a directory"
                )));
            }
            Ok(_) => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "`{model_visible_path}` already exists. write_file creates new files only; it never overwrites. Use edit_file for a targeted change, or apply_patch for a full rewrite."
                )));
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "unable to access `{model_visible_path}`: {err}"
                )));
            }
        }

        let apply_patch_changes = Arc::new(HashMap::from([(
            path_uri.clone(),
            ApplyPatchFileChange::Add {
                content: plan.content.clone(),
            },
        )]));
        let protocol_changes = Arc::new(HashMap::from([(
            path_uri.to_path_buf(),
            FileChange::Add {
                content: plan.content.clone(),
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
        let approval_patch = format!(
            "*** Begin Patch\n*** Add File: {model_visible_path}\n+{}\n*** End Patch",
            plan.content.replace('\n', "\n+")
        );
        let request = FileMutationRequest {
            turn_environment: turn_environment.clone(),
            mutation: FileMutation::Create {
                path: path_uri.clone(),
                content: plan.content,
            },
            protocol_changes,
            file_paths: vec![path_uri],
            approval_patch,
            exec_approval_requirement,
            additional_permissions: None,
            permissions_preapproved: false,
        };
        run_file_mutation(request, tool_ctx, Some(&tracker), auto_approved).await?;
        Ok(boxed_tool_output(ApplyPatchToolOutput::from_text(format!(
            "File created successfully at: {model_visible_path}"
        ))))
    }
}
