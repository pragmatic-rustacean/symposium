# Distribution

This chapter documents how Symposium is released and distributed across platforms.

## Release Orchestration

Releases are triggered by [release-plz](https://release-plz.dev/), which:

1. Creates a release PR when changes accumulate on `main`
2. When merged, publishes to crates.io and creates GitHub releases with tags

The `symposium-acp-agent-v*` tag triggers the binary release workflow.

## Distribution Channels

```
release-plz creates tag
        ↓
┌───────────────────────────────────────┐
│         GitHub Release                │
│  - Binary archives (all platforms)    │
│  - VSCode .vsix files                 │
│  - Source reference                   │
└───────────────────────────────────────┘
        ↓
┌─────────────┬─────────────┬───────────┐
│  crates.io  │   VSCode    │    Zed    │
│             │ Marketplace │Extensions │
│             │ + Open VSX  │           │
└─────────────┴─────────────┴───────────┘
```

### crates.io

The Rust crates are published directly by release-plz. Users can install via:
```bash
cargo install symposium-acp-agent
```

### VSCode Marketplace / Open VSX

Platform-specific extensions are built and published automatically. Each platform gets its own ~7MB extension containing only that platform's binary.

See [VSCode Packaging](./vscode-extension/packaging.md) for details.

### Zed Extensions

The Zed extension (`zed-extension/`) points to GitHub release archives. Publishing requires submitting a PR to the [zed-industries/extensions](https://github.com/zed-industries/extensions) repository.

### Direct Download

Binary archives are attached to each GitHub release for direct download:
- `symposium-darwin-arm64.tar.gz`
- `symposium-darwin-x64.tar.gz`
- `symposium-linux-x64.tar.gz`
- `symposium-linux-arm64.tar.gz`
- `symposium-linux-x64-musl.tar.gz`
- `symposium-windows-x64.zip`

## Supported Platforms

| Platform | Architecture | Notes |
|----------|--------------|-------|
| macOS | arm64 (Apple Silicon) | Primary development platform |
| macOS | x64 (Intel) | |
| Linux | x64 (glibc) | Standard Linux distributions |
| Linux | arm64 | ARM servers, Raspberry Pi |
| Linux | x64 (musl) | Static binary, Alpine Linux |
| Windows | x64 | |

## Secrets Required

The release workflow requires these GitHub secrets:

| Secret | Purpose |
|--------|---------|
| `RELEASE_PLZ_TOKEN` | GitHub token for release-plz to create releases |
| `VSCE_PAT` | Azure DevOps PAT for VSCode Marketplace |
| `OVSX_PAT` | Open VSX access token |
