import * as vscode from "vscode";

/**
 * AgentConfiguration - Identifies a unique agent setup
 *
 * Consists of the base agent name and enabled component names.
 * Tabs with the same configuration can share an ACP agent process.
 */

export class AgentConfiguration {
  constructor(
    public readonly agentName: string,
    public readonly components: string[],
  ) {
    // Sort components for consistent comparison
    this.components = [...components].sort();
  }

  /**
   * Get a unique key for this configuration
   */
  key(): string {
    return `${this.agentName}:${this.components.join(",")}`;
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
    if (this.components.length === 0) {
      return this.agentName;
    }
    return `${this.agentName} + ${this.components.length} component${this.components.length > 1 ? "s" : ""}`;
  }

  /**
   * Create an AgentConfiguration from current VSCode settings
   */
  static fromSettings(): AgentConfiguration {
    const config = vscode.workspace.getConfiguration("symposium");

    // Get current agent
    const currentAgentName = config.get<string>("currentAgent", "ElizACP");

    // Get enabled components
    const components = config.get<
      Record<string, { command: string; args?: string[]; disabled?: boolean }>
    >("components", {});

    const enabledComponents = Object.keys(components).filter(
      (name) => !components[name].disabled,
    );

    return new AgentConfiguration(currentAgentName, enabledComponents);
  }
}
