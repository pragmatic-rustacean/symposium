# Language Model Tool Bridging

This chapter describes how Symposium bridges tool calls between VS Code's Language Model API and ACP agents. There are two categories of tools that need different handling:

1. **VS Code-provided tools** - Tools that VS Code extensions offer to the model
2. **Agent-internal tools** - Tools the ACP agent manages internally (via its own MCP servers)

## Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         VS Code                                 │
│                                                                 │
│  Consumer (Copilot, etc.)                                      │
│    │                                                            │
│    │ options.tools[] ─────────────────┐                        │
│    │                                   │                        │
│    ▼                                   ▼                        │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │        LanguageModelChatProvider (TypeScript)            │   │
│  │                                                          │   │
│  │  Emits: LanguageModelToolCallPart                       │   │
│  │    - For VS Code tools (agent invoked via MCP)          │   │
│  │    - For symposium-agent-action (permission requests)    │   │
│  └──────────────────────────┬───────────────────────────────┘   │
└─────────────────────────────┼───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│              Symposium vscodelm (Rust)                          │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    Session Actor                          │  │
│  │                                                           │  │
│  │  - Manages ACP session lifecycle                         │  │
│  │  - Runs synthetic MCP server for VS Code tools           │  │
│  │  - Merges streams: updates, permissions, MCP calls       │  │
│  │  - Handles history matching for session continuity       │  │
│  └───────────────────────────────────────────────────────────┘  │
│                              │                                   │
│                              ▼                                   │
│                        ACP Agent                                 │
│                  (with internal MCP servers)                     │
└─────────────────────────────────────────────────────────────────┘
```

## VS Code-Provided Tools

VS Code consumers pass tools to the model via `options.tools[]` in each request. These are tools implemented by VS Code or its extensions (e.g., "search workspace", "read file").

### Tool Format from VS Code

```typescript
interface LanguageModelChatTool {
  name: string;
  description: string;
  inputSchema: object;  // JSON Schema
}
```

This is a flat list with no server grouping - just name, description, and schema.

### Bridging to ACP

ACP agents discover tools by connecting to MCP servers at session creation. To expose VS Code-provided tools to an ACP agent, Symposium creates a **synthetic MCP server** that:

1. Offers the same tools that VS Code provided in `options.tools[]`
2. When the agent invokes a tool, blocks and shuttles the call back to VS Code
3. Returns the result from VS Code to the agent

### Execution Flow

```
1. VS Code request arrives with options.tools[]
2. Symposium creates/updates synthetic MCP server with those tools
3. Agent decides to use a tool, invokes it via MCP
4. Symposium emits LanguageModelToolCallPart to VS Code
5. Symposium returns from provideLanguageModelChatResponse
6. VS Code consumer calls invokeTool(), shows confirmation UI
7. User approves → tool executes → result available
8. Next VS Code request arrives with tool result in message history
9. Symposium extracts result, returns it to agent via MCP
10. Agent continues with tool result
```

## Agent-Internal Tools

ACP agents have their own MCP servers providing tools (e.g., bash execution, file editing). The agent can execute these directly, but may request permission first via ACP's `session/request_permission`.

### Permission Flow

When an agent requests permission, Symposium surfaces this to VS Code using a special tool called `symposium-agent-action`:

```
1. Agent sends session/request_permission (e.g., "run bash: rm foo.txt")
2. Symposium emits LanguageModelToolCallPart for symposium-agent-action
3. prepareInvocation() formats the request for VS Code's confirmation UI
4. Symposium returns from provideLanguageModelChatResponse
5. VS Code shows confirmation dialog to user
6. User approves → invoke() called → returns "proceed"
7. Next VS Code request arrives with tool result
8. Symposium responds to agent with allow-once
9. Agent proceeds to execute the tool internally
```

### Permission Auto-Approval

If the agent asks permission before executing, Symposium always responds with approval at the ACP level. The actual user-facing permission check happens via VS Code's tool confirmation UI. This means:

- Agent asks permission → Symposium says "yes" at ACP level
- Agent executes tool → if it's VS Code-provided, goes through VS Code UI
- If user rejects in VS Code UI → cancellation propagates back

## Handle States

The TypeScript handle tracks state across VS Code requests:

```
┌─────────────────────────────────────────────────────────────────┐
│  Idle                                                            │
│  ────                                                            │
│  - No active prompt                                              │
│  - Waiting for user message                                      │
└─────────────────────────────────────────────────────────────────┘
        │
        │ [prompt arrives]
        ▼
┌─────────────────────────────────────────────────────────────────┐
│  Streaming                                                       │
│  ─────────                                                       │
│  - Pulling from agent update stream                             │
│  - Forwarding text chunks to VS Code                            │
│  - Holding: updates_rx                                          │
└─────────────────────────────────────────────────────────────────┘
        │
        ├──[agent done]──► Idle
        │
        ├──[permission request]──┐
        │                        ▼
        │   ┌─────────────────────────────────────────────────────┐
        │   │  AwaitingToolPermission                             │
        │   │  ──────────────────────                             │
        │   │  - Emitted symposium-agent-action tool call         │
        │   │  - Holding:                                         │
        │   │    - updates_rx: agent update stream                │
        │   │    - permission_tx: oneshot for decision            │
        │   │    - original_history: committed history            │
        │   │    - provisional_history: original + tool call      │
        │   └─────────────────────────────────────────────────────┘
        │
        └──[MCP tool invocation]──┐
                                  ▼
           ┌─────────────────────────────────────────────────────┐
           │  AwaitingToolResult                                  │
           │  ─────────────────                                   │
           │  - Emitted VS Code tool call                        │
           │  - Holding:                                          │
           │    - updates_rx: agent update stream                 │
           │    - result_tx: oneshot for tool result              │
           │    - original_history: committed history             │
           │    - provisional_history: original + tool call       │
           └─────────────────────────────────────────────────────┘
```

## History Matching

When a request arrives while in `AwaitingToolPermission` or `AwaitingToolResult`, Symposium compares the incoming message history:

### Case 1: Extends Provisional History

The incoming history includes the tool call and result:
```
original_history + [assistant: tool_call] + [user: tool_result]
```

This means the tool was approved. Symposium:
1. Sends result via the oneshot channel
2. Promotes provisional history to committed
3. Resumes streaming

### Case 2: Extends Only Original History

The incoming history doesn't include the tool call:
```
original_history + [user: new_message]
```

This means the tool was rejected (or user sent something else). Symposium:
1. Drops the oneshot channel (signals cancellation)
2. Reverts to original history
3. Processes the new prompt from that state

## Cancellation Handling

Cancellation can occur in several ways:

1. **User rejects tool in VS Code UI** - `CancellationError` thrown, token cancelled
2. **User cancels entire chat** - Same signal, indistinguishable
3. **User sends different message** - History doesn't match provisional

All cases are handled uniformly by detecting that the incoming history doesn't extend provisional history.

### Cancellation Flow

```
1. Cancellation detected (token or history mismatch)
2. If awaiting permission:
   - Send reject-once to agent via oneshot
   - Send session/cancel to agent
3. If awaiting tool result:
   - Drop result_tx (causes MCP call to error)
   - Send session/cancel to agent
4. Return to Idle state
5. Await next prompt
```

## Buffering Agent Activity

ACP agents work asynchronously and may generate output between VS Code requests. Symposium handles this by:

1. Merging all agent events into a single stream:
   - `session/update` notifications (text, tool progress)
   - `session/request_permission` requests
   - MCP tool invocations

2. Buffering events that arrive while no prompt is active

3. Draining the buffer when the next prompt arrives

This ensures no agent output is lost even though VS Code's API is request/response based.

## Session Actor Architecture

The Rust session actor manages the merged event stream:

```rust
enum SessionEvent {
    AgentUpdate(SessionUpdate),
    PermissionRequest {
        request: RequestPermission,
        response_tx: oneshot::Sender<PermissionResponse>,
    },
    McpToolInvocation {
        tool_name: String,
        input: serde_json::Value,
        result_tx: oneshot::Sender<ToolResult>,
    },
}
```

The actor processes events based on current state:

- **Has active prompt**: Forward updates to VS Code, pause on blocking events
- **Prompt cancelled**: Deny/error pending operations, cancel agent
- **No active prompt**: Buffer events for next request

## Limitations

### VS Code Tool Rejection Cancels Entire Chat

When a user rejects a tool in VS Code's confirmation UI, the entire chat is cancelled - not just that tool invocation. This is a VS Code limitation ([GitHub #241039](https://github.com/microsoft/vscode/issues/241039)).

Symposium handles this by:
1. Detecting cancellation
2. Propagating rejection to agent
3. Awaiting next prompt (which starts fresh)

### No Per-Tool Rejection Signaling

VS Code doesn't tell the model that a tool was rejected - the cancelled turn simply doesn't appear in history. The model has no memory of what it tried.

### Tool Approval Levels Managed by VS Code

VS Code manages approval persistence (single use, session, workspace, always). Symposium just receives `invoke()` calls - it doesn't know or care about the approval level.
