/**
 * Agent Registry - Types and built-in agent definitions
 *
 * Supports multiple distribution methods (npx, pipx, binary) and
 * merges built-in agents with user-configured agents from settings.
 */

import * as vscode from "vscode";
import * as os from "os";
import * as path from "path";

/**
 * Distribution methods for spawning an agent
 */
export interface NpxDistribution {
  package: string;
  args?: string[];
}

export interface PipxDistribution {
  package: string;
  args?: string[];
}

export interface BinaryDistribution {
  url: string;
  executable: string;
  args?: string[];
}

export interface Distribution {
  npx?: NpxDistribution;
  pipx?: PipxDistribution;
  binary?: Record<string, BinaryDistribution>; // keyed by platform, e.g., "darwin-aarch64"
}

/**
 * Agent configuration - matches registry format
 */
export interface AgentConfig {
  id: string;
  distribution: Distribution;
  name?: string;
  version?: string;
  description?: string;
  _source?: "registry" | "custom";
}

/**
 * Settings format - object keyed by agent id (id is implicit in key)
 */
export type AgentSettingsEntry = Omit<AgentConfig, "id">;
export type AgentSettings = Record<string, AgentSettingsEntry>;

/**
 * Built-in agents - these are always available unless overridden in settings
 */
export const BUILT_IN_AGENTS: AgentConfig[] = [
  {
    id: "zed-claude-code",
    name: "Claude Code",
    distribution: {
      npx: { package: "@zed-industries/claude-code-acp@latest" },
    },
    _source: "custom",
  },
  {
    id: "zed-codex",
    name: "Codex",
    distribution: {
      npx: { package: "@zed-industries/codex-acp@latest" },
    },
    _source: "custom",
  },
  {
    id: "google-gemini",
    name: "Gemini",
    distribution: {
      npx: {
        package: "@google/gemini-cli@latest",
        args: ["--experimental-acp"],
      },
    },
    _source: "custom",
  },
];

/**
 * Default agent ID when none is selected
 */
export const DEFAULT_AGENT_ID = "zed-claude-code";

/**
 * Get the current platform key for binary distribution lookup
 */
export function getPlatformKey(): string {
  const platform = process.platform;
  const arch = process.arch;

  const platformMap: Record<string, Record<string, string>> = {
    darwin: {
      arm64: "darwin-aarch64",
      x64: "darwin-x86_64",
    },
    linux: {
      x64: "linux-x86_64",
      arm64: "linux-aarch64",
    },
    win32: {
      x64: "windows-x86_64",
    },
  };

  return platformMap[platform]?.[arch] ?? `${platform}-${arch}`;
}

/**
 * Get the cache directory for binary agents
 */
export function getBinaryCacheDir(agentId: string, version: string): string {
  return path.join(os.homedir(), ".symposium", "bin", agentId, version);
}

/**
 * Merge built-in agents with user settings.
 * Settings entries override built-ins with the same id.
 */
export function getEffectiveAgents(): AgentConfig[] {
  const config = vscode.workspace.getConfiguration("symposium");
  const settingsAgents = config.get<AgentSettings>("agents", {});

  // Start with built-ins
  const agentsById = new Map<string, AgentConfig>();
  for (const agent of BUILT_IN_AGENTS) {
    agentsById.set(agent.id, agent);
  }

  // Override/add from settings
  for (const [id, entry] of Object.entries(settingsAgents)) {
    agentsById.set(id, { id, ...entry });
  }

  return Array.from(agentsById.values());
}

/**
 * Get a specific agent by ID
 */
export function getAgentById(id: string): AgentConfig | undefined {
  const agents = getEffectiveAgents();
  return agents.find((a) => a.id === id);
}

/**
 * Get the currently selected agent ID from settings
 */
export function getCurrentAgentId(): string {
  const config = vscode.workspace.getConfiguration("symposium");
  return config.get<string>("currentAgentId", DEFAULT_AGENT_ID);
}

/**
 * Get the currently selected agent config
 */
export function getCurrentAgent(): AgentConfig | undefined {
  return getAgentById(getCurrentAgentId());
}

/**
 * Resolved spawn command
 */
export interface ResolvedCommand {
  command: string;
  args: string[];
  env?: Record<string, string>;
}

/**
 * Resolve an agent's distribution to a spawn command.
 * Priority: npx > pipx > binary
 *
 * @throws Error if no compatible distribution is found
 */
export async function resolveDistribution(
  agent: AgentConfig,
): Promise<ResolvedCommand> {
  const dist = agent.distribution;

  // Try npx first
  if (dist.npx) {
    return {
      command: "npx",
      args: ["-y", dist.npx.package, ...(dist.npx.args ?? [])],
    };
  }

  // Try pipx
  if (dist.pipx) {
    return {
      command: "pipx",
      args: ["run", dist.pipx.package, ...(dist.pipx.args ?? [])],
    };
  }

  // Try binary for current platform
  if (dist.binary) {
    const platformKey = getPlatformKey();
    const binaryDist = dist.binary[platformKey];

    if (binaryDist) {
      const version = agent.version ?? "latest";
      const cacheDir = getBinaryCacheDir(agent.id, version);
      const executablePath = path.join(cacheDir, binaryDist.executable);

      // Check if binary exists in cache
      const fs = await import("fs/promises");
      try {
        await fs.access(executablePath);
      } catch {
        // Binary not cached - need to download
        await downloadAndCacheBinary(agent, binaryDist, cacheDir);
      }

      return {
        command: executablePath,
        args: binaryDist.args ?? [],
      };
    }
  }

  throw new Error(
    `No compatible distribution found for agent "${agent.id}" on platform ${getPlatformKey()}`,
  );
}

/**
 * Download and cache a binary distribution
 */
async function downloadAndCacheBinary(
  agent: AgentConfig,
  binaryDist: BinaryDistribution,
  cacheDir: string,
): Promise<void> {
  const fs = await import("fs/promises");

  // Clean up old versions first
  const parentDir = path.dirname(cacheDir);
  try {
    const entries = await fs.readdir(parentDir);
    for (const entry of entries) {
      const entryPath = path.join(parentDir, entry);
      if (entryPath !== cacheDir) {
        await fs.rm(entryPath, { recursive: true, force: true });
      }
    }
  } catch {
    // Parent directory doesn't exist yet, that's fine
  }

  // Create cache directory
  await fs.mkdir(cacheDir, { recursive: true });

  // Download the binary
  const response = await fetch(binaryDist.url);
  if (!response.ok) {
    throw new Error(
      `Failed to download binary for ${agent.id}: ${response.status} ${response.statusText}`,
    );
  }

  const buffer = await response.arrayBuffer();
  const url = new URL(binaryDist.url);
  const filename = path.basename(url.pathname);
  const downloadPath = path.join(cacheDir, filename);

  await fs.writeFile(downloadPath, Buffer.from(buffer));

  // Extract if it's an archive
  if (
    filename.endsWith(".tar.gz") ||
    filename.endsWith(".tgz") ||
    filename.endsWith(".zip")
  ) {
    await extractArchive(downloadPath, cacheDir);
    // Remove the archive after extraction
    await fs.unlink(downloadPath);
  }

  // Make executable on Unix
  if (process.platform !== "win32") {
    const executablePath = path.join(cacheDir, binaryDist.executable);
    await fs.chmod(executablePath, 0o755);
  }
}

/**
 * Extract an archive to a directory
 */
async function extractArchive(
  archivePath: string,
  destDir: string,
): Promise<void> {
  const { exec } = await import("child_process");
  const { promisify } = await import("util");
  const execAsync = promisify(exec);

  if (archivePath.endsWith(".zip")) {
    if (process.platform === "win32") {
      await execAsync(
        `powershell -command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}'"`,
      );
    } else {
      await execAsync(`unzip -o "${archivePath}" -d "${destDir}"`);
    }
  } else {
    // tar.gz or tgz
    await execAsync(`tar -xzf "${archivePath}" -C "${destDir}"`);
  }
}
