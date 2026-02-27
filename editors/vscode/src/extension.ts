import * as vscode from 'vscode';
import * as path from 'path';
import { exec } from 'child_process';

export function activate(context: vscode.ExtensionContext) {
    context.subscriptions.push(
        vscode.commands.registerCommand('agentDoc.submit', () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) {
                vscode.window.showWarningMessage('No active editor');
                return;
            }

            const workspaceFolder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
            if (!workspaceFolder) {
                vscode.window.showWarningMessage('File is not in a workspace');
                return;
            }

            const relativePath = path.relative(
                workspaceFolder.uri.fsPath,
                editor.document.uri.fsPath
            );

            exec(
                `agent-doc route ${relativePath}`,
                { cwd: workspaceFolder.uri.fsPath },
                (error, stdout, stderr) => {
                    if (error) {
                        vscode.window.showErrorMessage(
                            `agent-doc route failed: ${stderr || error.message}`
                        );
                        return;
                    }
                    if (stdout.trim()) {
                        vscode.window.showInformationMessage(stdout.trim());
                    }
                }
            );
        })
    );
}

export function deactivate() {}
