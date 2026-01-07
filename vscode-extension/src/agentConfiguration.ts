import * as vscode from "vscode";
import {
  getAgentById,
  getCurrentAgentId,
  DEFAULT_AGENT_ID,
} from "./agentRegistry";

/** Extension configuration */
export interface ExtensionConfig {
  id: string;
  enabled: boolean;
}

/** Default extensions when none configured */
const DEFAULT_EXTENSIONS: ExtensionConfig[] = [
  { id: "sparkle", enabled: true },
  { id: "ferris", enabled: true },
  { id: "cargo", enabled: true },
];

/**
 * AgentConfiguration - Identifies a unique agent setup
 *
 * Consists of the agent ID, workspace folder, and enabled extensions.
 * Tabs with the same configuration can share an ACP agent process.
 */

export class AgentConfiguration {
  constructor(
    public readonly agentId: string,
    public readonly workspaceFolder: vscode.WorkspaceFolder,
    public readonly extensions: ExtensionConfig[],
  ) {}

  /**
   * Get a unique key for this configuration.
   * Includes enabled extensions so different extension configs get different agents.
   */
  key(): string {
    const enabledExtensions = this.extensions
      .filter((e) => e.enabled)
      .map((e) => e.id)
      .join(",");
    return `${this.agentId}:${this.workspaceFolder.uri.fsPath}:${enabledExtensions}`;
  }

  /**
   * Check if two configurations are equivalent
   */
  equals(other: AgentConfiguration): boolean {
    return this.key() === other.key();
  }

  /**
   * Get a human-readable description
   */
  describe(): string {
    const agent = getAgentById(this.agentId);
    return agent?.name ?? this.agentId;
  }

  /**
   * Create an AgentConfiguration from current VSCode settings
   * @param workspaceFolder - Optional workspace folder. If not provided, will use the first workspace folder or prompt user if multiple exist.
   */
  static async fromSettings(
    workspaceFolder?: vscode.WorkspaceFolder,
  ): Promise<AgentConfiguration> {
    // Get current agent ID
    const currentAgentId = getCurrentAgentId();

    // Get extensions configuration
    const config = vscode.workspace.getConfiguration("symposium");
    const extensions = config.get<ExtensionConfig[]>(
      "extensions",
      DEFAULT_EXTENSIONS,
    );

    // Determine workspace folder
    let folder = workspaceFolder;
    if (!folder) {
      const folders = vscode.workspace.workspaceFolders;
      if (!folders || folders.length === 0) {
        throw new Error("No workspace folder open");
      } else if (folders.length === 1) {
        folder = folders[0];
      } else {
        // Multiple folders - ask user to choose
        const chosen = await vscode.window.showWorkspaceFolderPick({
          placeHolder: "Select workspace folder for agent",
        });
        if (!chosen) {
          throw new Error("No workspace folder selected");
        }
        folder = chosen;
      }
    }

    return new AgentConfiguration(currentAgentId, folder, extensions);
  }
}
