import * as vscode from "vscode";

/**
 * Code Action Provider that shows "Discuss in Symposium" when user has a selection.
 * Appears in the Command+. quick fix menu.
 */
export class DiscussCodeActionProvider implements vscode.CodeActionProvider {
  public static readonly providedCodeActionKinds = [
    vscode.CodeActionKind.QuickFix,
  ];

  provideCodeActions(
    document: vscode.TextDocument,
    range: vscode.Range | vscode.Selection,
    _context: vscode.CodeActionContext,
    _token: vscode.CancellationToken,
  ): vscode.CodeAction[] | undefined {
    // Only show when there's a non-empty selection
    if (range.isEmpty) {
      return undefined;
    }

    const action = new vscode.CodeAction(
      "Discuss in Symposium",
      vscode.CodeActionKind.QuickFix,
    );

    action.command = {
      command: "symposium.discussSelection",
      title: "Discuss in Symposium",
    };

    // Show at top of list
    action.isPreferred = true;

    return [action];
  }
}
