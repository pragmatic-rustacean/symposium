//! Symposium Ferris - Rust development tools as an ACP agent extension

use anyhow::Result;
use sacp::Component;
use symposium_ferris::FerrisComponent;

#[tokio::main]
async fn main() -> Result<()> {
    FerrisComponent::default()
        .serve(sacp_tokio::Stdio::new())
        .await?;
    Ok(())
}
