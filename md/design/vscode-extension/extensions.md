# Extension UI (VSCode)

This chapter covers VSCode-specific UI for managing extensions. For general extension concepts, see [Agent Extensions](../extensions.md).

## Configuration Storage

Extensions are configured via the `symposium.extensions` VS Code setting:

```json
"symposium.extensions": [
  { "id": "sparkle", "_enabled": true, "_source": "built-in" },
  { "id": "ferris", "_enabled": true, "_source": "built-in" },
  { "id": "cargo", "_enabled": true, "_source": "built-in" }
]
```

Custom extensions include their distribution:

```json
{
  "id": "my-extension",
  "_enabled": true,
  "_source": "custom",
  "name": "My Extension",
  "distribution": {
    "npx": { "package": "@myorg/my-extension" }
  }
}
```

**Default behavior** - when no setting exists, all built-in extensions are enabled. If the user returns to the default configuration, the key is removed from settings.json entirely.

## Settings UI

The Settings panel includes an Extensions section where users can:

- **Enable/disable** extensions via checkbox
- **Reorder** extensions by dragging the handle
- **Delete** extensions from the list
- **Add** extensions via the "+ Add extension" link, which opens a QuickPick dialog

### Add Extension Dialog

The QuickPick dialog shows three sections:

1. **Built-in** - sparkle, ferris, cargo (greyed out if already added)
2. **From Registry** - extensions from the shared registry with `type: "extension"`
3. **Add Custom Extension**:
   - From executable on your system (local command/path)
   - From npx package
   - From pipx package
   - From cargo crate
   - From URL to extension.json (GitHub URLs auto-converted to raw)

## Spawn Integration

When spawning an agent, the extension builds `--proxy` arguments from enabled extensions:

```bash
symposium-acp-agent run-with --proxy sparkle --proxy ferris --proxy cargo --agent '...'
```

Only enabled extensions are passed, in their configured order.
