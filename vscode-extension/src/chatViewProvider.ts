import * as vscode from "vscode";
import { AcpAgentActor } from "./acpAgentActor";

interface IndexedMessage {
  index: number;
  type: string;
  tabId: string;
  text?: string;
}

export class ChatViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "symposium.chatView";
  #view?: vscode.WebviewView;
  #agent: AcpAgentActor;
  #tabToAgentSession: Map<string, string> = new Map(); // tabId → agentSessionId
  #agentSessionToTab: Map<string, string> = new Map(); // agentSessionId → tabId
  #messageQueues: Map<string, IndexedMessage[]> = new Map(); // tabId → queue of unacked messages
  #nextMessageIndex: Map<string, number> = new Map(); // tabId → next index to assign
  #extensionUri: vscode.Uri;
  #extensionActivationId: string;

  constructor(
    extensionUri: vscode.Uri,
    context: vscode.ExtensionContext,
    extensionActivationId: string,
  ) {
    this.#extensionUri = extensionUri;
    this.#extensionActivationId = extensionActivationId;

    // Create agent with callbacks
    this.#agent = new AcpAgentActor({
      onAgentText: (agentSessionId, text) => {
        const tabId = this.#agentSessionToTab.get(agentSessionId);
        if (tabId) {
          this.#sendToWebview({
            type: "agent-text",
            tabId,
            text,
          });
        }
      },
      onAgentComplete: (agentSessionId) => {
        const tabId = this.#agentSessionToTab.get(agentSessionId);
        if (tabId) {
          this.#sendToWebview({
            type: "agent-complete",
            tabId,
          });
        }
      },
    });

    // Get configuration from settings
    const config = vscode.workspace.getConfiguration("symposium");

    // Get conductor command
    const conductorCommand = config.get<string>("conductor", "sacp-conductor");

    // Get components
    const components = config.get<
      Record<string, { command: string; args?: string[]; disabled?: boolean }>
    >("components", {
      "symposium-acp": { command: "symposium-acp", args: [] },
    });

    // Get enabled components (where disabled !== true)
    const enabledComponents = Object.entries(components)
      .filter(([_, component]) => !component.disabled)
      .map(([_, component]) => {
        const cmd = component.command;
        const args = component.args || [];
        return args.length > 0 ? `${cmd} ${args.join(" ")}` : cmd;
      });

    // Get agent configuration
    const agents = config.get<
      Record<
        string,
        { command: string; args?: string[]; env?: Record<string, string> }
      >
    >("agents", {
      ElizACP: { command: "elizacp", args: [], env: {} },
    });
    const currentAgentName = config.get<string>("currentAgent", "ElizACP");

    // Find the current agent configuration
    const currentAgent = agents[currentAgentName];
    if (!currentAgent) {
      vscode.window.showErrorMessage(
        `Agent "${currentAgentName}" not found in configured agents`,
      );
      return;
    }

    // Build the agent command string (command + args)
    const agentCmd = currentAgent.command;
    const agentArgs = currentAgent.args || [];
    const agentCommandStr =
      agentArgs.length > 0 ? `${agentCmd} ${agentArgs.join(" ")}` : agentCmd;

    // Build conductor arguments: agent <component1> <component2> ... <agent-command>
    const conductorArgs = ["agent", ...enabledComponents, agentCommandStr];

    console.log(
      `Initializing conductor: ${conductorCommand} ${conductorArgs.join(" ")}`,
    );

    // Initialize the ACP connection with conductor
    this.#agent
      .initialize(conductorCommand, conductorArgs, currentAgent.env)
      .catch((err) => {
        console.error("Failed to initialize ACP agent:", err);
        vscode.window.showErrorMessage(
          `Failed to initialize agent: ${err.message}`,
        );
      });
  }

  public resolveWebviewView(
    webviewView: vscode.WebviewView,
    context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken,
  ) {
    this.#view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [this.#extensionUri],
    };

    webviewView.webview.html = this.#getHtmlForWebview(webviewView.webview);

    // Handle webview visibility changes
    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        console.log("Webview became visible");
        this.#onWebviewVisible();
      } else {
        console.log("Webview became hidden");
        this.#onWebviewHidden();
      }
    });

    // Handle messages from the webview
    webviewView.webview.onDidReceiveMessage(async (message) => {
      switch (message.type) {
        case "new-tab":
          try {
            // Create a new agent session for this tab
            const agentSessionId = await this.#agent.createSession();
            this.#tabToAgentSession.set(message.tabId, agentSessionId);
            this.#agentSessionToTab.set(agentSessionId, message.tabId);

            // Initialize message tracking for this tab
            this.#messageQueues.set(message.tabId, []);
            this.#nextMessageIndex.set(message.tabId, 0);

            console.log(
              `Created agent session ${agentSessionId} for tab ${message.tabId}`,
            );
          } catch (err) {
            console.error("Failed to create agent session:", err);
          }
          break;

        case "message-ack":
          // Webview acknowledged a message - remove from queue
          this.#handleMessageAck(message.tabId, message.index);
          break;

        case "prompt":
          console.log(`Received prompt for tab ${message.tabId}`);

          // Get the agent session for this tab
          const agentSessionId = this.#tabToAgentSession.get(message.tabId);
          if (!agentSessionId) {
            console.error(`No agent session found for tab ${message.tabId}`);
            return;
          }

          console.log(`Sending prompt to agent session ${agentSessionId}`);

          // Send prompt to agent (responses come via callbacks)
          try {
            await this.#agent.sendPrompt(agentSessionId, message.prompt);
          } catch (err) {
            console.error("Failed to send prompt:", err);
            // TODO: Send error message to webview
          }
          break;

        case "webview-ready":
          // Webview is initialized and ready to receive messages
          console.log("Webview ready - replaying queued messages");
          this.#replayQueuedMessages();
          break;
      }
    });
  }

  #handleMessageAck(tabId: string, ackedIndex: number) {
    const queue = this.#messageQueues.get(tabId);
    if (!queue) {
      return;
    }

    // Remove all messages with index <= ackedIndex
    const remaining = queue.filter((msg) => msg.index > ackedIndex);
    this.#messageQueues.set(tabId, remaining);

    console.log(
      `Acked message ${ackedIndex} for tab ${tabId}, ${remaining.length} messages remain in queue`,
    );
  }

  #replayQueuedMessages() {
    if (!this.#view) {
      return;
    }

    // Replay all queued messages for all tabs
    for (const [tabId, queue] of this.#messageQueues.entries()) {
      for (const message of queue) {
        console.log(`Replaying message ${message.index} for tab ${tabId}`);
        this.#view.webview.postMessage(message);
      }
    }
  }

  #sendToWebview(message: any) {
    if (!this.#view) {
      return;
    }

    const tabId = message.tabId;
    if (!tabId) {
      console.error("Message missing tabId:", message);
      return;
    }

    // Assign index to message
    const index = this.#nextMessageIndex.get(tabId) ?? 0;
    this.#nextMessageIndex.set(tabId, index + 1);

    const indexedMessage: IndexedMessage = {
      index,
      ...message,
    };

    // Add to queue (unacked messages)
    const queue = this.#messageQueues.get(tabId) ?? [];
    queue.push(indexedMessage);
    this.#messageQueues.set(tabId, queue);

    // Send if webview is visible
    if (this.#view.visible) {
      console.log(`Sending message ${index} for tab ${tabId}`);
      this.#view.webview.postMessage(indexedMessage);
    } else {
      console.log(`Queued message ${index} for tab ${tabId} (webview hidden)`);
    }
  }

  #onWebviewVisible() {
    // Visibility change detected - webview will send "webview-ready" when initialized
    console.log("Webview became visible");
  }

  #onWebviewHidden() {
    // Nothing to do - messages stay queued until acked
    console.log("Webview became hidden");
  }

  #getHtmlForWebview(webview: vscode.Webview) {
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.#extensionUri, "out", "webview.js"),
    );

    return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Symposium Chat</title>
    <style>
        body {
            margin: 0;
            padding: 0;
            overflow: hidden;
        }
        #mynah-root {
            width: 100%;
            height: 100vh;
        }
    </style>
</head>
<body>
    <div id="mynah-root"></div>
    <script>
        // Embed extension activation ID so it's available immediately
        window.SYMPOSIUM_EXTENSION_ACTIVATION_ID = "${this.#extensionActivationId}";
    </script>
    <script src="${scriptUri}"></script>
</body>
</html>`;
  }

  dispose() {
    this.#agent.dispose();
  }
}
