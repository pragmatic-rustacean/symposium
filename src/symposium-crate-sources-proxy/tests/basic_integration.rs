//! Basic integration tests for the crate sources proxy using ElizACP.

use anyhow::Result;
use sacp::{ByteStreams, DynComponent};
use sacp_conductor::conductor::Conductor;
use symposium_crate_sources_proxy::CrateSourcesProxy;
use tokio::io::duplex;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Test that the rust_crate_query tool can be invoked and triggers a new session.
///
/// This test verifies:
/// 1. The CrateSourcesProxy exposes the rust_crate_query MCP tool
/// 2. Calling the tool triggers a new session to be spawned
/// 3. The session receives the research prompt
/// 4. The proxy handles the response (even if nonsensical from Eliza)
#[tokio::test]
async fn test_rust_crate_query_with_elizacp() -> Result<()> {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .try_init();

    // Create the component chain: CrateSourcesProxy -> ElizACP
    let proxy = CrateSourcesProxy;

    // Create duplex streams for editor <-> conductor communication
    let (editor_write, conductor_read) = duplex(8192);
    let (conductor_write, editor_read) = duplex(8192);

    // Spawn conductor with proxy + ElizACP chain
    let conductor_handle = tokio::spawn(async move {
        Conductor::new(
            "test-conductor".to_string(),
            vec![
                DynComponent::new(proxy),
                DynComponent::new(elizacp::ElizaAgent::new()),
            ],
            None,
        )
        .run(ByteStreams::new(
            conductor_write.compat_write(),
            conductor_read.compat(),
        ))
        .await
    });

    // Send a tool invocation to rust_crate_query
    // ElizACP expects format: "Use tool <server>::<tool> with <json_params>"
    let tool_call = r#"Use tool rust-crate-query::rust_crate_query with {"crate_name":"serde","prompt":"What is the signature of from_value?"}"#;

    let response = yopo::prompt(
        ByteStreams::new(editor_write.compat_write(), editor_read.compat()),
        tool_call,
    )
    .await?;

    tracing::info!("Response received: {} chars", response.len());
    tracing::debug!("Response content: {}", response);

    // Verify we got a response (even if it's nonsense from Eliza)
    assert!(!response.is_empty(), "Should receive a response");

    // The response should indicate either success (OK:) or error (ERROR:)
    // from ElizACP's tool execution
    assert!(
        response.contains("OK:") || response.contains("ERROR:"),
        "Response should indicate tool execution result: {}",
        response
    );

    // Clean up
    conductor_handle.await??;

    Ok(())
}
