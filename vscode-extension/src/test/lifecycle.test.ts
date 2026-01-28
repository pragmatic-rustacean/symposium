import * as assert from "assert";
import * as vscode from "vscode";
import { logger } from "../extension";
import { LogEvent } from "../logger";

function dumpLogs(logEvents: LogEvent[], label: string) {
  console.log(`\n=== ${label} - All log events (${logEvents.length}) ===`);
  for (const event of logEvents) {
    console.log(
      `[${event.category}] ${event.message}`,
      event.data ? JSON.stringify(event.data) : "",
    );
  }
  console.log(`=== End ${label} ===\n`);
}

suite("Webview Lifecycle Tests", () => {
  test("Chat view should persist tabs across hide/show", async function () {
    // This test may need more time for webview operations and agent spawning
    this.timeout(20000);

    // Capture log events
    const logEvents: LogEvent[] = [];
    const logDisposable = logger.onLog((event) => {
      logEvents.push(event);
      // Also log in real-time for CI visibility
      console.log(
        `[LOG ${event.category}] ${event.message}`,
        event.data ? JSON.stringify(event.data) : "",
      );
    });

    // Activate the extension
    const extension = vscode.extensions.getExtension("symposium-dev.symposium");
    assert.ok(extension);
    await extension.activate();

    // Show the chat view (open activity bar item)
    await vscode.commands.executeCommand("symposium.chatView.focus");

    // Give webview time to initialize
    await new Promise((resolve) => setTimeout(resolve, 1000));

    // Simulate creating a tab (this would normally come from the webview)
    console.log("Creating test tab...");
    await vscode.commands.executeCommand(
      "symposium.test.simulateNewTab",
      "test-tab-1",
    );

    // Wait for session to be created (poll instead of fixed wait)
    console.log(
      `SYMPOSIUM_CONFIG_DIR: ${process.env.SYMPOSIUM_CONFIG_DIR || "(not set)"}`,
    );
    const startTime = Date.now();
    const maxWaitMs = 10000;
    let gotSession = false;
    while (Date.now() - startTime < maxWaitMs) {
      const sessions = logEvents.filter(
        (e) => e.category === "agent" && e.message === "Agent session created",
      );
      if (sessions.length > 0) {
        gotSession = true;
        console.log(`Session created after ${Date.now() - startTime}ms`);
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    if (!gotSession) {
      dumpLogs(logEvents, "No session created");
    }

    // Verify the tab was created
    let tabs = (await vscode.commands.executeCommand(
      "symposium.test.getTabs",
    )) as string[];
    console.log(`Tabs after creation: ${tabs}`);
    assert.ok(tabs.includes("test-tab-1"), "Tab should exist after creation");

    // Close the view by switching to Explorer (this should dispose the webview)
    console.log("Hiding chat view by switching to Explorer...");
    await vscode.commands.executeCommand("workbench.view.explorer");
    await new Promise((resolve) => setTimeout(resolve, 1000));

    // Reopen the chat view
    console.log("Reopening chat view...");
    await vscode.commands.executeCommand("symposium.chatView.focus");

    // Give webview time to restore
    await new Promise((resolve) => setTimeout(resolve, 1000));

    // Verify the tab still exists after reopening
    tabs = (await vscode.commands.executeCommand(
      "symposium.test.getTabs",
    )) as string[];
    console.log(`Tabs after reopen: ${tabs}`);
    assert.ok(
      tabs.includes("test-tab-1"),
      "Tab should persist after view hide/show",
    );

    // Clean up
    logDisposable.dispose();

    // Assert on log events to verify lifecycle
    const webviewCreated = logEvents.filter(
      (e) =>
        e.category === "webview" &&
        e.message === "Webview resolved and created",
    );
    const webviewHidden = logEvents.filter(
      (e) => e.category === "webview" && e.message === "Webview became hidden",
    );
    const webviewVisible = logEvents.filter(
      (e) => e.category === "webview" && e.message === "Webview became visible",
    );
    const agentSpawned = logEvents.filter(
      (e) => e.category === "agent" && e.message === "Spawning new agent actor",
    );
    const agentReused = logEvents.filter(
      (e) =>
        e.category === "agent" && e.message === "Reusing existing agent actor",
    );
    const sessionCreated = logEvents.filter(
      (e) => e.category === "agent" && e.message === "Agent session created",
    );

    // Webview might already be created from previous tests
    // The key test is the hide/show cycle
    assert.ok(webviewHidden.length >= 1, "Webview should be hidden");
    assert.ok(
      webviewVisible.length >= 1,
      "Webview should become visible again",
    );
    // Agent might be spawned or reused depending on test order
    assert.ok(
      agentSpawned.length + agentReused.length >= 1,
      "Should spawn or reuse an agent",
    );
    assert.ok(sessionCreated.length >= 1, "Should create at least one session");

    console.log(`\nLog event summary:`);
    console.log(`- Webview created: ${webviewCreated.length}`);
    console.log(`- Webview hidden: ${webviewHidden.length}`);
    console.log(`- Webview visible: ${webviewVisible.length}`);
    console.log(`- Agent spawned: ${agentSpawned.length}`);
    console.log(`- Session created: ${sessionCreated.length}`);
  });
});
