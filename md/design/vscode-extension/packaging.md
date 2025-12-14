# Extension Packaging

This chapter documents how the VSCode extension is built and packaged for distribution.

## Overview

The extension packaging involves several steps:

1. **Build the Rust binary** (`symposium-acp-agent`) for the target platform(s)
2. **Build the TypeScript/webpack bundle** (extension code + webview)
3. **Package as `.vsix`** using `vsce`

## Directory Structure

```
vscode-extension/
├── bin/                          # Bundled binaries (gitignored)
│   └── darwin-arm64/             # Platform-specific directories
│       └── symposium-acp-agent   # The conductor binary
├── out/                          # Compiled JS output (gitignored)
│   ├── extension.js              # Main extension bundle
│   └── webview.js                # Webview bundle
├── src/                          # TypeScript source
├── vendor/                       # -> ../vendor (symlink or path)
├── package.json
├── webpack.config.js
├── .vscodeignore                 # Files to exclude from .vsix
└── symposium-0.0.1.vsix          # Packaged extension (gitignored)
```

## Build Steps

### 1. Build the Rust Binary

The `symposium-acp-agent` binary must be compiled for each target platform and placed in `bin/<platform>-<arch>/`:

```bash
# For local development (current platform only)
cargo build --release -p symposium-acp-agent

# Copy to the expected location
mkdir -p vscode-extension/bin/darwin-arm64
cp target/release/symposium-acp-agent vscode-extension/bin/darwin-arm64/
```

Platform directory names follow Node.js conventions:
- `darwin-arm64` (macOS Apple Silicon)
- `darwin-x64` (macOS Intel)
- `linux-x64` (Linux x86_64)
- `win32-x64` (Windows x86_64)

### 2. Build the Vendored mynah-ui

The extension uses a vendored fork of mynah-ui. It must be built before the extension:

```bash
cd vendor/mynah-ui
npm ci
npm run build
```

### 3. Build the Extension

The extension uses webpack to bundle the TypeScript code:

```bash
cd vscode-extension
npm ci
npm run webpack  # Production build
```

This produces two bundles:
- `out/extension.js` - The main extension (Node.js target)
- `out/webview.js` - The webview code (browser target)

### 4. Package as .vsix

Use `vsce` to create the installable package:

```bash
cd vscode-extension
npx vsce package
```

This creates `symposium-<version>.vsix`.

## Binary Resolution at Runtime

The extension looks for the conductor binary in this order (see `binaryPath.ts`):

1. **Bundled binary**: `<extensionPath>/bin/<platform>-<arch>/symposium-acp-agent`
2. **Simple layout**: `<extensionPath>/bin/symposium-acp-agent` (for single-platform dev)
3. **PATH fallback**: Just `symposium-acp-agent` (development mode)

This allows development without bundling binaries - just `cargo install` the binary and it will be found in PATH.

## .vscodeignore

The `.vscodeignore` file controls what goes into the `.vsix`:

```
.vscode/**
.vscode-test/**
src/**
.gitignore
tsconfig.json
**/*.map
**/*.ts
```

Currently missing entries that should be added:
- `../vendor/**` - The vendored mynah-ui source (only the built webview.js is needed)
- `node_modules/**` - Should be excluded since webpack bundles dependencies

## Multi-Platform Distribution

For marketplace distribution, the extension needs binaries for all platforms. Options:

### Option A: Platform-Specific Extensions

Publish separate extensions for each platform:
- `symposium-darwin-arm64`
- `symposium-darwin-x64`
- `symposium-linux-x64`
- `symposium-win32-x64`

### Option B: Universal Extension with Download

Ship without binaries and download on first activation. This is more complex but results in smaller initial download.

### Option C: Universal Extension with All Binaries

Bundle all platform binaries in a single extension. Simple but large (~70MB+ for all platforms).

## CI/CD Considerations

For automated builds:

1. The CI workflow needs to build mynah-ui before the extension
2. Cross-compilation of Rust binaries requires appropriate toolchains
3. Each platform's binary should be built on that platform (or cross-compiled)

Current CI builds mynah-ui in the `vscode-extension` job:

```yaml
- name: Build vendored mynah-ui
  working-directory: vendor/mynah-ui
  run: |
    npm ci
    npm run build
```

## Local Development

For local development without packaging:

```bash
# Install the conductor globally
cargo install --path src/symposium-acp-agent

# Build the extension
cd vscode-extension
npm ci
npm run compile  # or npm run watch

# Run in VSCode
# Press F5 to launch Extension Development Host
```

The extension will find `symposium-acp-agent` in PATH when no bundled binary exists.
