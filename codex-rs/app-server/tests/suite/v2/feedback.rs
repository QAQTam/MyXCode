use std::time::Duration;

use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::RequestId;
use serde_json::json;
use tokio::time::timeout;

#[tokio::test]
async fn feedback_upload_is_refused_without_touching_the_network() -> Result<()> {
    // MyCode: feedback uploads to the built-in Sentry project are disabled, so the
    // request fails before any network work starts. Upstream covered the upload
    // concurrency limit here; that path no longer exists in this build.
    let mut app_server = TestAppServer::builder().build_initialized().await?;

    let request_id = app_server
        .send_raw_request(
            "feedback/upload",
            Some(json!({ "classification": "bug", "includeLogs": false })),
        )
        .await?;
    let error = timeout(
        Duration::from_secs(/*secs*/ 15),
        app_server.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.error.code, -32603);
    assert!(
        error
            .error
            .message
            .contains("feedback upload is disabled in this build"),
        "unexpected error: {}",
        error.error.message
    );
    Ok(())
}
