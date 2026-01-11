import { defineConfig } from "@vscode/test-cli";

export default defineConfig({
  files: "out/test/**/*.test.js",
  // Pin to specific version for reproducible tests and better caching
  version: "1.108.0",
  workspaceFolder: "./test-workspace",
  mocha: {
    ui: "tdd",
    timeout: 20000,
  },
});
