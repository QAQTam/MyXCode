use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use codex_apply_patch::AppliedPatchChange;
use codex_apply_patch::AppliedPatchDelta;
use codex_apply_patch::AppliedPatchFileChange;
use codex_exec_server::CreateDirectoryOptions;
use codex_exec_server::ExecutorFileSystem;
use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::GetMetadataOptions;
use codex_exec_server::WriteFileOptions;
use codex_protocol::error::CodexErr;
use codex_protocol::error::SandboxErr;
use codex_protocol::exec_output::ExecToolCallOutput;
use codex_protocol::exec_output::StreamOutput;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::FileChange;
use codex_sandboxing::SandboxType;
use codex_sandboxing::SandboxablePreference;
use codex_sandboxing::is_likely_executor_managed_sandbox_denied;
use codex_sandboxing::policy_transforms::effective_permission_profile;
use codex_sandboxing::record_filesystem_sandbox_violation;
use codex_utils_path_uri::PathUri;

use crate::exec::is_likely_sandbox_denied;
use crate::function_tool::FunctionCallError;
use crate::safety::PatchSandboxRoute;
use crate::safety::SafetyCheck;
use crate::safety::assess_file_mutation_safety;
use crate::session::step_context::StepContext;
use crate::session::turn_context::TurnEnvironment;
use crate::tools::context::SharedTurnDiffTracker;
use crate::tools::events::ToolEmitter;
use crate::tools::events::ToolEventCtx;
use crate::tools::handlers::file_system_sandbox_policy_context_for_cwd;
use crate::tools::orchestrator::ToolOrchestrator;
use crate::tools::sandboxing::Approvable;
use crate::tools::sandboxing::ApprovalAction;
use crate::tools::sandboxing::ExecApprovalRequirement;
use crate::tools::sandboxing::SandboxAttempt;
use crate::tools::sandboxing::Sandboxable;
use crate::tools::sandboxing::ToolCtx;
use crate::tools::sandboxing::ToolError;
use crate::tools::sandboxing::ToolRuntime;
use crate::tools::sandboxing::executor_windows_sandbox_selection;
use crate::windows_sandbox::windows_sandbox_level_for_legacy_checks;

#[derive(Debug)]
pub(super) enum FileMutation {
    Edit {
        path: PathUri,
        old_content: String,
        new_content: String,
    },
    Create {
        path: PathUri,
        content: String,
    },
}

#[derive(Debug)]
pub(super) struct FileMutationRequest {
    pub turn_environment: TurnEnvironment,
    pub mutation: FileMutation,
    pub protocol_changes: Arc<HashMap<PathBuf, FileChange>>,
    pub file_paths: Vec<PathUri>,
    pub approval_patch: String,
    pub exec_approval_requirement: ExecApprovalRequirement,
    pub additional_permissions: Option<AdditionalPermissionProfile>,
    pub permissions_preapproved: bool,
}

#[derive(Default)]
pub(super) struct FileMutationRuntime {
    committed_delta: AppliedPatchDelta,
}

pub(super) struct FileMutationRuntimeOutput {
    pub exec_output: ExecToolCallOutput,
    pub delta: AppliedPatchDelta,
}

impl FileMutationRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn committed_delta(&self) -> &AppliedPatchDelta {
        &self.committed_delta
    }

    fn build_approval_action(req: &FileMutationRequest, call_id: &str) -> ApprovalAction {
        ApprovalAction::ApplyPatch {
            id: call_id.to_string(),
            environment_id: req.turn_environment.selection.environment_id.clone(),
            cwd: req.turn_environment.cwd().clone(),
            files: req.file_paths.clone(),
            patch: req.approval_patch.clone(),
            changes: Arc::clone(&req.protocol_changes),
            permissions_preapproved: req.permissions_preapproved,
        }
    }

    fn file_system_sandbox_context_for_attempt(
        req: &FileMutationRequest,
        attempt: &SandboxAttempt<'_>,
    ) -> Option<FileSystemSandboxContext> {
        if !attempt.sandbox_requested {
            return None;
        }

        let permissions = effective_permission_profile(
            attempt.exec_server_permissions,
            req.additional_permissions.as_ref(),
        );
        Some(FileSystemSandboxContext {
            permissions,
            cwd: attempt.sandbox_cwd.clone(),
            workspace_roots: attempt.workspace_roots.to_vec(),
            user_home_dir: req.turn_environment.user_home_dir.clone(),
            temporary_directories: None,
            windows_sandbox_selection: executor_windows_sandbox_selection(
                attempt.windows_sandbox_type,
                attempt.windows_sandbox_level,
                attempt.sandbox_cwd,
            ),
            windows_sandbox_proxy_settings_mode: None,
            use_legacy_landlock: attempt.use_legacy_landlock,
        })
    }

    async fn apply_mutation(
        req: &FileMutationRequest,
        fs: &dyn ExecutorFileSystem,
        sandbox: Option<&FileSystemSandboxContext>,
    ) -> io::Result<()> {
        match &req.mutation {
            FileMutation::Edit {
                path, new_content, ..
            } => {
                fs.write_file(
                    path,
                    new_content.as_bytes().to_vec(),
                    WriteFileOptions::default(),
                    sandbox,
                )
                .await
            }
            FileMutation::Create { path, content } => {
                match fs
                    .get_metadata(path, GetMetadataOptions::default(), sandbox)
                    .await
                {
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            format!(
                                "`{}` already exists; write_file creates new files only",
                                path.inferred_native_path_string()
                            ),
                        ));
                    }
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err),
                }

                if let Some(parent) = path.parent() {
                    fs.create_directory(
                        &parent,
                        CreateDirectoryOptions {
                            recursive: true,
                            follow_symlinks: true,
                        },
                        sandbox,
                    )
                    .await?;
                }
                fs.write_file(
                    path,
                    content.as_bytes().to_vec(),
                    WriteFileOptions::default(),
                    sandbox,
                )
                .await
            }
        }
    }
}

pub(super) fn prepare_file_mutation_approval(
    changes: &HashMap<PathUri, codex_apply_patch::ApplyPatchFileChange>,
    turn_environment: &TurnEnvironment,
    step_context: &StepContext,
) -> Result<(bool, ExecApprovalRequirement), FunctionCallError> {
    let sandbox_context = turn_environment.sandbox_context(/*additional_permissions*/ None);
    let policy_context =
        file_system_sandbox_policy_context_for_cwd(&sandbox_context, turn_environment.cwd());
    let sandbox_route = if turn_environment.environment.is_remote() {
        PatchSandboxRoute::ExecutorManaged
    } else {
        PatchSandboxRoute::Platform(windows_sandbox_level_for_legacy_checks(
            turn_environment.config().windows_sandbox_type,
            turn_environment.config().windows_sandbox_level,
        ))
    };
    let file_system_sandbox_policy = turn_environment
        .permission_profile()
        .file_system_sandbox_policy();
    let matching = sandbox_route
        .prepare_matching(&file_system_sandbox_policy, &policy_context)
        .map_err(|error| {
            FunctionCallError::RespondToModel(format!(
                "failed to prepare file mutation permissions: {error}"
            ))
        })?;
    let safety_check = assess_file_mutation_safety(
        changes,
        step_context.settings.approval_policy(),
        turn_environment.permission_profile(),
        &matching,
    )
    .map_err(|error| {
        FunctionCallError::RespondToModel(format!("failed to check file mutation safety: {error}"))
    })?;
    match safety_check {
        SafetyCheck::AutoApprove => Ok((
            true,
            ExecApprovalRequirement::Skip {
                bypass_sandbox: false,
                proposed_execpolicy_amendment: None,
            },
        )),
        SafetyCheck::AskUser => Ok((
            false,
            ExecApprovalRequirement::NeedsApproval {
                reason: None,
                proposed_execpolicy_amendment: None,
            },
        )),
        SafetyCheck::Reject { reason } => Err(FunctionCallError::RespondToModel(format!(
            "file mutation rejected: {reason}"
        ))),
    }
}

pub(super) async fn run_file_mutation(
    request: FileMutationRequest,
    tool_ctx: ToolCtx,
    tracker: Option<&SharedTurnDiffTracker>,
    auto_approved: bool,
) -> Result<(), FunctionCallError> {
    let changes = (*request.protocol_changes).clone();
    let emitter = ToolEmitter::apply_patch_for_environment(
        tool_ctx.tool_name.name.clone(),
        changes,
        auto_approved,
        request.turn_environment.selection.environment_id.clone(),
    );
    let event_ctx = ToolEventCtx::new(
        tool_ctx.session.as_ref(),
        tool_ctx.step_context.turn.as_ref(),
        &tool_ctx.step_context.settings.model_info,
        &tool_ctx.call_id,
        tracker,
    );
    emitter.begin(event_ctx).await;

    let mut orchestrator = ToolOrchestrator::new();
    let mut runtime = FileMutationRuntime::new();
    let result = orchestrator.run(&mut runtime, &request, &tool_ctx).await;
    let (result, delta) = match result {
        Ok(output) => (Ok(output.output.exec_output), Some(output.output.delta)),
        Err(error) => (Err(error), Some(runtime.committed_delta().clone())),
    };
    let event_ctx = ToolEventCtx::new(
        tool_ctx.session.as_ref(),
        tool_ctx.step_context.turn.as_ref(),
        &tool_ctx.step_context.settings.model_info,
        &tool_ctx.call_id,
        tracker,
    );
    emitter.finish(event_ctx, result, delta.as_ref()).await?;
    Ok(())
}

impl Sandboxable for FileMutationRuntime {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }

    fn escalate_on_failure(&self) -> bool {
        true
    }
}

impl Approvable<FileMutationRequest> for FileMutationRuntime {
    fn approval_action(
        &self,
        req: &FileMutationRequest,
        call_id: &str,
    ) -> io::Result<ApprovalAction> {
        Ok(Self::build_approval_action(req, call_id))
    }

    fn wants_no_sandbox_approval(&self, policy: AskForApproval) -> bool {
        match policy {
            AskForApproval::Never => false,
            AskForApproval::Granular(granular_config) => granular_config.allows_sandbox_approval(),
            AskForApproval::OnRequest => true,
            AskForApproval::UnlessTrusted => true,
        }
    }

    fn exec_approval_requirement(
        &self,
        req: &FileMutationRequest,
    ) -> Option<ExecApprovalRequirement> {
        Some(req.exec_approval_requirement.clone())
    }
}

impl ToolRuntime<FileMutationRequest, FileMutationRuntimeOutput> for FileMutationRuntime {
    fn turn_environment<'a>(&self, req: &'a FileMutationRequest) -> &'a TurnEnvironment {
        &req.turn_environment
    }

    fn uses_executor_managed_process_sandbox(&self, req: &FileMutationRequest) -> bool {
        req.turn_environment.environment.is_remote()
    }

    fn sandbox_cwd<'a>(&self, req: &'a FileMutationRequest) -> Option<&'a PathUri> {
        Some(req.turn_environment.cwd())
    }

    async fn run(
        &mut self,
        req: &FileMutationRequest,
        attempt: &SandboxAttempt<'_>,
        _ctx: &ToolCtx,
    ) -> Result<FileMutationRuntimeOutput, ToolError> {
        let started_at = Instant::now();
        let fs = req.turn_environment.environment.get_filesystem();
        let sandbox = Self::file_system_sandbox_context_for_attempt(req, attempt);
        match Self::apply_mutation(req, fs.as_ref(), sandbox.as_ref()).await {
            Ok(()) => {
                let delta = delta_for_mutation(&req.mutation);
                self.committed_delta.append(delta);
                Ok(FileMutationRuntimeOutput {
                    exec_output: success_output(started_at),
                    delta: self.committed_delta.clone(),
                })
            }
            Err(err) => {
                let output = io_error_output(&err, started_at);
                let sandbox_denied = if attempt.sandbox == SandboxType::None {
                    attempt.sandbox_requested && is_likely_executor_managed_sandbox_denied(&output)
                } else {
                    is_likely_sandbox_denied(attempt.sandbox, &output)
                };
                if sandbox_denied {
                    if attempt.sandbox != SandboxType::None {
                        record_filesystem_sandbox_violation(attempt.sandbox, &output);
                    }
                    Err(ToolError::Codex(CodexErr::Sandbox(SandboxErr::Denied {
                        output: Box::new(output),
                        network_policy_decision: None,
                    })))
                } else {
                    Err(ToolError::Codex(CodexErr::Io(err)))
                }
            }
        }
    }
}

fn delta_for_mutation(mutation: &FileMutation) -> AppliedPatchDelta {
    let change = match mutation {
        FileMutation::Edit {
            path,
            old_content,
            new_content,
            ..
        } => AppliedPatchChange {
            path: path.clone(),
            change: AppliedPatchFileChange::Update {
                move_path: None,
                old_content: old_content.clone(),
                overwritten_move_content: None,
                new_content: new_content.clone(),
            },
        },
        FileMutation::Create { path, content } => AppliedPatchChange {
            path: path.clone(),
            change: AppliedPatchFileChange::Add {
                content: content.clone(),
                overwritten_content: None,
            },
        },
    };
    AppliedPatchDelta::new(vec![change], /*exact*/ true)
}

fn success_output(started_at: Instant) -> ExecToolCallOutput {
    ExecToolCallOutput {
        exit_code: 0,
        stdout: StreamOutput::new(String::new()),
        stderr: StreamOutput::new(String::new()),
        aggregated_output: StreamOutput::new(String::new()),
        duration: started_at.elapsed(),
        timed_out: false,
    }
}

fn io_error_output(err: &io::Error, started_at: Instant) -> ExecToolCallOutput {
    let message = err.to_string();
    ExecToolCallOutput {
        exit_code: 1,
        stdout: StreamOutput::new(String::new()),
        stderr: StreamOutput::new(message.clone()),
        aggregated_output: StreamOutput::new(message),
        duration: started_at.elapsed(),
        timed_out: false,
    }
}
