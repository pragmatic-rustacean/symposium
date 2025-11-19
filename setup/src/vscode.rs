//! VSCode extension build and installation

use anyhow::{Context, Result, anyhow};
use std::path::Path;
use std::process::Command;

/// Build and install the VSCode extension
pub fn build_and_install_extension(repo_root: &Path, dry_run: bool) -> Result<()> {
    let extension_dir = repo_root.join("vscode-extension");

    if !extension_dir.exists() {
        return Err(anyhow!(
            "❌ VSCode extension directory not found at: {}",
            extension_dir.display()
        ));
    }

    println!("📦 Building VSCode extension...");

    if dry_run {
        println!("   Would install dependencies (npm install)");
        println!("   Would build extension (npm run webpack-dev)");
        println!("   Would package extension (npx vsce package)");
        println!("   Would install extension (code --install-extension)");
    } else {
        // Install dependencies
        install_dependencies(&extension_dir)?;

        // Build extension
        build_extension(&extension_dir)?;

        // Package extension
        package_extension(&extension_dir)?;

        // Find and install the .vsix file
        install_extension(&extension_dir)?;

        println!("✅ VSCode extension installed successfully!");
    }
    Ok(())
}

/// Install npm dependencies
fn install_dependencies(extension_dir: &Path) -> Result<()> {
    println!("📥 Installing extension dependencies...");

    let output = Command::new("npm")
        .args(["install"])
        .current_dir(extension_dir)
        .output()
        .context("Failed to execute npm install")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "❌ Failed to install extension dependencies:\n   Error: {}",
            stderr.trim()
        ));
    }

    Ok(())
}

/// Build the extension
fn build_extension(extension_dir: &Path) -> Result<()> {
    println!("🔨 Building extension...");

    let output = Command::new("npm")
        .args(["run", "webpack-dev"])
        .current_dir(extension_dir)
        .output()
        .context("Failed to execute npm run webpack-dev")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "❌ Failed to build extension:\n   Error: {}",
            stderr.trim()
        ));
    }

    Ok(())
}

/// Package the extension as .vsix
fn package_extension(extension_dir: &Path) -> Result<()> {
    println!("📦 Packaging VSCode extension...");

    let output = Command::new("npx")
        .args(["vsce", "package", "--no-dependencies"])
        .current_dir(extension_dir)
        .output()
        .context("Failed to execute vsce package")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "❌ Failed to package extension:\n   Error: {}",
            stderr.trim()
        ));
    }

    Ok(())
}

/// Install the packaged extension
fn install_extension(extension_dir: &Path) -> Result<()> {
    // Find the generated .vsix file
    let vsix_file = find_vsix_file(extension_dir)?;

    println!("📥 Installing VSCode extension: {}", vsix_file);

    let output = Command::new("code")
        .args(["--install-extension", &vsix_file])
        .current_dir(extension_dir)
        .output()
        .context("Failed to execute code --install-extension")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "❌ Failed to install VSCode extension:\n   Error: {}",
            stderr.trim()
        ));
    }

    Ok(())
}

/// Find the .vsix file in the extension directory
fn find_vsix_file(extension_dir: &Path) -> Result<String> {
    let entries = std::fs::read_dir(extension_dir).context("Failed to read extension directory")?;

    for entry in entries {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();
        if let Some(extension) = path.extension() {
            if extension == "vsix" {
                return Ok(path.file_name().unwrap().to_string_lossy().to_string());
            }
        }
    }

    Err(anyhow!("❌ No .vsix file found after packaging"))
}

/// Check if VSCode is available
pub fn check_vscode_available() -> Result<()> {
    if which::which("code").is_err() {
        return Err(anyhow!(
            "❌ VSCode 'code' command not found.\n   Please install VSCode and ensure the 'code' command is available.\n   Visit: https://code.visualstudio.com/"
        ));
    }
    Ok(())
}

/// Check if Node.js/npm is available
pub fn check_node_available() -> Result<()> {
    if which::which("npm").is_err() {
        return Err(anyhow!(
            "❌ npm not found.\n   Please install Node.js first.\n   Visit: https://nodejs.org/"
        ));
    }
    Ok(())
}
