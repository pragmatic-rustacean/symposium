import * as vscode from "vscode";

export interface LogEvent {
  timestamp: Date;
  level: "info" | "warn" | "error";
  category: string;
  message: string;
  data?: any;
}

/**
 * Structured logger that writes to Output channel and emits events for testing
 */
export class Logger {
  private outputChannel: vscode.OutputChannel;
  private eventEmitter = new vscode.EventEmitter<LogEvent>();

  constructor(name: string) {
    this.outputChannel = vscode.window.createOutputChannel(name);
  }

  public get onLog(): vscode.Event<LogEvent> {
    return this.eventEmitter.event;
  }

  public info(category: string, message: string, data?: any): void {
    this.log("info", category, message, data);
  }

  public warn(category: string, message: string, data?: any): void {
    this.log("warn", category, message, data);
  }

  public error(category: string, message: string, data?: any): void {
    this.log("error", category, message, data);
  }

  private log(
    level: "info" | "warn" | "error",
    category: string,
    message: string,
    data?: any,
  ): void {
    const event: LogEvent = {
      timestamp: new Date(),
      level,
      category,
      message,
      data,
    };

    // Emit event for testing
    this.eventEmitter.fire(event);

    // Format for output channel
    const timestamp = event.timestamp.toISOString();
    const levelStr = level.toUpperCase().padEnd(5);
    const categoryStr = `[${category}]`.padEnd(20);
    let output = `${timestamp} ${levelStr} ${categoryStr} ${message}`;

    if (data) {
      output += `\n${JSON.stringify(data, null, 2)}`;
    }

    this.outputChannel.appendLine(output);

    // Also log to console for test output visibility
    console.log(`[${category}] ${message}`, data || "");
  }

  public show(): void {
    this.outputChannel.show();
  }

  public dispose(): void {
    this.outputChannel.dispose();
    this.eventEmitter.dispose();
  }
}
