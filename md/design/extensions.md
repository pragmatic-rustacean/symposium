# Agent Extensions

Agent extensions are proxy components that enrich an agent's capabilities. They sit between the editor and the agent, adding tools, context, and behaviors.

## Built-in Extensions

| ID | Name | Description |
|----|------|-------------|
| `sparkle` | Sparkle | AI collaboration identity and embodiment |
| `ferris` | Ferris | Rust development tools (crate sources, rust researcher) |
| `cargo` | Cargo | Cargo build and run tools |

## Extension Sources

Extensions can come from multiple sources:

- **built-in**: Bundled with Symposium (sparkle, ferris, cargo)
- **registry**: Installed from the [shared agent registry](https://github.com/agentclientprotocol/registry)
- **custom**: User-defined via executable, npx, pipx, cargo, or URL

## Distribution Types

Extensions use the same distribution types as agents (see [Agent Registry](./agent-registry.md)):

- `local` - executable command on the system
- `npx` - npm package
- `pipx` - Python package  
- `cargo` - Rust crate from crates.io
- `binary` - platform-specific archive download

## Configuration

Extensions are passed to `symposium-acp-agent` via `--proxy` arguments:

```bash
symposium-acp-agent run-with --proxy sparkle --proxy ferris --proxy cargo --agent '...'
```

**Order matters** - extensions are applied in the order listed. The first extension is closest to the editor, and the last is closest to the agent.

The special value `defaults` expands to all known built-in extensions:

```bash
--proxy defaults  # equivalent to: --proxy sparkle --proxy ferris --proxy cargo
```

## Registry Format

The shared registry includes both agents and extensions:

```json
{
  "date": "2026-01-07",
  "agents": [...],
  "extensions": [
    {
      "id": "some-extension",
      "name": "Some Extension",
      "version": "1.0.0",
      "description": "Does something useful",
      "distribution": {
        "npx": { "package": "@example/some-extension" }
      }
    }
  ]
}
```

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Editor Extension (VSCode, Zed, etc.)           │
│  - Manages extension configuration              │
│  - Builds --proxy args for agent spawn          │
└─────────────────┬───────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────┐
│  symposium-acp-agent                            │
│  - Parses --proxy arguments                     │
│  - Resolves extension distributions             │
│  - Builds proxy chain in order                  │
│  - Conductor orchestrates the chain             │
└─────────────────────────────────────────────────┘
```

## Future Work

- **Per-extension configuration**: Add sub-options for extensions (e.g., which Ferris tools to enable)
- **Extension updates**: Check for and apply updates to registry-sourced extensions
- **Cargo.toml discovery**: Auto-discover extensions from dependency metadata
