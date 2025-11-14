// This file runs in the webview context (browser environment)
import { MynahUI, ChatItem, ChatItemType } from "@aws/mynah-ui";

// Browser API declarations for webview context
declare const acquireVsCodeApi: any;
declare const window: any & {
  SYMPOSIUM_EXTENSION_ACTIVATION_ID: string;
};

// Import uuid - note: webpack will bundle this for browser
import { v4 as uuidv4 } from "uuid";

const vscode = acquireVsCodeApi();

let mynahUI: MynahUI;

// Track accumulated agent response per tab
const tabAgentResponses: { [tabId: string]: string } = {};

// Track current message ID per tab (for MynahUI)
const tabCurrentMessageId: { [tabId: string]: string } = {};

// Track which messages we've seen per tab and mynah UI state
interface WebviewState {
  extensionActivationId: string;
  lastSeenIndex: { [tabId: string]: number };
  mynahTabs?: any; // Mynah UI tabs state
}

// Get extension activation ID from window (embedded by extension)
const currentExtensionActivationId = window.SYMPOSIUM_EXTENSION_ACTIVATION_ID;
console.log(`Extension activation ID: ${currentExtensionActivationId}`);

// Load saved state and check if we need to clear it
const savedState = vscode.getState() as WebviewState | undefined;
let lastSeenIndex: { [tabId: string]: number } = {};
let mynahTabs: any = undefined;

if (
  !savedState ||
  !savedState.extensionActivationId ||
  savedState.extensionActivationId !== currentExtensionActivationId
) {
  if (savedState) {
    console.log(
      `Extension activation ID mismatch or missing (saved: ${savedState.extensionActivationId}, current: ${currentExtensionActivationId}), clearing state`,
    );
  }
  // Clear persisted state
  vscode.setState(undefined);
  // Start fresh
  lastSeenIndex = {};
  mynahTabs = undefined;
} else {
  // Keep existing state - extension activation ID matches
  lastSeenIndex = savedState.lastSeenIndex ?? {};
  mynahTabs = savedState.mynahTabs;
  if (mynahTabs) {
    console.log("Restoring mynah tabs from saved state");
  }
}

const config: any = {
  rootSelector: "#mynah-root",
  loadStyles: true,
  config: {
    texts: {
      mainTitle: "Symposium",
      noTabsOpen: "### Join the symposium by opening a tab",
      spinnerText: "Discussing with the Symposium...",
    },
  },
  defaults: {
    store: {
      tabTitle: "Symposium",
    },
  },
  onTabAdd: (tabId: string) => {
    // Notify extension that a new tab was created
    console.log("New tab created:", tabId);
    vscode.postMessage({
      type: "new-tab",
      tabId: tabId,
    });
    // Save state when tab is added
    saveState();
  },
  onTabRemove: (tabId: string) => {
    // Save state when tab is closed
    console.log("Tab removed:", tabId);
    saveState();
  },
  onChatPrompt: (tabId: string, prompt: any) => {
    // Send prompt to extension with tabId
    vscode.postMessage({
      type: "prompt",
      tabId: tabId,
      prompt: prompt.prompt,
    });

    // Add the user's prompt to the chat
    mynahUI.addChatItem(tabId, {
      type: ChatItemType.PROMPT,
      body: prompt.prompt,
    });

    // Initialize empty response for this tab
    tabAgentResponses[tabId] = "";

    // Generate message ID for MynahUI tracking
    const messageId = uuidv4();
    tabCurrentMessageId[tabId] = messageId;

    // Add placeholder for the streaming answer
    mynahUI.addChatItem(tabId, {
      type: ChatItemType.ANSWER_STREAM,
      messageId: messageId,
      body: "",
    });

    // Save state when prompt is sent
    saveState();
  },
};

// If we have saved tabs, initialize with them
if (mynahTabs) {
  config.tabs = mynahTabs;
  console.log("Initializing MynahUI with restored tabs");
}

mynahUI = new MynahUI(config);
console.log("MynahUI initialized");

// Tell extension we're ready to receive messages
vscode.postMessage({ type: "webview-ready" });

// Save state helper
function saveState() {
  // Get current tabs from mynah UI
  const currentTabs = mynahUI?.getAllTabs();

  const state: WebviewState = {
    extensionActivationId: currentExtensionActivationId,
    lastSeenIndex,
    mynahTabs: currentTabs,
  };
  vscode.setState(state);
  console.log(
    "Saved state with extension activation ID:",
    currentExtensionActivationId,
  );
}

// Handle messages from the extension
window.addEventListener("message", (event: MessageEvent) => {
  const message = event.data;

  // Check if we've already seen this message
  const currentLastSeen = lastSeenIndex[message.tabId] ?? -1;
  if (message.index <= currentLastSeen) {
    console.log(
      `Ignoring duplicate message ${message.index} for tab ${message.tabId}`,
    );
    return;
  }

  // Process the message
  if (message.type === "agent-text") {
    // Append text to accumulated response
    tabAgentResponses[message.tabId] =
      (tabAgentResponses[message.tabId] || "") + message.text;

    // Update the chat UI with accumulated text
    mynahUI.updateLastChatAnswer(message.tabId, {
      body: tabAgentResponses[message.tabId],
    });
  } else if (message.type === "agent-complete") {
    // Mark the stream as complete using the message ID
    const messageId = tabCurrentMessageId[message.tabId];
    if (messageId) {
      mynahUI.endMessageStream(message.tabId, messageId);
    }

    // Clear accumulated response and message ID
    delete tabAgentResponses[message.tabId];
    delete tabCurrentMessageId[message.tabId];
  }

  // Update lastSeenIndex and save state
  lastSeenIndex[message.tabId] = message.index;
  saveState();

  // Send acknowledgment
  vscode.postMessage({
    type: "message-ack",
    tabId: message.tabId,
    index: message.index,
  });
});
