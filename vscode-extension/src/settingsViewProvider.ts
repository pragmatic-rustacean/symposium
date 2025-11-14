import * as vscode from "vscode";

export class SettingsViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "symposium.settingsView";
  #view?: vscode.WebviewView;
  #extensionUri: vscode.Uri;

  constructor(extensionUri: vscode.Uri) {
    this.#extensionUri = extensionUri;

    // Listen for configuration changes
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("symposium")) {
        this.#sendConfiguration();
      }
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
        // Refresh configuration when view becomes visible
        this.#sendConfiguration();
      }
    });

    // Handle messages from the webview
    webviewView.webview.onDidReceiveMessage(async (message) => {
      switch (message.type) {
        case "get-config":
          // Send current configuration to webview
          this.#sendConfiguration();
          break;
        case "set-current-agent":
          // Update current agent setting
          const config = vscode.workspace.getConfiguration("symposium");
          await config.update(
            "currentAgent",
            message.agentName,
            vscode.ConfigurationTarget.Global,
          );
          vscode.window.showInformationMessage(
            `Switched to agent: ${message.agentName}`,
          );
          // Send updated configuration to refresh the UI
          this.#sendConfiguration();
          break;
        case "toggle-component":
          // Toggle component enabled/disabled
          await this.#toggleComponent(message.componentName);
          break;
        case "toggle-bypass-permissions":
          // Toggle bypass permissions for an agent
          await this.#toggleBypassPermissions(message.agentName);
          break;
        case "open-settings":
          // Open VSCode settings focused on Symposium
          vscode.commands.executeCommand(
            "workbench.action.openSettings",
            "symposium",
          );
          break;
      }
    });
  }

  async #toggleComponent(componentName: string) {
    const config = vscode.workspace.getConfiguration("symposium");
    const components = config.get<
      Record<string, { command: string; args?: string[]; disabled?: boolean }>
    >("components", {});

    if (components[componentName]) {
      components[componentName].disabled = !components[componentName].disabled;
      await config.update(
        "components",
        components,
        vscode.ConfigurationTarget.Global,
      );
      this.#sendConfiguration();
    }
  }

  async #toggleBypassPermissions(agentName: string) {
    const config = vscode.workspace.getConfiguration("symposium");
    const agents = config.get<Record<string, any>>("agents", {});

    if (agents[agentName]) {
      const currentValue = agents[agentName].bypassPermissions || false;
      agents[agentName].bypassPermissions = !currentValue;
      await config.update("agents", agents, vscode.ConfigurationTarget.Global);
      vscode.window.showInformationMessage(
        `${agentName}: Bypass permissions ${!currentValue ? "enabled" : "disabled"}`,
      );
      this.#sendConfiguration();
    }
  }

  #sendConfiguration() {
    if (!this.#view) {
      return;
    }

    const config = vscode.workspace.getConfiguration("symposium");
    const agents = config.get("agents", {});
    const currentAgent = config.get("currentAgent", "");
    const components = config.get("components", {});

    this.#view.webview.postMessage({
      type: "config",
      agents,
      currentAgent,
      components,
    });
  }

  #getHtmlForWebview(webview: vscode.Webview) {
    return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Symposium Settings</title>
    <style>
        body {
            padding: 16px;
            color: var(--vscode-foreground);
            font-family: var(--vscode-font-family);
            font-size: var(--vscode-font-size);
        }
        h2 {
            margin-top: 0;
            margin-bottom: 16px;
            font-size: 16px;
            font-weight: 600;
        }
        .section {
            margin-bottom: 24px;
        }
        .agent-list, .component-list {
            display: flex;
            flex-direction: column;
            gap: 8px;
        }
        .agent-item, .component-item {
            padding: 8px 12px;
            background: var(--vscode-list-inactiveSelectionBackground);
            border-radius: 4px;
            cursor: pointer;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        .agent-item:hover, .component-item:hover {
            background: var(--vscode-list-hoverBackground);
        }
        .agent-item.active {
            background: var(--vscode-list-activeSelectionBackground);
            color: var(--vscode-list-activeSelectionForeground);
        }
        .component-item.disabled {
            opacity: 0.6;
        }
        .badges {
            display: flex;
            gap: 6px;
            align-items: center;
        }
        .badge {
            padding: 2px 8px;
            border-radius: 12px;
            font-size: 11px;
            background: var(--vscode-badge-background);
            color: var(--vscode-badge-foreground);
        }
        .badge.bypass {
            background: var(--vscode-inputValidation-warningBackground);
            color: var(--vscode-inputValidation-warningForeground);
            cursor: pointer;
        }
        .badge.bypass:hover {
            opacity: 0.8;
        }
        .toggle {
            font-size: 11px;
            color: var(--vscode-descriptionForeground);
        }
    </style>
</head>
<body>
    <div class="section">
        <h2>Current Agent</h2>
        <div class="agent-list" id="agent-list">
            <div>Loading...</div>
        </div>
    </div>

    <div class="section">
        <h2>Components</h2>
        <div class="component-list" id="component-list">
            <div>Loading...</div>
        </div>
    </div>

    <div class="section">
        <a href="#" id="configure-link" style="color: var(--vscode-textLink-foreground); text-decoration: none;">
            Configure agents and components...
        </a>
    </div>

    <script>
        const vscode = acquireVsCodeApi();

        // Request initial configuration
        vscode.postMessage({ type: 'get-config' });

        // Handle configure link
        document.getElementById('configure-link').onclick = (e) => {
            e.preventDefault();
            vscode.postMessage({ type: 'open-settings' });
        };

        // Handle messages from extension
        window.addEventListener('message', event => {
            const message = event.data;

            if (message.type === 'config') {
                renderAgents(message.agents, message.currentAgent);
                renderComponents(message.components);
            }
        });

        function renderAgents(agents, currentAgent) {
            const list = document.getElementById('agent-list');
            list.innerHTML = '';

            for (const [name, config] of Object.entries(agents)) {
                const item = document.createElement('div');
                item.className = 'agent-item' + (name === currentAgent ? ' active' : '');

                const badges = [];
                if (name === currentAgent) {
                    badges.push('<span class="badge">Active</span>');
                }
                if (config.bypassPermissions) {
                    badges.push('<span class="badge bypass" title="Click to disable bypass permissions">Bypass Permissions</span>');
                }

                item.innerHTML = \`
                    <span>\${name}</span>
                    <div class="badges">\${badges.join('')}</div>
                \`;

                // Handle clicking on the agent name (switch agent)
                const nameSpan = item.querySelector('span:first-child');
                nameSpan.onclick = (e) => {
                    e.stopPropagation();
                    vscode.postMessage({ type: 'set-current-agent', agentName: name });
                };

                // Handle clicking on the bypass badge (toggle bypass)
                const bypassBadge = item.querySelector('.badge.bypass');
                if (bypassBadge) {
                    bypassBadge.onclick = (e) => {
                        e.stopPropagation();
                        vscode.postMessage({ type: 'toggle-bypass-permissions', agentName: name });
                    };
                }

                list.appendChild(item);
            }
        }

        function renderComponents(components) {
            const list = document.getElementById('component-list');
            list.innerHTML = '';

            for (const [name, config] of Object.entries(components)) {
                const item = document.createElement('div');
                item.className = 'component-item' + (config.disabled ? ' disabled' : '');
                item.innerHTML = \`
                    <span>\${name}</span>
                    <span class="toggle">\${config.disabled ? 'Disabled' : 'Enabled'}</span>
                \`;
                item.onclick = () => {
                    vscode.postMessage({ type: 'toggle-component', componentName: name });
                };
                list.appendChild(item);
            }
        }
    </script>
</body>
</html>`;
  }
}
