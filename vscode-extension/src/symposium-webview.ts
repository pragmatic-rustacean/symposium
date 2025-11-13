// This file runs in the webview context (browser environment)
import { MynahUI, ChatItem, ChatItemType } from "@aws/mynah-ui";

// Browser API declarations for webview context
declare const acquireVsCodeApi: any;
declare const window: any;

const vscode = acquireVsCodeApi();

let messageIdCounter = 0;

// Initialize mynah-ui when the DOM is ready
const mynahUI = new MynahUI({
  rootSelector: "#mynah-root",
  loadStyles: true,
  config: {
    texts: {
      mainTitle: "Symposium",
      noTabsOpen: "### Join the symposium by opening a tab",
    },
  },
  defaults: {
    store: {
      tabTitle: "Symposium",
    },
  },
  onChatPrompt: (tabId, prompt) => {
    // User sent a prompt - send it to the extension
    const messageId = `msg-${messageIdCounter++}`;

    vscode.postMessage({
      type: "prompt",
      messageId: messageId,
      prompt: prompt.prompt,
    });

    // Add the user's prompt to the chat
    mynahUI.addChatItem(tabId, {
      type: ChatItemType.PROMPT,
      body: prompt.prompt,
    });

    // Add a placeholder for the streaming answer
    mynahUI.addChatItem(tabId, {
      type: ChatItemType.ANSWER_STREAM,
      messageId: messageId,
      body: "",
    });
  },
});

// Handle messages from the extension
window.addEventListener("message", (event: MessageEvent) => {
  const message = event.data;

  // Find the active tab
  const tabs = mynahUI.getAllTabs();
  const tabId = Object.keys(tabs).find((id) => tabs[id].isSelected);

  if (!tabId) {
    return;
  }

  if (message.type === "response-chunk") {
    // Update the streaming answer with the new chunk
    mynahUI.updateChatAnswerWithMessageId(tabId, message.messageId, {
      body: message.chunk,
    });
  } else if (message.type === "response-complete") {
    // Mark the stream as complete
    mynahUI.endMessageStream(tabId, message.messageId);
  }
});

console.log("MynahUI initialized:", mynahUI);
