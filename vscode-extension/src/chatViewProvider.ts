import * as vscode from "vscode";
import { AcpAgentActor } from "./acpAgentActor";
import { AgentConfiguration } from "./agentConfiguration";

interface IndexedMessage {
  index: number;
  type: string;
  tabId: string;
  text?: string;
}

export class ChatViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "symposium.chatView";
  #view?: vscode.WebviewView;
  #configToActor: Map<string, AcpAgentActor> = new Map(); // config.key() → AcpAgentActor
  #tabToConfig: Map<string, AgentConfiguration> = new Map(); // tabId → AgentConfiguration
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
  }

  /**
   * Get or create an ACP actor for the given configuration.
   * Actors are shared across tabs with the same configuration.
   */
  async #getOrCreateActor(config: AgentConfiguration): Promise<AcpAgentActor> {
    const key = config.key();

    // Return existing actor if we have one for this config
    const existing = this.#configToActor.get(key);
    if (existing) {
      return existing;
    }

    // Create a new actor with callbacks
    const actor = new AcpAgentActor({
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

    // Initialize the actor
    await actor.initialize(config);

    // Store it in the map
    this.#configToActor.set(key, actor);

    return actor;
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
            // Get the current agent configuration from settings
            const config = AgentConfiguration.fromSettings();

            // Store the configuration for this tab
            this.#tabToConfig.set(message.tabId, config);

            // Initialize message tracking for this tab
            this.#messageQueues.set(message.tabId, []);
            this.#nextMessageIndex.set(message.tabId, 0);

            // Update tab title immediately (before spawning agent)
            this.#sendToWebview({
              type: "set-tab-title",
              tabId: message.tabId,
              title: config.agentName,
            });

            // Get or create an actor for this configuration (may spawn process)
            const actor = await this.#getOrCreateActor(config);

            // Create a new agent session for this tab
            const agentSessionId = await actor.createSession();
            this.#tabToAgentSession.set(message.tabId, agentSessionId);
            this.#agentSessionToTab.set(agentSessionId, message.tabId);

            console.log(
              `Created agent session ${agentSessionId} for tab ${message.tabId} using ${config.describe()}`,
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

          // Get the configuration and actor for this tab
          const tabConfig = this.#tabToConfig.get(message.tabId);
          if (!tabConfig) {
            console.error(`No configuration found for tab ${message.tabId}`);
            return;
          }

          const tabActor = this.#configToActor.get(tabConfig.key());
          if (!tabActor) {
            console.error(
              `No actor found for configuration ${tabConfig.key()}`,
            );
            return;
          }

          console.log(`Sending prompt to agent session ${agentSessionId}`);

          // Send prompt to agent (responses come via callbacks)
          try {
            await tabActor.sendPrompt(agentSessionId, message.prompt);
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
    // Dispose all actors
    for (const actor of this.#configToActor.values()) {
      actor.dispose();
    }
    this.#configToActor.clear();
  }
}
