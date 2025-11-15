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
vscode.postMessage({
  type: "log",
  message: "Getting saved state",
  data: { extensionActivationId: currentExtensionActivationId },
});

const savedState = vscode.getState() as WebviewState | undefined;
let lastSeenIndex: { [tabId: string]: number } = {};
let mynahTabs: any = undefined;

if (
  !savedState ||
  !savedState.extensionActivationId ||
  savedState.extensionActivationId !== currentExtensionActivationId
) {
  if (savedState) {
    vscode.postMessage({
      type: "log",
      message: "Extension activation ID mismatch - clearing state",
      data: {
        savedId: savedState.extensionActivationId,
        currentId: currentExtensionActivationId,
      },
    });
  } else {
    vscode.postMessage({
      type: "log",
      message: "No saved state found - starting fresh",
    });
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
  const tabCount = mynahTabs ? Object.keys(mynahTabs).length : 0;
  vscode.postMessage({
    type: "log",
    message: "Restoring state from previous session",
    data: { tabCount, hasLastSeenIndex: Object.keys(lastSeenIndex).length },
  });
}

// Handle approval request button clicks
function handleApprovalResponse(
  tabId: string,
  approvalId: string,
  action: "approve" | "deny" | "bypass-all",
  options: any[],
) {
  let response: any;
  let bypassAll = false;

  if (action === "approve") {
    // Find the "allow_once" option
    const allowOption = options.find((opt: any) => opt.kind === "allow_once");
    if (allowOption) {
      response = {
        outcome: { outcome: "selected", optionId: allowOption.optionId },
      };
    } else {
      response = { outcome: { outcome: "cancelled" } };
    }
  } else if (action === "deny") {
    // Find the "reject_once" option
    const rejectOption = options.find((opt: any) => opt.kind === "reject_once");
    if (rejectOption) {
      response = {
        outcome: { outcome: "selected", optionId: rejectOption.optionId },
      };
    } else {
      response = { outcome: { outcome: "cancelled" } };
    }
  } else if (action === "bypass-all") {
    // Approve this time and enable bypass
    const allowOption = options.find((opt: any) => opt.kind === "allow_once");
    if (allowOption) {
      response = {
        outcome: { outcome: "selected", optionId: allowOption.optionId },
      };
      bypassAll = true;
    } else {
      response = { outcome: { outcome: "cancelled" } };
    }
  }

  // Send response to extension
  vscode.postMessage({
    type: "approval-response",
    approvalId,
    response,
    bypassAll,
  });
}

// Handle approval request from extension
function handleApprovalRequest(message: any) {
  const { tabId, approvalId, toolCall, options } = message;

  // Log what we received for debugging
  console.log("Approval request received:", { toolCall, options });

  // Extract tool information with fallbacks
  const toolName = toolCall.title || toolCall.toolCallId || "Unknown Tool";

  // Format tool parameters for display
  let paramsDisplay = "";
  if (toolCall.rawInput && typeof toolCall.rawInput === "object") {
    paramsDisplay =
      "```json\n" + JSON.stringify(toolCall.rawInput, null, 2) + "\n```";
  }

  // Create approval card
  const messageId = `approval-${approvalId}`;
  mynahUI.addChatItem(tabId, {
    type: ChatItemType.ANSWER,
    messageId,
    body: `### Tool Permission Request\n\n**Tool:** \`${toolName}\`\n\n${paramsDisplay ? "**Parameters:**\n" + paramsDisplay : ""}`,
    buttons: [
      {
        id: "approve",
        text: "Approve",
        status: "success",
        keepCardAfterClick: false,
      },
      {
        id: "deny",
        text: "Deny",
        status: "error",
        keepCardAfterClick: false,
      },
      {
        id: "bypass-all",
        text: "Bypass Permissions",
        status: "warning",
        keepCardAfterClick: false,
      },
    ],
  });

  // Store approval context for button handler
  (window as any)[`approval_${messageId}`] = { approvalId, options };
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
  onInBodyButtonClicked: (tabId: string, messageId: string, action: any) => {
    // Check if this is an approval button
    const approvalContext = (window as any)[`approval_${messageId}`];
    if (approvalContext) {
      handleApprovalResponse(
        tabId,
        approvalContext.approvalId,
        action.id,
        approvalContext.options,
      );
      // Clean up context
      delete (window as any)[`approval_${messageId}`];
    }
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

  vscode.postMessage({
    type: "log",
    message: "Saved state",
    data: {
      extensionActivationId: currentExtensionActivationId,
      tabCount: currentTabs ? Object.keys(currentTabs).length : 0,
      lastSeenIndexCount: Object.keys(lastSeenIndex).length,
    },
  });
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
  } else if (message.type === "set-tab-title") {
    // Update the tab title
    mynahUI.updateStore(message.tabId, {
      tabTitle: message.title,
    });
  } else if (message.type === "approval-request") {
    // Display approval request UI
    handleApprovalRequest(message);
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
