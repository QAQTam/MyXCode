use super::*;
use crate::config::PermissionProfileSnapshot;
use crate::environment_selection::TurnEnvironmentState;
use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context_with_dynamic_tools_and_rx;
use crate::session::turn_context::TurnEnvironment;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::turn_diff_tracker::TurnDiffTracker;
use codex_protocol::items::FileChangeItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::FileChange;
use codex_tools::ToolExecutor;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

fn replace_primary_environment_cwd(turn: &mut crate::TurnContext, cwd: AbsolutePathBuf) {
    let mut current = turn
        .initial_environments
        .turn_environments()
        .next()
        .cloned()
        .expect("default local turn environment");
    current.config_mut().workspace_roots.clear();
    let mut selection = current.selection;
    selection.cwd = PathUri::from_abs_path(&cwd);
    selection.workspace_roots.clear();
    turn.initial_environments.environments[0] = TurnEnvironmentState::Ready(TurnEnvironment::new(
        selection,
        current.config_origin,
        current.environment,
        current.shell,
    ));
}

async fn unsandboxed_turn_at(
    dir: &std::path::Path,
) -> (
    Arc<Session>,
    Arc<crate::TurnContext>,
    async_channel::Receiver<Event>,
) {
    let (session, mut turn, rx_event) =
        make_session_and_context_with_dynamic_tools_and_rx(Vec::new()).await;
    {
        let turn_ref = Arc::get_mut(&mut turn).expect("turn should be uniquely owned");
        replace_primary_environment_cwd(
            turn_ref,
            AbsolutePathBuf::from_absolute_path(dir).expect("absolute test dir"),
        );
        Arc::make_mut(&mut turn_ref.config)
            .permissions
            .set_permission_profile(PermissionProfile::Disabled)
            .expect("set thread permission profile");
        let TurnEnvironmentState::Ready(environment) =
            &mut turn_ref.initial_environments.environments[0]
        else {
            panic!("primary environment should be ready");
        };
        environment.config_mut().permission_profile =
            PermissionProfileSnapshot::legacy(PermissionProfile::Disabled);
    }
    (session, turn, rx_event)
}

async fn run_read(
    session: Arc<Session>,
    turn: Arc<crate::TurnContext>,
    arguments: serde_json::Value,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    ReadFileHandler
        .handle(ToolInvocation {
            session,
            step_context: StepContext::for_test(Arc::clone(&turn)),
            turn,
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            tracker: Arc::new(Mutex::new(TurnDiffTracker::new())),
            call_id: "call-read".to_string(),
            tool_name: codex_tools::ToolName::plain("read_file"),
            source: ToolCallSource::Direct,
            payload: ToolPayload::Function {
                arguments: arguments.to_string(),
            },
        })
        .await
}

async fn run_edit(
    session: Arc<Session>,
    turn: Arc<crate::TurnContext>,
    arguments: serde_json::Value,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    EditFileHandler
        .handle(ToolInvocation {
            session,
            step_context: StepContext::for_test(Arc::clone(&turn)),
            turn,
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            tracker: Arc::new(Mutex::new(TurnDiffTracker::new())),
            call_id: "call-edit".to_string(),
            tool_name: codex_tools::ToolName::plain("edit_file"),
            source: ToolCallSource::Direct,
            payload: ToolPayload::Function {
                arguments: arguments.to_string(),
            },
        })
        .await
}

async fn run_write(
    session: Arc<Session>,
    turn: Arc<crate::TurnContext>,
    arguments: serde_json::Value,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    WriteFileHandler
        .handle(ToolInvocation {
            session,
            step_context: StepContext::for_test(Arc::clone(&turn)),
            turn,
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            tracker: Arc::new(Mutex::new(TurnDiffTracker::new())),
            call_id: "call-write".to_string(),
            tool_name: codex_tools::ToolName::plain("write_file"),
            source: ToolCallSource::Direct,
            payload: ToolPayload::Function {
                arguments: arguments.to_string(),
            },
        })
        .await
}

fn model_error(result: Result<Box<dyn ToolOutput>, FunctionCallError>) -> String {
    match result {
        Err(FunctionCallError::RespondToModel(message)) => message,
        Err(other) => panic!("expected a model-facing error, got {other:?}"),
        Ok(output) => panic!("expected the call to fail, got {}", output.log_output()),
    }
}

async fn completed_file_change(rx_event: &async_channel::Receiver<Event>) -> FileChangeItem {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), rx_event.recv())
            .await
            .expect("file change event")
            .expect("channel open");
        if let EventMsg::ItemCompleted(completed) = event.msg
            && let TurnItem::FileChange(item) = completed.item
            && item.status.is_some()
        {
            return item;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn read_handler_returns_line_numbered_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("sample.txt"), "alpha\nbeta\n").expect("seed file");
    let (session, turn, _rx_event) = unsandboxed_turn_at(dir.path()).await;

    let output = run_read(
        session,
        turn,
        json!({ "file_path": "sample.txt", "offset": 1, "limit": 1 }),
    )
    .await
    .expect("read should succeed");

    let output = output.log_output();
    assert!(output.contains("     1→alpha"), "{output}");
    assert!(output.contains("offset=2"), "{output}");
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_handler_rewrites_and_emits_file_change() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("sample.txt");
    std::fs::write(&file, "alpha\nbeta\n").expect("seed file");
    let (session, turn, rx_event) = unsandboxed_turn_at(dir.path()).await;

    let output = run_edit(
        session,
        turn,
        json!({
            "file_path": "sample.txt",
            "old_string": "beta",
            "new_string": "BETA",
        }),
    )
    .await
    .expect("edit should succeed");

    assert_eq!(output.log_output(), "The file has been updated.");
    assert_eq!(
        std::fs::read_to_string(&file).expect("read file"),
        "alpha\nBETA\n"
    );

    let item = completed_file_change(&rx_event).await;
    assert_eq!(item.id, "call-edit");
    let change = item
        .changes
        .get(&file)
        .unwrap_or_else(|| panic!("expected a change for {}", file.display()));
    let FileChange::Update { unified_diff, .. } = change else {
        panic!("expected an update change, got {change:?}");
    };
    assert!(unified_diff.contains("-beta"), "{unified_diff}");
    assert!(unified_diff.contains("+BETA"), "{unified_diff}");
}

#[tokio::test(flavor = "multi_thread")]
async fn write_handler_creates_and_refuses_to_overwrite() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("nested").join("new.txt");
    let (session, turn, rx_event) = unsandboxed_turn_at(dir.path()).await;

    let output = run_write(
        Arc::clone(&session),
        Arc::clone(&turn),
        json!({ "file_path": "nested/new.txt", "content": "hello\n" }),
    )
    .await
    .expect("write should succeed");
    assert!(output.log_output().starts_with("File created successfully"));
    assert_eq!(
        std::fs::read_to_string(&file).expect("read file"),
        "hello\n"
    );

    let item = completed_file_change(&rx_event).await;
    assert_eq!(item.id, "call-write");
    let change = item
        .changes
        .get(&file)
        .unwrap_or_else(|| panic!("expected a change for {}", file.display()));
    let FileChange::Add { content } = change else {
        panic!("expected an add change, got {change:?}");
    };
    assert_eq!(content, "hello\n");

    let error = run_write(
        session,
        turn,
        json!({ "file_path": "nested/new.txt", "content": "replaced\n" }),
    )
    .await;
    assert!(model_error(error).contains("already exists"));
    assert_eq!(
        std::fs::read_to_string(&file).expect("read file"),
        "hello\n"
    );
}
